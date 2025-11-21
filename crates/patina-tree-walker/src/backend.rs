//! Backend trait implementation for the tree-walking interpreter
//!
//! This module provides the `TreeWalker` struct which wraps the `Evaluator`
//! and implements the `Backend` trait from `patina-runtime`. This allows the
//! tree-walker to be used as a pluggable backend in the interpreter.

use crate::eval::{EvalError, Evaluator};
use patina_runtime::{Backend, Environment, Value};
use std::rc::Rc;

/// Tree-walking interpreter backend
///
/// This is a lightweight wrapper around `Evaluator` that implements the
/// `Backend` trait. It provides the same functionality as the raw `Evaluator`,
/// but with a standardized interface that allows it to be swapped with other
/// backends (VM, JIT, etc.).
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
    evaluator: Evaluator,
}

impl TreeWalker {
    /// Create a new tree-walking backend with a fresh environment
    ///
    /// This initializes:
    /// - Global environment with all R7RS primitives
    /// - Library loading infrastructure
    /// - Bootstrap macros (let, cond, case, etc.)
    pub fn new() -> Self {
        TreeWalker {
            evaluator: Evaluator::new(),
        }
    }

    /// Create a tree-walker from an existing evaluator
    ///
    /// This is useful for tests that need to configure the evaluator
    /// before creating the backend (e.g., adding search paths, installing
    /// custom primitives).
    pub fn from_evaluator(evaluator: Evaluator) -> Self {
        TreeWalker { evaluator }
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
}

impl Default for TreeWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for TreeWalker {
    type Error = EvalError;

    fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error> {
        self.evaluator.eval_in_env(expr, env)
    }

    fn global_env(&self) -> &Rc<Environment> {
        &self.evaluator.global_env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_walker_creation() {
        let backend = TreeWalker::new();
        assert!(backend.global_env().get("car").is_some());
    }

    #[test]
    fn test_tree_walker_eval_self_evaluating() {
        let backend = TreeWalker::new();
        let expr = Value::Integer(42);
        let result = backend.eval_global(&expr).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_tree_walker_eval_primitive() {
        let backend = TreeWalker::new();

        // Build (+ 1 2 3) as a Value
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;

        let plus = Value::Symbol(StdRc::from("+"));
        let one = Value::Integer(1);
        let two = Value::Integer(2);
        let three = Value::Integer(3);

        // (+ 1 2 3) = (+ . (1 . (2 . (3 . ()))))
        let expr = Value::Pair(StdRc::new(RefCell::new((
            plus,
            Value::Pair(StdRc::new(RefCell::new((
                one,
                Value::Pair(StdRc::new(RefCell::new((
                    two,
                    Value::Pair(StdRc::new(RefCell::new((three, Value::Null)))),
                )))),
            )))),
        ))));

        let result = backend.eval_global(&expr).unwrap();
        assert!(matches!(result, Value::Integer(6)));
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
        custom_env.define("x".to_string(), Value::Integer(99));

        // Evaluate x in custom env
        let expr = Value::Symbol(Rc::from("x"));
        let result = backend.eval(&expr, &custom_env).unwrap();

        assert!(matches!(result, Value::Integer(99)));
    }
}
