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
//!
//! # Example
//!
//! ```ignore
//! use patina_runtime::{Backend, Value, Environment};
//! use std::rc::Rc;
//!
//! fn evaluate_with_any_backend<B: Backend>(backend: &B, code: &str) -> Result<Value, B::Error> {
//!     // Parse code to get expr (using frontend)
//!     let expr = parse(code)?;
//!
//!     // Evaluate using whatever backend is provided
//!     backend.eval(&expr, backend.global_env())
//! }
//! ```

use crate::{Environment, Value};
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
    type Error: std::error::Error + Send + Sync + 'static;

    /// Evaluate an expression in the given environment
    ///
    /// This is the core evaluation method. The backend is responsible for:
    /// - Interpreting the expression according to R7RS semantics
    /// - Managing tail call optimization
    /// - Handling special forms and procedure application
    /// - Propagating errors appropriately
    ///
    /// # Arguments
    ///
    /// - `expr`: The expression to evaluate (as a Value AST)
    /// - `env`: The environment in which to evaluate the expression
    ///
    /// # Returns
    ///
    /// The resulting value, or a backend-specific error.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = backend.eval(&expr, &env)?;
    /// ```
    fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error>;

    /// Get a reference to the global environment
    ///
    /// This is needed for:
    /// - REPL (evaluating top-level expressions)
    /// - Library loading (defining exports in global scope)
    /// - Introspection (listing defined variables)
    ///
    /// # Returns
    ///
    /// A reference to the backend's global environment.
    fn global_env(&self) -> &Rc<Environment>;

    /// Evaluate an expression in the global environment (convenience method)
    ///
    /// This is equivalent to `self.eval(expr, self.global_env())` but provided
    /// as a convenience for the common case of top-level evaluation.
    ///
    /// # Default Implementation
    ///
    /// The default implementation calls `eval` with `global_env()`.
    /// Backends can override this if they have a more efficient implementation.
    fn eval_global(&self, expr: &Value) -> Result<Value, Self::Error> {
        let global = self.global_env().clone();
        self.eval(expr, &global)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::RuntimeError;

    // Mock backend for testing
    struct MockBackend {
        global: Rc<Environment>,
    }

    impl Backend for MockBackend {
        type Error = RuntimeError;

        fn eval(&self, expr: &Value, _env: &Rc<Environment>) -> Result<Value, Self::Error> {
            // Just return the expression (identity backend)
            Ok(expr.clone())
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

        let expr = Value::Integer(42);
        let result = backend.eval_global(&expr).unwrap();

        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_backend_eval_with_env() {
        let backend = MockBackend {
            global: Rc::new(Environment::new()),
        };

        let custom_env = Rc::new(Environment::new());
        let expr = Value::Boolean(true);
        let result = backend.eval(&expr, &custom_env).unwrap();

        assert!(matches!(result, Value::Boolean(true)));
    }
}
