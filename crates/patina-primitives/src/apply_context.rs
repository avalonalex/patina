//! ApplyContext trait - abstraction for backend-specific procedure application
//!
//! Primitive procedures that need to call back into the evaluator (higher-order
//! primitives like `force`, `make-parameter`, `member` with custom comparator)
//! use this trait to remain backend-agnostic.

use patina_runtime::EvalError;
use patina_runtime::{Environment, Library, SharedHeap, TaggedValue};
use std::rc::Rc;

/// Context for higher-order primitive invocations.
///
/// Implemented by each backend (tree-walker, VM) to allow shared primitive
/// implementations to call back into the evaluator when needed.
pub trait ApplyContext {
    /// Get the shared heap for value allocation
    fn heap(&self) -> &SharedHeap;

    /// Apply a procedure to arguments, returning the result.
    ///
    /// For higher-order primitives (force, member with comparator, etc.)
    /// that need to call back into the evaluator.
    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError>;

    /// Evaluate a datum expression in the given environment.
    ///
    /// Used by `eval` and `interaction-environment` primitives.
    fn eval_expr(&self, expr: TaggedValue, env: &Rc<Environment>)
    -> Result<TaggedValue, EvalError>;

    /// Load a Scheme library by name.
    ///
    /// Used by `environment`, `null-environment`, `scheme-report-environment`.
    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError>;

    /// Get the interaction (global mutable) environment.
    ///
    /// Used by `load` and `interaction-environment`.
    fn interaction_environment(&self) -> Rc<Environment>;
}
