//! Special form trait definition
//!
//! This module defines the `SpecialForm` trait that all special forms must implement.
//! Special forms are syntactic constructs that don't evaluate their arguments in the
//! normal way - they have full control over evaluation order and can introduce new
//! bindings, control flow, or prevent evaluation entirely.

use crate::eval::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Trait for special form implementations
///
/// Special forms are syntactic constructs that have special evaluation rules.
/// Unlike procedures, special forms:
/// - Don't evaluate all their arguments automatically
/// - Can introduce new bindings in the environment
/// - Can control evaluation order and flow
/// - Can prevent evaluation of some arguments entirely
///
/// Examples: `quote`, `if`, `lambda`, `define`, `set!`, `begin`
///
/// # Evaluation Model
///
/// Special forms receive unevaluated arguments and have complete control over:
/// - Which arguments to evaluate
/// - When to evaluate them
/// - In what environment to evaluate them
/// - Whether to perform tail calls
///
/// # Tail Call Optimization
///
/// Special forms must properly handle tail position by returning the appropriate
/// `EvalResult` variant:
/// - `EvalResult::Value(v)` - Return a computed value
/// - `EvalResult::TailCall { expr, env }` - Tail call to be trampolined
/// - `EvalResult::TailCallPrimitive { proc, args }` - Tail call to primitive
///
/// # Examples
///
/// ```ignore
/// // Quote never evaluates its argument
/// (quote x) => x  (returns symbol x, not its value)
///
/// // If only evaluates one branch
/// (if #t (+ 1 2) (/ 1 0))  => 3  (doesn't evaluate division)
///
/// // Lambda captures environment without evaluation
/// (lambda (x) (+ x 1))  => <procedure>
/// ```
pub trait SpecialForm {
    /// The name of this special form
    ///
    /// This is the symbol that triggers this form in code.
    /// For example: "quote", "if", "lambda", "define", etc.
    fn name(&self) -> &'static str;

    /// Evaluate this special form
    ///
    /// This is the core evaluation method. The special form has complete control
    /// over how its arguments are evaluated.
    ///
    /// # Arguments
    ///
    /// * `evaluator` - The evaluator instance (for recursive evaluation)
    /// * `args` - The arguments AFTER the special form name (unevaluated)
    /// * `env` - The environment in which to evaluate
    /// * `in_tail_position` - Whether this form is in tail position
    ///
    /// # Argument Format
    ///
    /// The `args` parameter is the rest of the list after the special form name.
    /// For example:
    /// - `(quote x)` -> args is `(x)` as a Value
    /// - `(if test then else)` -> args is `(test then else)` as a Value
    /// - `(lambda (x) body)` -> args is `((x) body)` as a Value
    ///
    /// # Returns
    ///
    /// An `EvalResult` which can be:
    /// - `Value(v)` - A computed value (not in tail position or can't tail call)
    /// - `TailCall { expr, env }` - A tail call to evaluate expr in env
    /// - `TailCallPrimitive { proc, args }` - A tail call to a primitive procedure
    ///
    /// If `in_tail_position` is true, the form should return a tail call when possible
    /// to enable proper tail call optimization (required by R7RS).
    ///
    /// # Errors
    ///
    /// Returns `EvalError` for:
    /// - Invalid syntax (wrong number of arguments, malformed expressions)
    /// - Type errors (e.g., non-symbol in variable binding)
    /// - Evaluation errors from recursive evaluation
    ///
    /// # Example Implementation
    ///
    /// ```ignore
    /// impl SpecialForm for QuoteForm {
    ///     fn name(&self) -> &'static str {
    ///         "quote"
    ///     }
    ///
    ///     fn eval(
    ///         &self,
    ///         evaluator: &Evaluator,
    ///         args: &Value,
    ///         _env: &Rc<Environment>,
    ///         _in_tail_position: bool,
    ///     ) -> Result<EvalResult, EvalError> {
    ///         // Extract single argument
    ///         let (datum, rest) = evaluator.extract_pair(args)?;
    ///         if !matches!(rest, Value::Null) {
    ///             return Err(EvalError::InvalidSyntax(
    ///                 "quote expects exactly one argument".into()
    ///             ));
    ///         }
    ///         // Return datum without evaluating
    ///         Ok(EvalResult::Value(datum))
    ///     }
    /// }
    /// ```
    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError>;

    /// Get help text for this special form
    ///
    /// This is used by documentation and help systems.
    /// The default implementation returns a generic message.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn help(&self) -> &'static str {
    ///     "(quote datum) returns datum without evaluating it."
    /// }
    /// ```
    fn help(&self) -> &'static str {
        "No documentation available."
    }

    /// Validate syntax without evaluating (optional)
    ///
    /// This method can be used for:
    /// - Better error messages (check syntax before evaluation)
    /// - Static analysis tools
    /// - IDE support (syntax highlighting, linting)
    ///
    /// The default implementation does no validation (always succeeds).
    /// Forms can override this to provide stricter syntax checking.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if syntax is valid
    /// - `Err(EvalError::InvalidSyntax(_))` with detailed error message
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
    ///     // Ensure we have exactly one argument
    ///     let (_first, rest) = extract_pair(args)?;
    ///     if !matches!(rest, Value::Null) {
    ///         return Err(EvalError::InvalidSyntax(
    ///             "quote expects exactly one argument".into()
    ///         ));
    ///     }
    ///     Ok(())
    /// }
    /// ```
    fn validate_syntax(&self, _args: &Value) -> Result<(), EvalError> {
        Ok(())
    }
}
