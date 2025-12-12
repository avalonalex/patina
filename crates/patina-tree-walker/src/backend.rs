//! Backend trait implementation for the tree-walking interpreter
//!
//! This module provides the `TreeWalker` struct which wraps the `Evaluator`
//! and implements the `Backend` trait from `patina-runtime`. This allows the
//! tree-walker to be used as a pluggable backend in the interpreter.

use crate::eval::{eval_cps, EvalError, Evaluator};
use patina_runtime::{Backend, Environment, Value};
use std::rc::Rc;

// Note: All special forms now in CoreExpr - no fallback forms needed!

/// Evaluation mode for the tree-walker backend
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvalMode {
    /// Direct CoreExpr evaluation (default, faster)
    #[default]
    Direct,
    /// CPS transformation then evaluation (supports call/cc, shift/reset)
    Cps,
}

/// Tree-walking interpreter backend
///
/// This is a lightweight wrapper around `Evaluator` that implements the
/// `Backend` trait. It provides the same functionality as the raw `Evaluator`,
/// but with a standardized interface that allows it to be swapped with other
/// backends (VM, JIT, etc.).
///
/// # Evaluation Modes
///
/// The tree-walker supports two evaluation modes:
///
/// - **Direct** (default): Evaluates CoreExpr directly. Faster but doesn't support
///   first-class continuations.
/// - **CPS**: Transforms CoreExpr to CpsExpr before evaluation. Slower but supports
///   `call/cc`, `shift`, `reset`, and other continuation operations.
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
    mode: EvalMode,
}

impl TreeWalker {
    /// Create a new tree-walking backend with a fresh environment
    ///
    /// This initializes:
    /// - Global environment with all R7RS primitives
    /// - Library loading infrastructure
    /// - Bootstrap macros (let, cond, case, etc.)
    ///
    /// Uses Direct evaluation mode by default.
    pub fn new() -> Self {
        TreeWalker {
            evaluator: Rc::new(Evaluator::new()),
            mode: EvalMode::Direct,
        }
    }

    /// Create a new tree-walking backend with CPS evaluation mode
    ///
    /// This enables support for first-class continuations (call/cc) and
    /// delimited continuations (shift/reset). Use this when you need
    /// continuation support.
    pub fn new_with_cps() -> Self {
        TreeWalker {
            evaluator: Rc::new(Evaluator::new()),
            mode: EvalMode::Cps,
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
            mode: EvalMode::Direct,
        }
    }

    /// Create a tree-walker from an existing evaluator with specified mode
    pub fn from_evaluator_with_mode(evaluator: Evaluator, mode: EvalMode) -> Self {
        TreeWalker {
            evaluator: Rc::new(evaluator),
            mode,
        }
    }

    /// Get the current evaluation mode
    pub fn mode(&self) -> EvalMode {
        self.mode
    }

    /// Set the evaluation mode
    pub fn set_mode(&mut self, mode: EvalMode) {
        self.mode = mode;
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
        // CoreExpr pipeline with MACRO-AWARE DESUGARING:
        // 1. Create desugarer with environment (enables macro expansion)
        // 2. Desugar - the desugarer will expand macros as needed during desugaring
        // 3. Evaluate via CoreExpr (Direct mode) or CpsExpr (CPS mode)
        //
        // This approach is better than pre-expanding all macros because:
        // - The desugarer knows which parts of each special form to expand
        // - No duplication of special form logic
        // - Macros are expanded lazily, only when encountered

        use crate::eval::eval_core;
        use patina_frontend::Desugarer;

        // Create macro-aware desugarer with the environment
        let desugarer = Desugarer::with_env(env.clone());

        // Desugar to CoreExpr - this will expand macros as needed
        let core_expr = desugarer.desugar(expr).map_err(|e| {
            EvalError::InternalError(format!("Failed to desugar expression: {}", e))
        })?;

        // Evaluate based on mode
        match self.mode {
            EvalMode::Direct => {
                // Direct CoreExpr evaluation (faster, no continuation support)
                eval_core(&core_expr, env.clone(), &self.evaluator)
            }
            EvalMode::Cps => {
                // CPS transformation then evaluation (supports call/cc, shift/reset)
                eval_cps(&core_expr, env.clone(), &self.evaluator)
            }
        }
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
        let expr = Value::symbol("x");
        let result = backend.eval(&expr, &custom_env).unwrap();

        assert!(matches!(result, Value::Integer(99)));
    }

    #[test]
    fn test_tree_walker_cps_mode() {
        // Create backend with CPS mode
        let backend = TreeWalker::new_with_cps();
        assert_eq!(backend.mode(), EvalMode::Cps);

        // CPS mode should still evaluate simple expressions
        let expr = Value::Integer(42);
        let result = backend.eval_global(&expr).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_tree_walker_cps_arithmetic() {
        // Build (+ 1 2 3) as a Value
        use std::cell::RefCell;
        use std::rc::Rc as StdRc;

        let backend = TreeWalker::new_with_cps();

        let plus = Value::Symbol(StdRc::from("+"));
        let one = Value::Integer(1);
        let two = Value::Integer(2);
        let three = Value::Integer(3);

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
    fn test_tree_walker_mode_switch() {
        let mut backend = TreeWalker::new();
        assert_eq!(backend.mode(), EvalMode::Direct);

        backend.set_mode(EvalMode::Cps);
        assert_eq!(backend.mode(), EvalMode::Cps);

        // Should still work
        let expr = Value::Integer(100);
        let result = backend.eval_global(&expr).unwrap();
        assert!(matches!(result, Value::Integer(100)));
    }
}
