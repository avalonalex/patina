//! `Backend` trait implementation for the VM.
//!
//! `VmBackend` wraps `VmState` and implements the `patina_runtime::Backend`
//! trait so the VM can be used anywhere a `TreeWalker` is accepted, including
//! `Interpreter<VmBackend>` and the REPL.
//!
//! ## Error bridging
//!
//! `Backend::Error` must be `Send + Sync + 'static`, but `VmError` holds
//! `Symbol = Rc<str>` which is not `Send`. We define `VmBackendError` — a
//! thin wrapper that converts all variable names to `String` on construction.

use crate::compiler::compile_with_qq_resolving;
use crate::error::VmError;
use crate::runtime::vm_state::import_export;
use crate::runtime::{VmState, execute};
use patina_core::environment::Environment;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use patina_frontend::{Desugarer, SchemeLibraryLoader};
use patina_runtime::HasDiagnostic;
use patina_runtime::library_loader::{ImportSet, build_library};
use patina_runtime::library_registry::LibraryError;
use patina_runtime::{
    Backend, Library, LibraryLoaderRegistry, LibraryRegistry, RustLibraryLoader, stdlib,
};
use std::cell::RefCell;
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// VmBackendError — Send + Sync wrapper around VmError
// ─────────────────────────────────────────────────────────────────────────────

/// Backend-visible error: all variable names converted to `String` so the type
/// is `Send + Sync + 'static` as required by the `Backend` trait.
///
/// Source locations are preserved for error formatting with caret context.
#[derive(Debug, thiserror::Error)]
pub enum VmBackendError {
    #[error("{error}")]
    WithDiagnostic {
        error: Box<VmBackendError>,
        diagnostic: Box<patina_runtime::Diagnostic>,
    },

    #[error("compile error: {0}")]
    Compile(String),

    #[error("{message}")]
    Runtime {
        message: String,
        location: Option<SourceLocation>,
    },

    /// Rejected before it ran. `location` is the form the desugarer was in
    /// when it refused, where it knew one (#432).
    #[error("desugar error: {message}")]
    Desugar {
        message: String,
        location: Option<SourceLocation>,
    },
}

impl VmBackendError {
    fn with_diagnostic(self, diagnostic: patina_runtime::Diagnostic) -> Self {
        Self::WithDiagnostic {
            error: Box::new(self),
            diagnostic: Box::new(diagnostic),
        }
    }

    /// Return the source location attached to this error, if any.
    pub fn source_location(&self) -> Option<&SourceLocation> {
        match self {
            VmBackendError::Runtime { location, .. } | VmBackendError::Desugar { location, .. } => {
                location.as_ref()
            }
            VmBackendError::Compile(_) => None,
            VmBackendError::WithDiagnostic { error, .. } => error.source_location(),
        }
    }
}

impl patina_core::error::HasSourceLocation for VmBackendError {
    fn source_location(&self) -> Option<&SourceLocation> {
        self.source_location()
    }
}

impl From<VmError> for VmBackendError {
    fn from(e: VmError) -> Self {
        let location = e.source_location().cloned();
        VmBackendError::Runtime {
            message: e.to_string(),
            location,
        }
        .with_diagnostic(e.diagnostic())
    }
}

impl From<patina_frontend::DesugarError> for VmBackendError {
    fn from(e: patina_frontend::DesugarError) -> Self {
        VmBackendError::Desugar {
            message: e.to_string(),
            location: e.source_location().cloned(),
        }
        .with_diagnostic(e.diagnostic())
    }
}

impl From<crate::error::CompileError> for VmBackendError {
    fn from(e: crate::error::CompileError) -> Self {
        VmBackendError::Compile(e.to_string()).with_diagnostic(e.diagnostic())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VmBackend
// ─────────────────────────────────────────────────────────────────────────────

/// VM backend implementing the `Backend` trait.
///
/// Holds a `VmState` and compiles + executes each expression on demand.
/// The heap is shared with the parser so `TaggedValue` indices remain valid.
pub struct VmBackend {
    state: RefCell<VmState>,
    global_env: Rc<Environment>,
    library_registry: Rc<RefCell<LibraryRegistry>>,
    loader_registry: Rc<RefCell<LibraryLoaderRegistry>>,
    /// Why `(scheme base)` could not be loaded at construction, if it could
    /// not: see [`VmBackend::bootstrap_error`].
    bootstrap_error: Option<LibraryError>,
}

impl VmBackend {
    /// Why the base library, `(scheme base)`, could not be loaded when this
    /// backend was made — `lib/` not found, most often. Every program then
    /// runs in an environment with nothing in it, so the CLI says this once,
    /// up front, instead (#436).
    pub fn bootstrap_error(&self) -> Option<&LibraryError> {
        self.bootstrap_error.as_ref()
    }

    /// How many compiled code objects the VM is holding: what a test checks to
    /// see the code of finished forms let go (#338). Not an interface.
    #[doc(hidden)]
    pub fn loaded_code_objects(&self) -> usize {
        self.state.borrow().loaded_code_objects()
    }

    /// How many slots the VM's code store has, holding code or not: what a
    /// test checks to see slots given to later code rather than added (#352).
    /// Not an interface.
    #[doc(hidden)]
    pub fn code_store_slots(&self) -> usize {
        self.state.borrow().code_store_slots()
    }

    /// Create a new VM backend with a fresh environment and primitive registry.
    pub fn new() -> Self {
        Self::with_fs(std::sync::Arc::new(patina_core::NativeFs))
    }

    /// Create a VM backend with a custom filesystem.
    pub fn with_fs(fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        Self::with_fs_and_gc_mode(fs, patina_core::GcMode::from_env())
    }

    /// A VM backend collecting in `mode`, whatever `PATINA_GC`,
    /// `PATINA_GC_STRESS` or `PATINA_GC_ZEAL` say: for a test that compares
    /// collecting with not collecting, whose not-collecting side a stress
    /// lane's variable would otherwise turn into a collecting one (#626).
    /// Not an interface: Patina always collects.
    #[doc(hidden)]
    pub fn with_gc_mode(mode: patina_core::GcMode) -> Self {
        Self::with_fs_and_gc_mode(std::sync::Arc::new(patina_core::NativeFs), mode)
    }

    fn with_fs_and_gc_mode(
        fs: std::sync::Arc<dyn patina_core::FileSystem>,
        mode: patina_core::GcMode,
    ) -> Self {
        let global_env = Rc::new(Environment::new());
        // Name this backend to `cond-expand` and `(features)`, once, on the
        // heap they both read. `crates/patina-tests/tests/backend_feature.rs`
        // covers every shape that reaches a `cond-expand` — a program, `eval`,
        // a library body, a `.sld` declaration, a quasiquote — because a
        // missed one does not error, it silently takes the `else` branch.
        global_env.heap().borrow_mut().add_feature("patina-vm");
        let mut state = VmState::with_gc_mode(Rc::clone(&global_env), mode);
        // Deliberately *not* `install_primitives()` — that bound every
        // registered primitive into globals regardless of the import set
        // (`cadddr` was callable with only `(scheme base)` imported).
        // `load_bootstrap()` below defines exactly the exports a fresh top
        // level starts with, as the tree-walker already did. The primitive
        // fast path is unaffected: library-bound primitives resolve their
        // registry index on first call via `resolve_index_cached`. Guarded by
        // `import_set_is_enforced.rs`; history in
        // `PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.

        // Set up library loading infrastructure (Rc-shared with VmState)
        let mut lib_registry = LibraryRegistry::with_default_paths();
        lib_registry.set_fs(fs.clone());
        let library_registry = Rc::new(RefCell::new(lib_registry));
        let loader_registry = Rc::new(RefCell::new(LibraryLoaderRegistry::new()));

        // Share registries with VmState so eval primitives can load libraries
        state.configure_libraries(
            fs,
            Rc::clone(&library_registry),
            Rc::clone(&loader_registry),
        );

        let mut backend = VmBackend {
            state: RefCell::new(state),
            global_env,
            library_registry,
            loader_registry,
            bootstrap_error: None,
        };

        // Initialize library loaders
        backend.init_loaders();
        LibraryLoaderRegistry::install_availability_checker(
            backend.global_env.heap(),
            &backend.library_registry,
            &backend.loader_registry,
        );

        // Load bootstrap libraries (scheme base, etc.)
        backend.bootstrap_error = backend.load_bootstrap();

        backend
    }

    /// Attach a structured tracer for instruction-level debugging.
    pub fn set_tracer(&self, tracer: Option<crate::tracer::TracerHandle>) {
        self.state.borrow_mut().set_tracer(tracer);
    }

    /// Shared body of `eval` and `eval_with_source_map` — the two entries
    /// differ only in desugarer construction.
    ///
    /// We always evaluate in the global environment (same as the tree-walker
    /// does for top-level defines).
    fn eval_datum(
        &self,
        expr: TaggedValue,
        source_map: Option<&Rc<RefCell<patina_frontend::SourceMap>>>,
    ) -> Result<TaggedValue, VmBackendError> {
        let heap = self.global_env.heap().clone();

        // An inline (define-library ...) is a library definition, not an
        // expression — route it to the library loader before desugaring.
        if patina_frontend::is_define_library_form(expr, &heap) {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds the `define-library` datum, not read after the call: the library's \
                          body forms are in its `ParsedLibrary`, which holds \
                          `GcDeferGuard::holding` while it lives, so no collection runs during the \
                          load"
            )]
            self.eval_inline_define_library(expr).map_err(|e| {
                VmBackendError::Runtime {
                    message: e.to_string(),
                    location: None,
                }
                .with_diagnostic(e.diagnostic())
            })?;
            return Ok(TaggedValue::UNSPECIFIED);
        }

        // Desugar: TaggedValue → CoreExpr.
        let desugarer = match source_map {
            Some(sm) => Desugarer::with_env_and_source_map(Rc::clone(&self.global_env), sm.clone())
                .with_fs(self.state.borrow().fs().clone()),
            None => Desugarer::with_env(Rc::clone(&self.global_env))
                .with_fs(self.state.borrow().fs().clone()),
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "the import callback loads libraries during the expansion, which holds the \
                      datum and the partial expansion: guarded by `desugar_with_imports`' \
                      `GcDeferGuard::holding`"
        )]
        let core_expr = desugarer.desugar_with_imports(
            expr,
            &heap,
            |set, env| {
                self.process_import_set(set, env).map_err(|e| {
                    VmBackendError::Runtime {
                        message: e.to_string(),
                        location: e.source_location().cloned(),
                    }
                    .with_diagnostic(e.diagnostic())
                })
            },
            VmBackendError::from,
        )?;

        // Compile: CoreExpr → CodeObject (5-pass pipeline + quasiquote expansion).
        let registry = Rc::clone(self.state.borrow().primitive_registry());
        let (top, nested) =
            compile_with_qq_resolving(&core_expr, &heap, &self.global_env, &registry)?;

        let mut state = self.state.borrow_mut();
        let top_id = state.load_unit(top, nested);

        // Execute, then let go of the form's code unless something it left
        // behind can run it again (#338).
        #[expect(
            clippy::disallowed_methods,
            reason = "the backend's top level, outermost, so the run may collect. Holds the form's \
                      `CoreExpr` and datum, neither read after the call; the form's constants are \
                      in the loaded unit, which the code store roots while it runs"
        )]
        let result = execute(&mut state, top_id);
        state.release_unit_if_unused(top_id);
        Ok(result?)
    }

    /// Initialize library loaders (Rust internal libs + Scheme .sld loader).
    fn init_loaders(&self) {
        let mut loaders = self.loader_registry.borrow_mut();

        let mut rust_loader = RustLibraryLoader::with_standard_libraries();

        // === Internal libraries (patina internal ...) ===
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "numbers".into()],
            stdlib::build_internal_numbers,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "lists".into()],
            stdlib::build_internal_lists,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "chars".into()],
            stdlib::build_internal_chars,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "strings".into()],
            stdlib::build_internal_strings,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "vectors".into()],
            stdlib::build_internal_vectors,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "bytevectors".into()],
            stdlib::build_internal_bytevectors,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "bitwise".into()],
            stdlib::build_internal_bitwise,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "control".into()],
            stdlib::build_internal_control,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "errors".into()],
            stdlib::build_internal_errors,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "io".into()],
            stdlib::build_internal_io,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "predicates".into()],
            stdlib::build_internal_predicates,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "records".into()],
            stdlib::build_internal_records,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "params".into()],
            stdlib::build_internal_params,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "time".into()],
            stdlib::build_internal_time,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "system".into()],
            stdlib::build_internal_system,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "lazy".into()],
            stdlib::build_internal_lazy,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "ephemeron".into()],
            stdlib::build_internal_ephemeron,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "syntax".into()],
            stdlib::build_internal_syntax,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "eval".into()],
            stdlib::build_internal_eval,
        );
        rust_loader.register(
            vec!["patina".into(), "internal".into(), "r5rs".into()],
            stdlib::build_internal_r5rs,
        );

        // === Patina extensions ===
        rust_loader.register(
            vec!["patina".into(), "debug".into()],
            stdlib::build_patina_debug,
        );

        // Add Rust loader first (highest priority)
        loaders.add_loader(Box::new(rust_loader));

        // Add Scheme loader for .sld files
        loaders.add_evaluating_loader(Box::new(SchemeLibraryLoader::new(
            self.state.borrow().fs().clone(),
        )));
    }

    /// Load bootstrap libraries and import them into the global environment,
    /// returning why `(scheme base)` could not be loaded, if it could not.
    fn load_bootstrap(&self) -> Option<LibraryError> {
        // Load (scheme base)
        #[expect(
            clippy::disallowed_methods,
            reason = "bootstrap, from outside any loop: holds nothing. `(scheme base)` is an \
                      `.sld`, whose body runs while its `ParsedLibrary` holds \
                      `GcDeferGuard::holding`"
        )]
        let base = self.load_library(&["scheme".into(), "base".into()]).err();

        // Load Patina debug utilities
        #[expect(
            clippy::disallowed_methods,
            reason = "bootstrap: holds nothing. `(patina debug)` is built in Rust and runs no \
                      Scheme"
        )]
        let _ = self.load_library(&["patina".into(), "debug".into()]);

        // Import (scheme base) into global environment
        if let Ok(lib) = self.get_loaded_library(&["scheme".into(), "base".into()]) {
            for name in lib.export_names() {
                lib.import_into(&self.global_env, name, name);
            }
        }

        // Import (patina debug) into global environment
        if let Ok(lib) = self.get_loaded_library(&["patina".into(), "debug".into()]) {
            for name in lib.export_names() {
                lib.import_into(&self.global_env, name, name);
            }
        }

        // `import` and the reserved `expand` keyword are not `(scheme base)`
        // exports. Seed them so the frontend can handle or diagnose them.
        stdlib::seed_top_level_syntax(&self.global_env);
        base
    }

    /// Get an already-loaded library by name.
    fn get_loaded_library(&self, name: &[String]) -> Result<Library, LibraryError> {
        self.library_registry
            .borrow()
            .get(name)
            .cloned()
            .ok_or_else(|| LibraryError::not_found(name))
    }

    /// Return library search paths from the library registry.
    fn library_search_paths(&self) -> Vec<std::path::PathBuf> {
        self.library_registry.borrow().search_paths().to_vec()
    }

    /// Add a library search path (for testing).
    ///
    /// Counterpart of `Evaluator::add_library_search_path` on the
    /// tree-walker, so tests that resolve libraries outside `lib/` (the
    /// upstream SRFI suites) can run on both backends.
    pub fn add_library_search_path(&self, path: std::path::PathBuf) {
        self.library_registry.borrow_mut().add_search_path(path);
    }

    /// Add a library search path ahead of every existing one (the CLI's `-I`).
    pub fn prepend_library_search_path(&self, path: std::path::PathBuf) {
        self.library_registry.borrow_mut().prepend_search_path(path);
    }

    /// Evaluate an inline `(define-library ...)` form.
    ///
    /// Parses the datum with the same loader the `.sld` path uses (includes
    /// resolve against the current directory), evaluates the body, and
    /// registers the library — replacing a previous same-named one, so
    /// re-evaluating the form at the REPL redefines it.
    fn eval_inline_define_library(&self, form: TaggedValue) -> Result<(), LibraryError> {
        let heap = self.global_env.heap().clone();
        let can_load_library =
            |lib_name: &[String]| patina_frontend::cond_expand::library_available(&heap, lib_name);
        let loader = SchemeLibraryLoader::new(self.state.borrow().fs().clone());
        let parsed = loader.parse_inline_form(
            form,
            std::path::Path::new("."),
            heap.clone(),
            &can_load_library,
        )?;

        let loading = LibraryRegistry::begin_loading_scoped(&self.library_registry, &parsed.name)?;
        let result = self.evaluate_parsed_library(parsed);
        drop(loading);
        let lib = result?;
        self.library_registry.borrow_mut().register_or_replace(lib);
        Ok(())
    }

    /// Compile a source string to bytecode and disassemble it to stdout.
    ///
    /// Used by the `(vm-compile ...)` REPL special form.
    pub fn disasm_source(&self, source: &str) -> Result<(), VmBackendError> {
        use crate::disasm::disassemble;
        use patina_frontend::Parser;

        let heap = self.global_env.heap().clone();
        let parsed = Parser::new_with_heap(source, heap.clone())
            .map_err(|e| VmBackendError::Compile(e.to_string()).with_diagnostic(e.diagnostic()))?
            .parse_all()
            .map_err(|e| VmBackendError::Compile(e.to_string()).with_diagnostic(e.diagnostic()))?;

        let desugarer = Desugarer::with_env(Rc::clone(&self.global_env))
            .with_fs(self.state.borrow().fs().clone());
        for tv in parsed {
            let core_expr = desugarer
                .desugar_tagged(tv, &heap)
                .map_err(VmBackendError::from)?;

            let registry = Rc::clone(self.state.borrow().primitive_registry());
            let (top, nested) =
                compile_with_qq_resolving(&core_expr, &heap, &self.global_env, &registry)?;

            disassemble(&top, &nested);
        }
        Ok(())
    }

    /// Load a library by name (shared heap variant for TaggedValue compatibility).
    pub fn load_library(&self, name: &[String]) -> Result<Library, LibraryError> {
        // Check if already loaded
        {
            let registry = self.library_registry.borrow();
            if let Some(lib) = registry.get(name) {
                return Ok(lib.clone());
            }
        }

        // Circular dependency detection, ended on every way out (#436).
        let loading = LibraryRegistry::begin_loading_scoped(&self.library_registry, name)?;

        let search_paths = self.library_search_paths();
        let heap = self.global_env.heap().clone();

        // Try simple (Rust) loaders first
        let rust_result = {
            let loaders = self.loader_registry.borrow();
            loaders.try_simple_load_with_heap(name, &search_paths, heap.clone())?
        };

        let lib = if let Some(lib) = rust_result {
            lib
        } else {
            // Try evaluating (Scheme .sld) loaders
            let can_load_library = |lib_name: &[String]| {
                patina_frontend::cond_expand::library_available(&heap, lib_name)
            };

            let parsed = {
                let loaders = self.loader_registry.borrow();
                loaders.try_parse_with_heap_and_library_checker(
                    name,
                    &search_paths,
                    heap.clone(),
                    &can_load_library,
                )?
            };

            match parsed {
                Some(parsed) => self.evaluate_parsed_library(parsed)?,
                None => return Err(LibraryError::not_found_in(name, &search_paths)),
            }
        };

        // End loading tracking
        drop(loading);

        // Register the library
        let _ = self.library_registry.borrow_mut().register(lib);

        // Return from registry
        self.library_registry
            .borrow()
            .get(name)
            .cloned()
            .ok_or_else(|| LibraryError::not_found(name))
    }

    /// Evaluate a parsed library (from a .sld file) using the VM.
    ///
    /// Instead of creating a temporary VmState, we swap the main state's
    /// globals to `lib_env`, execute body expressions directly, then swap
    /// back. This ensures continuations, code objects, and closures all
    /// live in the single real execution context.
    fn evaluate_parsed_library(
        &self,
        parsed: patina_runtime::library_loader::ParsedLibrary,
    ) -> Result<Library, LibraryError> {
        // Collection is already deferred for this whole function: `parsed`
        // carries a `GcDeferGuard` for as long as it holds unevaluated body
        // forms (see `ParsedLibrary`). That covers `saved_globals` and
        // `lib_env` too, both of which are reachable only from this frame.

        // Create a fresh environment for this library, sharing the global heap
        // so TaggedValue indices are compatible with the global environment.
        let lib_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));

        // Step 1: Resolve imports into lib_env
        for import_set in &parsed.imports {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds `parsed` and `lib_env` across each import's load: `parsed` holds \
                          `GcDeferGuard::holding` for as long as it lives (`ParsedLibrary`), so no \
                          collection runs"
            )]
            self.process_import_set(import_set, &lib_env)?;
        }

        // Step 2: Swap globals to lib_env, compile + execute each body
        // expression in the main VmState, then swap back.
        // Closures created during execution capture lib_env as their globals
        // (per-closure environment pointer), so no seeding or merge is needed.

        // A relative `include` in the body resolves beside the `.sld`.
        let desugarer = Desugarer::with_env(lib_env.clone())
            .with_fs(self.state.borrow().fs().clone())
            .with_include_base_of(parsed.source.as_deref());
        let shared_heap = lib_env.heap().clone();

        {
            let mut state = self.state.borrow_mut();
            state.with_globals(lib_env.clone(), |state| -> Result<(), LibraryError> {
                for tv in &parsed.body {
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "the import callback loads libraries during the expansion: \
                                  guarded by `desugar_with_imports`', `parsed`'s and \
                                  `with_globals`' `GcDeferGuard::holding`"
                    )]
                    let core_expr = desugarer.desugar_with_imports(
                        *tv,
                        &shared_heap,
                        |set, env| crate::runtime::vm_state::vm_process_import_set(state, set, env),
                        |e| {
                            patina_runtime::LibraryError::processing(
                                parsed.source.as_deref(),
                                format!("desugar error: {e}"),
                                e.diagnostic(),
                            )
                            .at_opt(e.source_location().cloned())
                        },
                    )?;

                    let (top, nested) = compile_with_qq_resolving(
                        &core_expr,
                        &shared_heap,
                        &lib_env,
                        state.primitive_registry(),
                    )
                    .map_err(|e| {
                        patina_runtime::LibraryError::processing(
                            parsed.source.as_deref(),
                            format!("compile error: {}", e),
                            e.diagnostic(),
                        )
                    })?;

                    let top_id = state.load_unit(top, nested);
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "holds `parsed` (the body's later forms), `lib_env` and, through \
                                  `with_globals`, the saved globals across the form's run: \
                                  `parsed` and `with_globals` each hold `GcDeferGuard::holding`, \
                                  so no collection runs"
                    )]
                    let result = execute(state, top_id);
                    state.release_unit_if_unused(top_id);

                    result.map_err(|e| {
                        patina_runtime::LibraryError::processing(
                            parsed.source.as_deref(),
                            format!("runtime error: {}", e),
                            e.diagnostic(),
                        )
                        .at_opt(e.source_location().cloned())
                    })?;
                }
                Ok(())
            })?;
        }

        // Step 3: Assemble the library and resolve its exports
        build_library(parsed, lib_env)
    }

    /// Resolve an import set into the given environment.
    fn process_import_set(
        &self,
        import_set: &ImportSet,
        lib_env: &Rc<Environment>,
    ) -> Result<(), LibraryError> {
        // Loading may re-enter Scheme. Borrow state only after it completes.
        #[expect(
            clippy::disallowed_methods,
            reason = "holds nothing of its own: `lib_env` is the caller's, and its call site gives \
                      the reason; the state is borrowed only after the load"
        )]
        let library = self.load_library(import_set.library_name())?;
        let mut state = self.state.borrow_mut();
        for binding in import_set.resolve_bindings(library.export_names()) {
            let (name, export) = binding?;
            import_export(&mut state, lib_env, name, &library, &export);
        }
        Ok(())
    }
}

impl Default for VmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for VmBackend {
    type Error = VmBackendError;

    fn eval(&self, expr: TaggedValue, _env: &Rc<Environment>) -> Result<TaggedValue, Self::Error> {
        self.eval_datum(expr, None)
    }

    fn global_env(&self) -> &Rc<Environment> {
        &self.global_env
    }

    /// The source map's positions are attached to CoreExpr nodes during
    /// desugaring, then threaded through the compiler pipeline into
    /// CodeObject source maps.
    fn eval_with_source_map(
        &self,
        expr: TaggedValue,
        _env: &Rc<Environment>,
        source_map: &Rc<RefCell<patina_frontend::SourceMap>>,
    ) -> Result<TaggedValue, Self::Error> {
        self.eval_datum(expr, Some(source_map))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

impl patina_runtime::HasDiagnostic for VmBackendError {
    fn diagnostic(&self) -> patina_runtime::Diagnostic {
        match self {
            Self::WithDiagnostic { diagnostic, .. } => (**diagnostic).clone(),
            Self::Desugar { message, .. } => {
                patina_runtime::Diagnostic::new(patina_runtime::DiagnosticKind::Syntax, message)
            }
            _ => patina_runtime::Diagnostic::new(
                patina_runtime::DiagnosticKind::Runtime,
                self.to_string(),
            ),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests evaluate through the interpreter from outside any loop, as an embedder \
              does"
)]
mod tests {
    use super::*;
    use patina_interpreter::Interpreter;

    fn eval(code: &str) -> TaggedValue {
        let backend = VmBackend::new();
        let interp = Interpreter::new(backend);
        // Use eval_program to handle multiple top-level expressions.
        interp.eval_program(code).expect("eval failed")
    }

    /// The interpreter's source-named evaluation is generic over backends, so
    /// a program run on the VM gets its errors placed and quoted from the
    /// source map, and `-k`'s outcome counts them, as on the tree-walker.
    #[test]
    fn source_named_evaluation_places_and_counts_vm_errors() {
        let interp = Interpreter::new(VmBackend::new());
        let (result, source_map) =
            interp.eval_program_with_source_name("(define x 1)\n\n(no-such x)\n", "t.scm");
        let error = result.expect_err("an unbound variable");
        let rendered =
            patina_interpreter::format_backend_error_with_source(&error, &source_map.borrow());
        assert!(rendered.contains("t.scm:3:2"), "{rendered}");
        assert!(rendered.contains("3 | (no-such x)"), "{rendered}");

        let (_, outcome) =
            interp.eval_program_resilient_with_source_name("(no-such)\n(also-no-such)\n", "k.scm");
        assert_eq!(outcome.eval_errors, 2);
        assert!(outcome.read_to_end);
    }

    #[test]
    fn let_simple() {
        assert_eq!(eval("(let ((x 5)) (+ x 1))").as_fixnum(), Some(6));
    }

    #[test]
    fn addition() {
        assert_eq!(eval("(+ 1 2)").as_fixnum(), Some(3));
    }

    #[test]
    fn conditional() {
        assert_eq!(eval("(if #t 42 0)").as_fixnum(), Some(42));
    }

    #[test]
    fn define_and_ref() {
        assert_eq!(eval("(begin (define x 10) x)").as_fixnum(), Some(10));
    }

    #[test]
    fn lambda_call() {
        assert_eq!(eval("((lambda (x) (+ x 1)) 41)").as_fixnum(), Some(42));
    }

    #[test]
    fn closure_capture() {
        assert_eq!(
            eval("((lambda (x) ((lambda () x))) 99)").as_fixnum(),
            Some(99)
        );
    }

    #[test]
    fn tail_recursive_fibonacci() {
        let code = "(define (fib-iter n a b)
                      (if (= n 0)
                          a
                          (fib-iter (- n 1) b (+ a b))))
                    (fib-iter 10 0 1)";
        assert_eq!(eval(code).as_fixnum(), Some(55));
    }
}
