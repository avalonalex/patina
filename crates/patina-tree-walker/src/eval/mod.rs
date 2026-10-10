// Module declarations
mod application;
mod apply_context_impl;
mod cps_eval;
mod debug;
mod error;
mod primitives;

// Re-export error type for public API
pub use cps_eval::{CpsEvaluator, eval_cps};
pub use error::EvalError;

// Re-export datum writer functions for use by interpreter crate
pub use patina_primitives::primitives::io::datum_writer::{
    format_display_tagged, format_write_tagged,
};

use debug::DebugConfig;
use patina_runtime::HasDiagnostic;
use patina_runtime::environment::Environment;
use patina_runtime::library_loader::LibraryLoaderRegistry;
use patina_runtime::library_registry::LibraryRegistry;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Result of evaluation step
///
/// Primitives return `Tagged` for final values, or `TailCallPrimitive`
/// to participate in tail call optimization (used by `call-with-values`).
#[derive(Debug)]
pub enum EvalResult {
    /// Final value as TaggedValue
    Tagged(patina_core::TaggedValue),
    /// Tail call to a primitive procedure with already-evaluated arguments
    TailCallPrimitive {
        proc: patina_core::TaggedValue,
        args: Vec<patina_core::TaggedValue>,
    },
}

pub struct Evaluator {
    pub global_env: Rc<Environment>,
    pub(crate) debug: Rc<DebugConfig>,
    /// Registry of loaded libraries
    pub(crate) library_registry: Rc<RefCell<LibraryRegistry>>,
    /// Registry of library loaders (Rust, Scheme, etc.)
    pub(crate) loader_registry: Rc<RefCell<LibraryLoaderRegistry>>,
    /// Registry of primitive procedures (shared across all backends via patina-primitives)
    pub(crate) primitive_registry: patina_primitives::PrimitiveRegistry,
    /// Virtual filesystem for all file I/O operations
    pub(crate) fs: Arc<dyn patina_core::FileSystem>,
    /// Garbage collector policy and state (see `docs/GC_DESIGN.md`).
    /// Serviced at trampoline safe points; always adaptive outside the
    /// differential test lanes.
    pub(crate) gc: RefCell<patina_core::GcController>,
    /// The heap's collection-pending flag, cached at construction so
    /// trampoline entry costs no heap borrow and the per-step safe point is
    /// a single load.
    pub(crate) gc_pending: Rc<std::cell::Cell<bool>>,
    /// Why `(scheme base)` could not be loaded at construction, if it could
    /// not: see [`Evaluator::bootstrap_error`].
    bootstrap_error: Option<patina_runtime::LibraryError>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::with_fs(Arc::new(patina_core::NativeFs))
    }

    /// Create an evaluator with a custom filesystem.
    ///
    /// Use this to inject a `MemoryFs` for testing or a WASM-compatible
    /// filesystem for browser targets.
    pub fn with_fs(fs: Arc<dyn patina_core::FileSystem>) -> Self {
        Self::with_fs_and_gc_mode(fs, patina_core::GcMode::from_env())
    }

    /// An evaluator collecting in `mode`, whatever `PATINA_GC`,
    /// `PATINA_GC_STRESS` or `PATINA_GC_ZEAL` say: for a test that compares
    /// collecting with not collecting, whose not-collecting side a stress
    /// lane's variable would otherwise turn into a collecting one (#626).
    /// Not an interface: Patina always collects.
    #[doc(hidden)]
    pub fn with_gc_mode(mode: patina_core::GcMode) -> Self {
        Self::with_fs_and_gc_mode(Arc::new(patina_core::NativeFs), mode)
    }

    fn with_fs_and_gc_mode(
        fs: Arc<dyn patina_core::FileSystem>,
        mode: patina_core::GcMode,
    ) -> Self {
        let global_env = Rc::new(Environment::new());
        // Counterpart of the VM's, on the same seam — see that comment.
        global_env
            .heap()
            .borrow_mut()
            .add_feature("patina-tree-walker");

        // Create primitive registry and register all primitives
        let mut primitive_registry = patina_primitives::PrimitiveRegistry::new();
        Self::register_all_primitives(&mut primitive_registry);

        // Create library registries
        let mut lib_registry = LibraryRegistry::with_default_paths();
        lib_registry.set_fs(fs.clone());
        let library_registry = Rc::new(RefCell::new(lib_registry));
        let loader_registry = Rc::new(RefCell::new(LibraryLoaderRegistry::new()));

        // Pairing heap with controller: install the policy's trigger
        // threshold (a bare heap defaults to inert) and cache the pending
        // flag the safe point reads.
        let gc = patina_core::GcController::new(mode);
        patina_core::GcController::note_backend("tree-walker");
        global_env
            .heap()
            .borrow_mut()
            .set_gc_threshold(gc.current_threshold());
        let gc_pending = global_env.heap().borrow().gc_pending_handle();

        let mut evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
            library_registry,
            loader_registry,
            primitive_registry,
            fs,
            gc: RefCell::new(gc),
            gc_pending,
            bootstrap_error: None,
        };

        // Initialize library loaders
        evaluator.init_loaders();
        LibraryLoaderRegistry::install_availability_checker(
            evaluator.global_env.heap(),
            &evaluator.library_registry,
            &evaluator.loader_registry,
        );

        // Load bootstrap library
        evaluator.bootstrap_error = evaluator.load_bootstrap();

        evaluator
    }

    /// Initialize library loaders
    ///
    /// Sets up the loader registry with:
    /// 1. RustLibraryLoader for internal libraries (patina internal ...)
    /// 2. RustLibraryLoader for legacy scheme libraries (transitional)
    /// 3. SchemeLibraryLoader for .sld files
    fn init_loaders(&self) {
        use crate::library_support::SchemeLibraryLoader;
        use patina_runtime::{RustLibraryLoader, stdlib};

        let mut loaders = self.loader_registry.borrow_mut();

        // Create Rust loader and register libraries
        let mut rust_loader = RustLibraryLoader::with_standard_libraries();

        // === Internal libraries (patina internal ...) ===
        // These are domain-specific primitive collections used by .sld files
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "numbers".to_string(),
            ],
            stdlib::build_internal_numbers,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "lists".to_string(),
            ],
            stdlib::build_internal_lists,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "chars".to_string(),
            ],
            stdlib::build_internal_chars,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "strings".to_string(),
            ],
            stdlib::build_internal_strings,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "vectors".to_string(),
            ],
            stdlib::build_internal_vectors,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "bytevectors".to_string(),
            ],
            stdlib::build_internal_bytevectors,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "bitwise".to_string(),
            ],
            stdlib::build_internal_bitwise,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "control".to_string(),
            ],
            stdlib::build_internal_control,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "errors".to_string(),
            ],
            stdlib::build_internal_errors,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "io".to_string(),
            ],
            stdlib::build_internal_io,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "predicates".to_string(),
            ],
            stdlib::build_internal_predicates,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "records".to_string(),
            ],
            stdlib::build_internal_records,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "params".to_string(),
            ],
            stdlib::build_internal_params,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "time".to_string(),
            ],
            stdlib::build_internal_time,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "system".to_string(),
            ],
            stdlib::build_internal_system,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "lazy".to_string(),
            ],
            stdlib::build_internal_lazy,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "ephemeron".to_string(),
            ],
            stdlib::build_internal_ephemeron,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "syntax".to_string(),
            ],
            stdlib::build_internal_syntax,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "eval".to_string(),
            ],
            stdlib::build_internal_eval,
        );
        rust_loader.register(
            vec![
                "patina".to_string(),
                "internal".to_string(),
                "r5rs".to_string(),
            ],
            stdlib::build_internal_r5rs,
        );

        // === R7RS libraries are now loaded from .sld files ===
        // (scheme base)           -> lib/scheme/base.sld
        // (scheme char)           -> lib/scheme/char.sld
        // (scheme complex)        -> lib/scheme/complex.sld
        // (scheme inexact)        -> lib/scheme/inexact.sld
        // (scheme lazy)           -> lib/scheme/lazy.sld
        // (scheme time)           -> lib/scheme/time.sld
        // (scheme file)           -> lib/scheme/file.sld
        // (scheme read)           -> lib/scheme/read.sld
        // (scheme write)          -> lib/scheme/write.sld
        // (scheme eval)           -> lib/scheme/eval.sld
        // (scheme process-context)-> lib/scheme/process-context.sld
        // (scheme cxr)            -> lib/scheme/cxr.sld
        // (scheme r5rs)           -> lib/scheme/r5rs.sld

        // === Patina extensions ===
        rust_loader.register(
            vec!["patina".to_string(), "debug".to_string()],
            stdlib::build_patina_debug,
        );

        // Add Rust loader first (highest priority)
        loaders.add_loader(Box::new(rust_loader));

        // Add Scheme loader second (for .sld files)
        loaders.add_evaluating_loader(Box::new(SchemeLibraryLoader::new(self.fs.clone())));
    }

    /// Load the bootstrap libraries into the global environment, returning
    /// why `(scheme base)` could not be loaded, if it could not.
    fn load_bootstrap(&self) -> Option<patina_runtime::LibraryError> {
        // Load (scheme base) library, lib/scheme/base.sld
        #[expect(
            clippy::disallowed_methods,
            reason = "bootstrap, from outside any loop, so the load may collect: holds nothing. \
                      `(scheme base)` is an `.sld`, whose body and environment its registry entry \
                      roots while it loads (#677)"
        )]
        let base = self
            .load_library(&["scheme".to_string(), "base".to_string()])
            .err();

        // Load Patina debugging utilities
        // Auto-loaded in REPL for convenience (commonly used during development)
        #[expect(
            clippy::disallowed_methods,
            reason = "bootstrap: holds nothing. `(patina debug)` is built in Rust and runs no \
                      Scheme"
        )]
        let _ = self.load_library(&["patina".to_string(), "debug".to_string()]);

        // After loading libraries, import (scheme base) into global environment
        // This makes primitives and macros available without explicit import
        // (R7RS-style convenience for REPL)
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["scheme".to_string(), "base".to_string()])
        {
            for name in lib.export_names() {
                lib.import_into(&self.global_env, name, name);
            }
        }

        // `import` and the reserved `expand` keyword are not `(scheme base)`
        // exports. Seed them so the frontend can handle or diagnose them.
        patina_runtime::stdlib::seed_top_level_syntax(&self.global_env);

        // Import (patina debug) into global environment for REPL convenience
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["patina".to_string(), "debug".to_string()])
        {
            for name in lib.export_names() {
                lib.import_into(&self.global_env, name, name);
            }
        }
        base
    }

    /// Why the base library, `(scheme base)`, could not be loaded when this
    /// evaluator was made — `lib/` not found, most often. Every program then
    /// runs in an environment with nothing in it, so the CLI says this once,
    /// up front, instead (#436).
    pub fn bootstrap_error(&self) -> Option<&patina_runtime::LibraryError> {
        self.bootstrap_error.as_ref()
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// The evaluator made the heap, so dropping it tears the heap down, as
/// dropping a VM backend does (`Heap::teardown`, #604).
impl Drop for Evaluator {
    fn drop(&mut self) {
        if let Ok(mut heap) = self.global_env.heap().try_borrow_mut() {
            heap.teardown();
        }
    }
}

// Library loading methods
impl Evaluator {
    /// Load a library by name
    ///
    /// This method:
    /// 1. Checks if the library is already loaded
    /// 2. If not, uses the loader registry to load it
    /// 3. Registers it in the library registry
    ///
    /// Returns the loaded library or an error.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `load_library_with` with a detached context: holds nothing"
    )]
    pub fn load_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_runtime::Library>, patina_runtime::LibraryError> {
        let cps = cps_eval::CpsEvaluator::new(self);
        self.load_library_with(name, &cps_eval::CallbackContext::detached(&cps))
    }

    fn load_library_with(
        &self,
        name: &[String],
        context: &cps_eval::CallbackContext<'_, '_, '_>,
    ) -> Result<Rc<patina_runtime::Library>, patina_runtime::LibraryError> {
        // Check if already loaded
        {
            let registry = self.library_registry.borrow();
            if let Some(lib) = registry.get(name) {
                return Ok(Rc::new(lib.clone()));
            }
        }

        // Check for circular dependencies, ended on every way out (#436).
        let loading = LibraryRegistry::begin_loading_scoped(&self.library_registry, name)?;

        // Get search paths (copy to avoid borrow conflicts)
        let search_paths = {
            let registry = self.library_registry.borrow();
            registry.search_paths().to_vec()
        };

        // Try simple loaders first (Rust libraries)
        // Pass the global heap so library environments share the same heap for TaggedValue compatibility
        let lib_result = {
            let loaders = self.loader_registry.borrow();
            loaders.try_simple_load_with_heap(
                name,
                &search_paths,
                self.global_env.heap().clone(),
            )?
        };

        let (lib, loaded_via_rust) = if let Some(lib) = lib_result {
            // Simple loader succeeded (Rust library)
            (lib, true)
        } else {
            // Try evaluating loaders (Scheme .sld files)
            // Create a library availability checker for cond-expand
            let can_load_library = |lib_name: &[String]| {
                patina_frontend::cond_expand::library_available(self.global_env.heap(), lib_name)
            };

            let parsed = {
                let loaders = self.loader_registry.borrow();
                loaders.try_parse_with_heap_and_library_checker(
                    name,
                    &search_paths,
                    self.global_env.heap().clone(),
                    &can_load_library,
                )?
            };

            match parsed {
                // Parse succeeded, now evaluate
                Some(parsed) => (
                    self.evaluate_parsed_library(parsed, context, &loading)?,
                    false,
                ),
                // No loader can handle this library
                None => {
                    return Err(patina_runtime::LibraryError::not_found_in(
                        name,
                        &search_paths,
                    ));
                }
            }
        };

        // End loading tracking
        drop(loading);

        // Register the library first
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.register(lib)?;
        }

        // A library built in Rust exports everything in its environment.
        if loaded_via_rust {
            let mut registry = self.library_registry.borrow_mut();
            if let Some(library) = registry.get_mut(name) {
                let all_bindings = library.env.bindings();
                library.clear_exports();
                for (binding_name, value) in all_bindings {
                    library.export_tagged(binding_name, value);
                }
            }
        }

        // Point B, between this library and the next the importer loads.
        self.collect_if_pending();

        // Return the registered library
        let lib_rc = {
            let registry = self.library_registry.borrow();
            Rc::new(registry.get(name).cloned().expect("Library should exist"))
        };

        Ok(lib_rc)
    }

    /// This backend's primitive registry: `patina_primitives::register_all`
    /// plus what this backend adds on top (see
    /// `primitives::register_all_primitives`).
    ///
    /// Exposed for the test that asserts every name a shipped library exports
    /// can actually be dispatched here. That check needs *this* registry
    /// rather than the shared one, because the backend-specific entries are
    /// exactly what it is about.
    pub fn primitive_registry(&self) -> &patina_primitives::PrimitiveRegistry {
        &self.primitive_registry
    }

    /// Check if a library is loaded
    pub fn is_library_loaded(&self, name: &[String]) -> bool {
        self.library_registry.borrow().is_loaded(name)
    }

    /// Get a loaded library
    pub fn get_library(&self, name: &[String]) -> Option<Rc<patina_runtime::Library>> {
        self.library_registry
            .borrow()
            .get(name)
            .map(|lib| Rc::new(lib.clone()))
    }

    /// Get the library search paths
    pub fn library_search_paths(&self) -> Vec<PathBuf> {
        self.library_registry.borrow().search_paths().to_vec()
    }

    /// Find a library file in the search paths
    pub fn find_library_file(&self, name: &[String]) -> Option<PathBuf> {
        self.library_registry.borrow().find_library_file(name)
    }

    /// Add a library search path (for testing)
    pub fn add_library_search_path(&self, path: PathBuf) {
        self.library_registry.borrow_mut().add_search_path(path);
    }

    /// Add a library search path ahead of every existing one (the CLI's `-I`).
    pub fn prepend_library_search_path(&self, path: PathBuf) {
        self.library_registry.borrow_mut().prepend_search_path(path);
    }

    /// Evaluate an inline `(define-library ...)` form.
    ///
    /// Parses the datum with the same loader the `.sld` path uses (includes
    /// resolve against the current directory), evaluates the body, and
    /// registers the library — replacing a previous same-named one, so
    /// re-evaluating the form at the REPL redefines it.
    pub fn eval_inline_define_library(
        &self,
        form: patina_core::TaggedValue,
    ) -> Result<(), patina_runtime::LibraryError> {
        use patina_frontend::SchemeLibraryLoader;

        let can_load_library = |lib_name: &[String]| {
            patina_frontend::cond_expand::library_available(self.global_env.heap(), lib_name)
        };
        let loader = SchemeLibraryLoader::new(self.fs.clone());
        let parsed = loader.parse_inline_form(
            form,
            std::path::Path::new("."),
            self.global_env.heap().clone(),
            &can_load_library,
        )?;

        let loading = LibraryRegistry::begin_loading_scoped(&self.library_registry, &parsed.name)?;
        let cps = cps_eval::CpsEvaluator::new(self);
        let result = self.evaluate_parsed_library(
            parsed,
            &cps_eval::CallbackContext::detached(&cps),
            &loading,
        );
        drop(loading);
        let lib = result?;
        self.library_registry.borrow_mut().register_or_replace(lib);
        // Point B: a run of `define-library` forms runs no other code that
        // could collect (#614).
        self.collect_if_pending();
        Ok(())
    }

    /// Point B (#677): a collection pending where a load ends runs now, if
    /// nothing defers here — no trampoline running, no holder's guard alive.
    /// Anywhere else the running trampoline's own safe points service it.
    fn collect_if_pending(&self) {
        cps_eval::CpsEvaluator::new(self).collect_if_pending();
    }

    /// Evaluate a parsed library
    ///
    /// This method is called after a library is parsed from a .sld file.
    /// It handles import resolution, body evaluation, and export collection.
    ///
    /// The body and `lib_env` go into `loading`'s registry entry first,
    /// which roots them until the load ends, so the load collects where it
    /// runs on the outermost trampoline: inside each body form, and between
    /// the forms and the libraries it imports (#677). A load requested by
    /// running code, on a nested trampoline, or under a guard collects at
    /// none of these.
    fn evaluate_parsed_library(
        &self,
        parsed: patina_runtime::library_loader::ParsedLibrary,
        context: &cps_eval::CallbackContext<'_, '_, '_>,
        loading: &patina_runtime::library_registry::Loading,
    ) -> Result<patina_runtime::Library, patina_runtime::LibraryError> {
        // Create a fresh environment for this library, sharing global heap for TaggedValue compatibility
        let lib_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
        let declarations = parsed.hand_to(loading, &lib_env);

        // Step 1: Resolve imports
        for import_set in &declarations.imports {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds `lib_env` and the body across each import's load, which this \
                          load's registry entry roots (#677); the declarations hold no value"
            )]
            self.process_import_set(import_set, &lib_env, context)?;
        }

        // Step 2: Evaluate library body (definitions only)
        // Use CPS evaluation so that all lambdas become CpsLambdas, enabling
        // proper continuation support throughout the codebase.
        // A relative `include` in the body resolves beside the `.sld`.
        let source = declarations.source.as_deref();
        let desugarer = patina_frontend::Desugarer::with_env(lib_env.clone())
            .with_fs(self.fs.clone())
            .with_include_base_of(source);
        let shared_heap = lib_env.heap().clone();
        while let Some(form) = loading.next_form() {
            // Desugar TaggedValue to CoreExpr
            #[expect(
                clippy::disallowed_methods,
                reason = "the import callback loads libraries during the expansion, which holds \
                          the form and the partial expansion: guarded by `desugar_with_imports`' \
                          `GcDeferGuard::holding`"
            )]
            let core_expr = desugarer.desugar_with_imports(
                form,
                &shared_heap,
                |set, env| self.process_import_set(set, env, context),
                |e| {
                    patina_runtime::LibraryError::processing(
                        source,
                        format!("Failed to desugar expression: {e}"),
                        e.diagnostic(),
                    )
                    .at_opt(e.source_location().cloned())
                },
            )?;

            // Initialization runs under the importing program's dynamic
            // context. A guard can leave the load here: preserve that escape
            // and do not evaluate later forms or register a partial library.
            #[expect(
                clippy::disallowed_methods,
                reason = "outermost when the load is, so the run may collect (point D, #677). \
                          Holds `core_expr`, not read after the call, and `lib_env` and the body's \
                          later forms, which this load's registry entry roots; the run roots the \
                          CPS tree it is entered with"
            )]
            context.eval_core(&core_expr, &lib_env).map_err(|e| {
                patina_runtime::LibraryError::EvaluationError {
                    file: source.map(|p| p.display().to_string()).unwrap_or_default(),
                    error: Box::new(e),
                }
            })?;
        }

        // Step 3: Assemble the library and resolve its exports
        patina_runtime::library_loader::build_library(declarations, lib_env)
    }

    /// Process a single import set
    fn process_import_set(
        &self,
        import_set: &patina_runtime::library_loader::ImportSet,
        lib_env: &Rc<Environment>,
        context: &cps_eval::CallbackContext<'_, '_, '_>,
    ) -> Result<(), patina_runtime::LibraryError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "holds nothing of its own: `lib_env` is the caller's, and its call site gives \
                      the reason"
        )]
        let library = self.load_library_with(import_set.library_name(), context)?;
        for binding in import_set.resolve_bindings(library.export_names()) {
            let (name, export) = binding?;
            library.import_into(lib_env, name, &export);
        }
        Ok(())
    }

    /// Process an import set for eval context
    ///
    /// This imports library identifiers into a regular environment (not building a library).
    /// Used by the `import` special form.
    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `process_import_for_eval_with` with a detached context: holds \
                  nothing"
    )]
    pub fn process_import_for_eval(
        &self,
        import_set: &patina_frontend::ImportSet,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        let cps = cps_eval::CpsEvaluator::new(self);
        self.process_import_for_eval_with(
            import_set,
            env,
            &cps_eval::CallbackContext::detached(&cps),
        )
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a wrapper over `process_import_set`: holds nothing"
    )]
    fn process_import_for_eval_with(
        &self,
        import_set: &patina_frontend::ImportSet,
        env: &Rc<Environment>,
        context: &cps_eval::CallbackContext<'_, '_, '_>,
    ) -> Result<(), EvalError> {
        self.process_import_set(import_set, env, context)
            .map_err(patina_runtime::LibraryError::into_eval_error)
    }
}
