//! Backend trait for pluggable interpreter implementations
//!
//! This module defines the core `Backend` trait that allows Patina to support
//! multiple evaluation strategies (tree-walker, VM, JIT) without changing the
//! high-level interpreter API.
//!
//! # Design Philosophy
//!
//! The Backend trait abstracts over evaluation strategy while maintaining access
//! to the global environment (needed for REPL, library loading, etc.). Each backend
//! is responsible for:
//!
//! - Evaluating expressions in a given environment
//! - Managing its own internal state (stack, registers, etc.)
//! - Providing access to the global environment for introspection

use crate::Environment;
use patina_core::{SourceMap, TaggedValue};
use std::cell::RefCell;
use std::rc::Rc;

/// Core trait for interpreter backend implementations
///
/// This trait abstracts over different evaluation strategies:
/// - Tree-walker: Direct AST interpretation with trampoline TCO
/// - VM: Bytecode compilation and stack-based execution
/// - JIT: Native code generation with LLVM/Cranelift
///
/// Each backend must provide:
/// 1. Expression evaluation in a given environment
/// 2. Access to the global environment (for REPL, introspection)
/// 3. A backend-specific error type
pub trait Backend {
    /// Backend-specific error type
    ///
    /// Should implement `std::error::Error` and ideally wrap or convert
    /// `RuntimeError` for common semantic errors.
    type Error: std::error::Error + crate::HasDiagnostic + Send + Sync + 'static;

    /// Evaluate a TaggedValue expression in the given environment
    ///
    /// This is the core evaluation method. The backend is responsible for:
    /// - Interpreting the expression according to R7RS semantics
    /// - Managing tail call optimization
    /// - Handling special forms and procedure application
    /// - Propagating errors appropriately
    ///
    /// # Arguments
    ///
    /// - `expr`: The expression to evaluate (as a TaggedValue)
    /// - `env`: The environment in which to evaluate the expression
    ///
    /// # Returns
    ///
    /// The resulting TaggedValue, or a backend-specific error.
    fn eval(&self, expr: TaggedValue, env: &Rc<Environment>) -> Result<TaggedValue, Self::Error>;

    /// Get a reference to the global environment
    ///
    /// This is needed for:
    /// - REPL (evaluating top-level expressions)
    /// - Library loading (defining exports in global scope)
    /// - Introspection (listing defined variables)
    fn global_env(&self) -> &Rc<Environment>;

    /// Evaluate an expression in the global environment (convenience method)
    ///
    /// This is equivalent to `self.eval(expr, self.global_env())` but provided
    /// as a convenience for the common case of top-level evaluation.
    #[expect(
        clippy::disallowed_methods,
        reason = "holds `global`, a clone of the backend's global environment, which the backend \
                  roots: nothing the evaluation could free"
    )]
    fn eval_global(&self, expr: TaggedValue) -> Result<TaggedValue, Self::Error> {
        let global = self.global_env().clone();
        self.eval(expr, &global)
    }

    /// Evaluate an expression the parser read into `source_map`, so that what
    /// the evaluation reports carries the positions recorded there.
    ///
    /// Every caller that reads a program with positions — a file, standard
    /// input, a session — evaluates through this, which is what lets that
    /// reading be written once for all backends. A backend that does not
    /// track positions evaluates the expression as `eval` does.
    #[expect(
        clippy::disallowed_methods,
        reason = "holds nothing it reads as a value: the source map is keyed by raw bits it \
                  never dereferences and owns no Scheme value, so a collection can only leave it \
                  stale keys (docs/GC_DESIGN.md §9.1), which misattribute a diagnostic and free \
                  nothing"
    )]
    fn eval_with_source_map(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<TaggedValue, Self::Error> {
        let _ = source_map;
        self.eval(expr, env)
    }

    /// Bind `name` to `value` in `env`, as a program's `define` there does,
    /// so that code compiled there before the definition calls the new
    /// value as well.
    ///
    /// The default writes the environment's binding, which is all a backend
    /// needs that looks a name up at each call, as the tree-walker does. The
    /// VM compiles a call to a primitive's binding into a fast path, and
    /// marks the primitive before it rebinds the name, as its `Define`
    /// instruction does, so that the code compiled before stops taking the
    /// fast path (#673). A host defines through this, by way of
    /// `Interpreter::define` and `Interpreter::define_in`;
    /// `Environment::define` and `Environment::set` are the raw layer, which
    /// write the binding and nothing else.
    fn define(&self, env: &Rc<Environment>, name: &str, value: TaggedValue) {
        env.define(name, value);
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests of the trait's default methods against a mock backend that evaluates \
              nothing"
)]
mod tests {
    use super::*;
    use crate::error::RuntimeError;

    // Mock backend for testing
    struct MockBackend {
        global: Rc<Environment>,
    }

    impl Backend for MockBackend {
        type Error = RuntimeError;

        fn eval(
            &self,
            expr: TaggedValue,
            _env: &Rc<Environment>,
        ) -> Result<TaggedValue, Self::Error> {
            // Just return the expression (identity backend)
            Ok(expr)
        }

        fn global_env(&self) -> &Rc<Environment> {
            &self.global
        }
    }

    #[test]
    fn test_backend_trait() {
        let backend = MockBackend {
            global: Rc::new(Environment::new()),
        };

        let expr = TaggedValue::fixnum(42);
        let result = backend.eval_global(expr).unwrap();

        assert_eq!(result.as_fixnum(), Some(42));
    }

    /// A backend that does not track positions evaluates with a source map as
    /// it does without one.
    #[test]
    fn eval_with_source_map_defaults_to_eval() {
        let backend = MockBackend {
            global: Rc::new(Environment::new()),
        };
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let global = backend.global_env().clone();
        let result = backend
            .eval_with_source_map(TaggedValue::fixnum(7), &global, &source_map)
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(7));
    }

    /// A backend that compiles nothing defines by writing the environment.
    #[test]
    fn define_defaults_to_the_environment() {
        let backend = MockBackend {
            global: Rc::new(Environment::new()),
        };
        let global = backend.global_env().clone();
        backend.define(&global, "answer", TaggedValue::fixnum(42));
        assert_eq!(global.get("answer"), Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_backend_eval_with_env() {
        let backend = MockBackend {
            global: Rc::new(Environment::new()),
        };

        let custom_env = Rc::new(Environment::new());
        let expr = TaggedValue::TRUE;
        let result = backend.eval(expr, &custom_env).unwrap();

        assert_eq!(result, TaggedValue::TRUE);
    }
}
