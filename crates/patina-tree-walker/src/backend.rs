//! Backend trait implementation for the tree-walking interpreter
//!
//! This module provides the `TreeWalker` struct which wraps the `Evaluator`
//! and implements the `Backend` trait from `patina-runtime`. This allows the
//! tree-walker to be used as a pluggable backend in the interpreter.
//!
//! # CPS-Only Evaluation
//!
//! The tree-walker uses CPS (Continuation-Passing Style) transformation as
//! the evaluation mode. This ensures all lambdas (including library code)
//! are CPS lambdas, enabling proper continuation support throughout the
//! codebase including `call/cc`, `dynamic-wind`, and exception handling.

use crate::eval::{EvalError, Evaluator, eval_cps};
use patina_core::TaggedValue;
use patina_frontend::SourceMap;
use patina_runtime::HasDiagnostic;
use patina_runtime::{Backend, Environment};
use std::cell::RefCell;
use std::rc::Rc;

/// Tree-walking interpreter backend
///
/// This is a lightweight wrapper around `Evaluator` that implements the
/// `Backend` trait. It provides the same functionality as the raw `Evaluator`,
/// but with a standardized interface that allows it to be swapped with other
/// backends (VM, JIT, etc.).
///
/// # CPS Evaluation
///
/// All evaluation is done via CPS transformation. This ensures:
/// - Full continuation support (`call/cc`, `dynamic-wind`)
/// - Consistent lambda representation (all lambdas are CPS lambdas)
/// - No need for thread-local escape mechanisms
///
/// # Example
///
/// ```ignore
/// use patina_tree_walker::TreeWalker;
/// use patina_runtime::Backend;
///
/// let backend = TreeWalker::new();
/// let expr = parse("(+ 1 2 3)");
/// let result = backend.eval_global(&expr).unwrap();
/// ```
pub struct TreeWalker {
    evaluator: Rc<Evaluator>,
}

impl TreeWalker {
    /// Create a new tree-walking backend with a fresh environment
    ///
    /// This initializes:
    /// - Global environment with all R7RS primitives
    /// - Library loading infrastructure
    /// - Bootstrap macros (let, cond, case, etc.)
    ///
    /// All evaluation uses CPS transformation for full continuation support.
    pub fn new() -> Self {
        TreeWalker {
            evaluator: Rc::new(Evaluator::new()),
        }
    }

    /// Create a tree-walker with a custom filesystem.
    pub fn with_fs(fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        TreeWalker {
            evaluator: Rc::new(Evaluator::with_fs(fs)),
        }
    }

    /// A tree-walker collecting in `mode`, whatever the environment says: see
    /// [`Evaluator::with_gc_mode`]. Not an interface.
    #[doc(hidden)]
    pub fn with_gc_mode(mode: patina_core::GcMode) -> Self {
        TreeWalker {
            evaluator: Rc::new(Evaluator::with_gc_mode(mode)),
        }
    }

    /// Create a tree-walker from an existing evaluator
    ///
    /// This is useful for tests that need to configure the evaluator
    /// before creating the backend (e.g., adding search paths, installing
    /// custom primitives).
    pub fn from_evaluator(evaluator: Evaluator) -> Self {
        TreeWalker {
            evaluator: Rc::new(evaluator),
        }
    }

    /// Get a reference to the underlying evaluator
    ///
    /// This provides access to evaluator-specific functionality that's
    /// not part of the generic Backend trait, such as:
    /// - Debug configuration
    /// - Library registry access
    /// - Direct environment manipulation
    ///
    /// Use with caution - code that uses this method won't be portable
    /// to other backends.
    pub fn evaluator(&self) -> &Evaluator {
        &self.evaluator
    }

    /// Shared body of `eval` and `eval_with_source_map` — the two entries
    /// differ only in desugarer construction.
    fn eval_datum(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
        source_map: Option<&Rc<RefCell<SourceMap>>>,
    ) -> Result<TaggedValue, EvalError> {
        use patina_frontend::Desugarer;

        // A form starts with no interrupted `exit` noted; see
        // `patina_runtime::exit_status`.
        patina_runtime::exit_status::forget_interrupted_exit();
        let internal_heap = self.evaluator.global_env.heap();

        // An inline (define-library ...) is a library definition, not an
        // expression — route it to the library loader before desugaring.
        if patina_frontend::is_define_library_form(expr, env) {
            #[expect(
                clippy::disallowed_methods,
                reason = "the backend's top level, so the load may collect (#677). Holds the \
                          `define-library` datum, not read after the call: the library's body \
                          forms are in its `ParsedLibrary`, under its `GcDeferGuard::holding`, \
                          until its registry entry roots them"
            )]
            self.evaluator
                .eval_inline_define_library(expr)
                .map_err(|e| {
                    EvalError::InvalidSyntax(format!("define-library failed: {}", e))
                        .with_diagnostic(e.diagnostic())
                })?;
            return Ok(TaggedValue::UNSPECIFIED);
        }

        let desugarer = match source_map {
            Some(sm) => Desugarer::with_env_and_source_map(env.clone(), sm.clone())
                .with_fs(self.evaluator.fs.clone()),
            None => Desugarer::with_env(env.clone()).with_fs(self.evaluator.fs.clone()),
        };

        #[expect(
            clippy::disallowed_methods,
            reason = "the import callback loads libraries during the expansion, which holds the \
                      datum and the partial expansion: guarded by `desugar_with_imports`' \
                      `GcDeferGuard::holding`"
        )]
        let core_expr = desugarer.desugar_with_imports(
            expr,
            internal_heap,
            |set, env| self.evaluator.process_import_for_eval(set, env),
            |e| {
                let location = e.source_location().cloned();
                EvalError::DesugarError(e.to_string())
                    .with_diagnostic(e.diagnostic())
                    .at_opt(location)
            },
        )?;

        #[expect(
            clippy::disallowed_methods,
            reason = "the backend's entry for a form, from outside any loop, so the run may \
                      collect: holds `core_expr` and the datum, neither read after the call; the \
                      run roots the CPS tree it is entered with, literals included"
        )]
        let value = eval_cps(&core_expr, env.clone(), &self.evaluator);
        value
    }

    /// Add a library search path — counterpart of
    /// `VmBackend::add_library_search_path`, so callers can treat both
    /// backends uniformly.
    pub fn add_library_search_path(&self, path: std::path::PathBuf) {
        self.evaluator.add_library_search_path(path);
    }

    /// Add a library search path ahead of every existing one (the CLI's `-I`).
    pub fn prepend_library_search_path(&self, path: std::path::PathBuf) {
        self.evaluator.prepend_library_search_path(path);
    }

    /// See [`Evaluator::bootstrap_error`](crate::eval::Evaluator::bootstrap_error).
    pub fn bootstrap_error(&self) -> Option<&patina_runtime::LibraryError> {
        self.evaluator.bootstrap_error()
    }
}

impl Default for TreeWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for TreeWalker {
    type Error = EvalError;

    fn eval(&self, expr: TaggedValue, env: &Rc<Environment>) -> Result<TaggedValue, Self::Error> {
        self.eval_datum(expr, env, None)
    }

    fn global_env(&self) -> &Rc<Environment> {
        &self.evaluator.global_env
    }

    /// The source map's positions are attached to CoreExpr nodes during
    /// desugaring.
    fn eval_with_source_map(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<TaggedValue, Self::Error> {
        self.eval_datum(expr, env, Some(source_map))
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests evaluate through `Backend` from outside any loop, as an embedder does"
)]
mod tests {
    use super::*;

    /// Helper: parse a string and evaluate it via the Backend trait
    fn eval_str(backend: &TreeWalker, code: &str) -> Result<TaggedValue, EvalError> {
        let heap = backend.global_env().heap();
        let mut parser = patina_frontend::Parser::new_with_heap(code, heap.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        backend.eval_global(expr)
    }

    #[test]
    fn test_tree_walker_creation() {
        let backend = TreeWalker::new();
        assert!(backend.global_env().get("car").is_some());
    }

    #[test]
    fn test_tree_walker_eval_self_evaluating() {
        let backend = TreeWalker::new();
        let result = eval_str(&backend, "42").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_tree_walker_eval_primitive() {
        let backend = TreeWalker::new();
        let result = eval_str(&backend, "(+ 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(6));
    }

    #[test]
    fn test_tree_walker_default() {
        let backend = TreeWalker::default();
        assert!(backend.global_env().get("+").is_some());
    }

    #[test]
    fn test_tree_walker_eval_in_custom_env() {
        let backend = TreeWalker::new();
        let custom_env = Rc::new(Environment::with_parent(backend.global_env().clone()));

        // Define a variable in custom env
        custom_env.define("x".to_string(), TaggedValue::fixnum(99));

        // Parse and evaluate x in custom env
        let heap = backend.global_env().heap();
        let mut parser = patina_frontend::Parser::new_with_heap("x", heap.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let result = backend.eval(expr, &custom_env).unwrap();

        assert_eq!(result.as_fixnum(), Some(99));
    }

    #[test]
    fn test_tree_walker_cps_arithmetic() {
        let backend = TreeWalker::new();
        let result = eval_str(&backend, "(+ 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(6));
    }
}
