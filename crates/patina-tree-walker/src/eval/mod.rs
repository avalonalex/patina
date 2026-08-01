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
    pub(crate) library_registry: RefCell<LibraryRegistry>,
    /// Registry of library loaders (Rust, Scheme, etc.)
    pub(crate) loader_registry: RefCell<LibraryLoaderRegistry>,
    /// Registry of primitive procedures (shared across all backends via patina-primitives)
    pub(crate) primitive_registry: patina_primitives::PrimitiveRegistry,
    /// Virtual filesystem for all file I/O operations
    pub(crate) fs: Arc<dyn patina_core::FileSystem>,
    /// Garbage collector policy and state (see `docs/GC_DESIGN.md`).
    /// Serviced at trampoline safe points; off unless requested or enabled.
    pub(crate) gc: RefCell<patina_core::GcController>,
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
        let global_env = Rc::new(Environment::new());

        // Create primitive registry and register all primitives
        let mut primitive_registry = patina_primitives::PrimitiveRegistry::new();
        Self::register_all_primitives(&mut primitive_registry);

        // Create library registries
        let mut lib_registry = LibraryRegistry::with_default_paths();
        lib_registry.set_fs(fs.clone());
        let library_registry = RefCell::new(lib_registry);
        let loader_registry = RefCell::new(LibraryLoaderRegistry::new());

        let evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
            library_registry,
            loader_registry,
            primitive_registry,
            fs,
            gc: RefCell::new(patina_core::GcController::from_env()),
        };

        // Initialize library loaders
        evaluator.init_loaders();

        // Load bootstrap library
        evaluator.load_bootstrap();

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

        // === Test framework ===
        rust_loader.register(
            vec![
                "chibi".to_string(),
                "test".to_string(),
                "primitives".to_string(),
            ],
            stdlib::build_chibi_test_primitives,
        );

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

    fn load_bootstrap(&self) {
        // Load (scheme base) library
        // This will load Rust primitives and automatically load base-extras.scm
        let _ = self.load_library(&["scheme".to_string(), "base".to_string()]);

        // Load Patina debugging utilities
        // Auto-loaded in REPL for convenience (commonly used during development)
        let _ = self.load_library(&["patina".to_string(), "debug".to_string()]);

        // After loading libraries, import (scheme base) into global environment
        // This makes primitives and macros available without explicit import
        // (R7RS-style convenience for REPL)
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["scheme".to_string(), "base".to_string()])
        {
            for (name, tv) in lib.exports_iter_tagged() {
                self.global_env.define(name.clone(), tv);
            }
        }

        // Import (patina debug) into global environment for REPL convenience
        if let Some(lib) = self
            .library_registry
            .borrow()
            .get(&["patina".to_string(), "debug".to_string()])
        {
            for (name, tv) in lib.exports_iter_tagged() {
                self.global_env.define(name.clone(), tv);
            }
        }
    }

    /// Load Scheme-implemented extras for a library
    ///
    /// After loading Rust primitives, this checks for a corresponding `-extras.scm` file.
    /// If found, it's evaluated in the library's environment.
    ///
    /// Convention:
    /// - (scheme base) → lib/scheme/base-extras.scm
    /// - (scheme lazy) → lib/scheme/lazy-extras.scm
    ///
    /// Note: Some libraries are fully defined as .sld files:
    /// - (scheme case-lambda) → lib/scheme/case-lambda.sld
    /// - (chibi test) → lib/chibi/test.sld
    ///
    /// This allows any library to have:
    /// - Rust primitives (performance-critical operations)
    /// - Scheme code (derived forms, macros, convenience functions)
    fn load_library_extras(&self, name: &[String]) {
        if name.is_empty() {
            return;
        }

        // Construct relative extras file path: (scheme base) → scheme/base-extras.scm
        let mut relative_path = std::path::PathBuf::new();
        for part in &name[..name.len() - 1] {
            relative_path.push(part);
        }
        relative_path.push(format!("{}-extras.scm", name.last().unwrap()));

        // Search for the extras file in all library search paths
        let search_paths = self.library_search_paths();
        let extras_path = search_paths
            .iter()
            .map(|base| base.join(&relative_path))
            .find(|path| self.fs.file_exists(path));

        let extras_path = match extras_path {
            Some(path) => path,
            None => return, // No extras file - that's fine
        };

        // Read the extras file
        let extras_content = match self.fs.read_to_string(&extras_path) {
            Ok(content) => content,
            Err(_) => return, // Can't read file - silently skip
        };

        // Get the library's environment
        let lib_env = {
            let registry = self.library_registry.borrow();
            match registry.get(name) {
                Some(lib) => lib.env.clone(),
                None => {
                    tracing::warn!(
                        library = ?name,
                        path = %extras_path.display(),
                        "Library not loaded, cannot load extras"
                    );
                    return;
                }
            }
        };

        // Create an evaluation environment that has access to (scheme base) primitives
        // The extras file needs these for macro definitions (display, write, newline, etc.)
        let eval_env = {
            // Start with library environment as base, sharing global heap for TaggedValue compatibility
            let env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));

            // Import all (scheme base) exports if this isn't (scheme base) itself
            let is_scheme_base = name == ["scheme".to_string(), "base".to_string()];
            if !is_scheme_base {
                let registry = self.library_registry.borrow();
                if let Some(scheme_base) = registry.get(&["scheme".to_string(), "base".to_string()])
                {
                    // Add all (scheme base) exports to the evaluation environment
                    for (export_name, tv) in scheme_base.exports_iter_tagged() {
                        env.define(export_name.clone(), tv);
                    }
                }
            }

            // Add all library's own definitions (these can shadow scheme base)
            for (binding_name, value) in lib_env.bindings() {
                env.define(binding_name, value);
            }

            env
        };

        // Parse and evaluate all expressions in the evaluation environment
        // Use shared heap for parser allocations
        let heap = self.global_env.heap();
        let mut parser = match patina_frontend::Parser::new_with_heap(&extras_content, heap.clone())
        {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    path = %extras_path.display(),
                    error = ?e,
                    "Failed to parse extras file"
                );
                return;
            }
        };

        // Create desugarer with environment for macro expansion
        let desugarer =
            patina_frontend::Desugarer::with_env(eval_env.clone()).with_fs(self.fs.clone());

        loop {
            match parser.parse() {
                Ok(tagged) => {
                    // Desugar TaggedValue to CoreExpr - desugar_tagged manages heap borrows internally
                    let core_expr = match desugarer.desugar_tagged(tagged, heap) {
                        Ok(ce) => ce,
                        Err(e) => {
                            tracing::warn!(
                                path = %extras_path.display(),
                                error = %e,
                                "Failed to desugar expression in extras file"
                            );
                            continue;
                        }
                    };

                    // Evaluate using CPS evaluator so all lambdas become CpsLambdas
                    if let Err(e) = eval_cps(&core_expr, eval_env.clone(), self) {
                        tracing::warn!(
                            path = %extras_path.display(),
                            error = %e,
                            "Failed to evaluate expression in extras file"
                        );
                    }
                }
                Err(patina_frontend::ParseError::UnexpectedEof) => break,
                Err(e) => {
                    tracing::warn!(
                        path = %extras_path.display(),
                        error = ?e,
                        "Parse error in extras file"
                    );
                    break;
                }
            }
        }

        // Copy any new definitions from the evaluation environment back to the library environment
        for (binding_name, value) in eval_env.bindings() {
            // Skip (scheme base) primitives - only copy definitions created by the extras file
            let is_from_extras = {
                let registry = self.library_registry.borrow();
                if let Some(scheme_base) = registry.get(&["scheme".to_string(), "base".to_string()])
                {
                    !scheme_base.exports.contains_key(&binding_name)
                } else {
                    true // If (scheme base) not loaded, copy everything
                }
            };

            if is_from_extras {
                lib_env.define(binding_name, value);
            }
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
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
    pub fn load_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_runtime::Library>, patina_runtime::LibraryError> {
        // Check if already loaded
        {
            let registry = self.library_registry.borrow();
            if let Some(lib) = registry.get(name) {
                return Ok(Rc::new(lib.clone()));
            }
        }

        // Check for circular dependencies
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.begin_loading(name)?;
        }

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
            // Clone search_paths for use in the closure
            let search_paths_for_checker = search_paths.clone();
            let can_load_library = |lib_name: &[String]| {
                let loaders = self.loader_registry.borrow();
                loaders.can_load_with_paths(lib_name, &search_paths_for_checker)
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

            if let Some(parsed) = parsed {
                // Parse succeeded, now evaluate
                (self.evaluate_parsed_library(parsed)?, false)
            } else {
                // No loader can handle this library
                self.library_registry.borrow_mut().end_loading(name);
                return Err(patina_runtime::LibraryError::NotFound(name.to_vec()));
            }
        };

        // End loading tracking
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.end_loading(name);
        }

        // Register the library first
        {
            let mut registry = self.library_registry.borrow_mut();
            registry.register(lib)?;
        }

        // If loaded via Rust, load extras and update exports in-place
        if loaded_via_rust {
            // Load extras file - this adds definitions to the library's environment
            self.load_library_extras(name);

            // Update the library's exports to include everything from the environment
            {
                let mut registry = self.library_registry.borrow_mut();
                if let Some(library) = registry.get_mut(name) {
                    let all_bindings = library.env.bindings();

                    // Clear and re-export everything
                    library.exports.clear();
                    for (binding_name, value) in all_bindings {
                        library.export_tagged(binding_name, value);
                    }
                }
            }
        }

        // Return the registered library
        let lib_rc = {
            let registry = self.library_registry.borrow();
            Rc::new(registry.get(name).cloned().expect("Library should exist"))
        };

        Ok(lib_rc)
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

    /// Evaluate a parsed library
    ///
    /// This method is called after a library is parsed from a .sld file.
    /// It handles import resolution, body evaluation, and export collection.
    fn evaluate_parsed_library(
        &self,
        parsed: patina_runtime::library_loader::ParsedLibrary,
    ) -> Result<patina_runtime::Library, patina_runtime::LibraryError> {
        use patina_runtime::library_loader::ExportSpec;

        // Create a fresh environment for this library, sharing global heap for TaggedValue compatibility
        let lib_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));

        // Step 1: Resolve imports
        for import_set in &parsed.imports {
            self.process_import_set(import_set, &lib_env)?;
        }

        // Step 2: Evaluate library body (definitions only)
        // Use CPS evaluation so that all lambdas become CpsLambdas, enabling
        // proper continuation support throughout the codebase.
        let desugarer =
            patina_frontend::Desugarer::with_env(lib_env.clone()).with_fs(self.fs.clone());
        let shared_heap = lib_env.heap().clone();
        // `parsed.body` holds the not-yet-evaluated forms as TaggedValues in
        // a Rust local — invisible to every root provider. Defer collection
        // until the whole body has been evaluated (`docs/GC_DESIGN.md` §7).
        let _gc_defer = patina_core::GcDeferGuard::new(&shared_heap);
        for tv in &parsed.body {
            // Desugar TaggedValue to CoreExpr
            let core_expr = desugarer.desugar_tagged(*tv, &shared_heap).map_err(|e| {
                patina_runtime::LibraryError::ParseError {
                    file: parsed
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    message: format!("Failed to desugar expression: {}", e),
                }
            })?;

            // Evaluate using CPS evaluator
            eval_cps(&core_expr, lib_env.clone(), self).map_err(|e| {
                patina_runtime::LibraryError::ParseError {
                    file: parsed
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    message: format!("Error evaluating library body: {:?}", e),
                }
            })?;
        }

        // Step 3: Collect exports and create library
        let mut library = patina_runtime::Library::with_env(parsed.name.clone(), lib_env.clone());
        if let Some(source) = parsed.source {
            library.set_source(source);
        }

        for spec in &parsed.exports {
            match spec {
                ExportSpec::Identifier(name) => {
                    // Export with same name
                    if let Some(value) = lib_env.get(name) {
                        library.export_tagged(name.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", name),
                        });
                    }
                }

                ExportSpec::Rename { internal, external } => {
                    // Export with different name
                    if let Some(value) = lib_env.get(internal) {
                        library.export_tagged(external.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", internal),
                        });
                    }
                }
            }
        }

        Ok(library)
    }

    /// Process a single import set
    fn process_import_set(
        &self,
        import_set: &patina_runtime::library_loader::ImportSet,
        lib_env: &Rc<Environment>,
    ) -> Result<(), patina_runtime::LibraryError> {
        use patina_runtime::library_loader::ImportSet;
        use std::collections::HashSet;

        match import_set {
            ImportSet::Library(lib_name) => {
                // Direct library import: import all exports
                let imported_lib = self.load_library(lib_name)?;

                // Import all exports into this library's environment
                for (name, value) in imported_lib.exports_iter_tagged() {
                    lib_env.define(name.clone(), value);
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // Import only specific identifiers
                // First process the inner import set to get the bindings
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_set(import_set, &temp_env)?;

                // Then import only the specified identifiers
                for id in identifiers {
                    if let Some(value) = temp_env.get(id) {
                        lib_env.define(id.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found in import set", id),
                        });
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Import all except specific identifiers
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_set(import_set, &temp_env)?;

                let exclude: HashSet<_> = identifiers.iter().collect();

                // Import all bindings except the excluded ones
                for (name, value) in temp_env.bindings() {
                    if !exclude.contains(&name) {
                        lib_env.define(name, value);
                    }
                }

                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Import with prefix: foo → prefix:foo
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_set(import_set, &temp_env)?;

                // Import all bindings with the prefix added
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    lib_env.define(prefixed_name, value);
                }

                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Import with renames: old-name → new-name
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_set(import_set, &temp_env)?;

                // Apply renames
                for (old_name, new_name) in renames {
                    if let Some(value) = temp_env.get(old_name) {
                        lib_env.define(new_name.clone(), value);
                    } else {
                        return Err(patina_runtime::LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found for rename", old_name),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// Process an import set for eval context
    ///
    /// This imports library identifiers into a regular environment (not building a library).
    /// Used by the `import` special form.
    pub(crate) fn process_import_for_eval(
        &self,
        import_set: &patina_frontend::ImportSet,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        use patina_frontend::ImportSet;
        use std::collections::HashSet;

        match import_set {
            ImportSet::Library(lib_name) => {
                // Load the library
                let lib = self.load_library(lib_name).map_err(|e| {
                    EvalError::InvalidSyntax(format!("Failed to load library: {}", e))
                })?;

                // Import all exports into the current environment
                for export_name in lib.export_names() {
                    if let Some(value) = lib.get_export_tagged(export_name) {
                        env.define(export_name.to_string(), value);
                    }
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // First process the nested import set into a temporary environment
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_for_eval(import_set, &temp_env)?;

                // Then import only the specified identifiers
                let allowed: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if allowed.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Process nested import set into temp environment
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all except specified identifiers
                let excluded: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if !excluded.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Process nested import set into temp environment
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all with prefix
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    env.define(prefixed_name, value);
                }
                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Process nested import set into temp environment
                // Use shared heap for TaggedValue compatibility
                let temp_env = Rc::new(Environment::with_heap(self.global_env.heap().clone()));
                self.process_import_for_eval(import_set, &temp_env)?;

                // Build rename map
                let rename_map: std::collections::HashMap<_, _> = renames.iter().cloned().collect();

                // Import with renaming
                for (name, value) in temp_env.bindings() {
                    let final_name = rename_map.get(&name).unwrap_or(&name);
                    env.define(final_name.clone(), value);
                }
                Ok(())
            }
        }
    }
}
