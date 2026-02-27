// InterpreterError contains DesugarError (large)
// Boxing would add complexity for minimal benefit in this interpreter context
#![allow(clippy::result_large_err)]

//! Patina Interpreter - High-level interface for Scheme evaluation
//!
//! This crate provides the `Interpreter` API that combines frontend (parsing)
//! with backend (evaluation). It supports multiple backend implementations
//! through the `Backend` trait.
//!
//! # CoreExpr IR Pipeline
//!
//! The interpreter uses a clean IR-based evaluation pipeline:
//!
//! ```text
//! String → Parser → TaggedValue AST → Macro Expander → Desugarer → CoreExpr → Evaluator → TaggedValue
//! ```
//!
//! **Architecture:**
//! - **Frontend**: Lexer → Parser → Macro Expander (produces TaggedValue AST)
//! - **Desugarer**: Converts TaggedValue AST to CoreExpr IR (9 core forms)
//! - **Backend**: Tree-walking interpreter evaluates CoreExpr
//!
//! **CoreExpr advantages:**
//! - Clean separation: frontend (homoiconic) vs backend (typed IR)
//! - Simpler evaluator: 9 core forms
//! - Better foundation for future backends (VM, JIT)
//!
//! # Example
//!
//! ```no_run
//! use patina_interpreter::Interpreter;
//! use patina_tree_walker::TreeWalker;
//!
//! let interp = Interpreter::new(TreeWalker::new());
//! let result = interp.eval_str("(+ 1 2 3)").unwrap();
//! println!("Result: {}", result);
//! ```
//!
//! # Using the Default Backend
//!
//! For convenience, a type alias `TreeWalkInterpreter` is provided that uses
//! the tree-walking backend by default:
//!
//! ```no_run
//! use patina_interpreter::TreeWalkInterpreter;
//!
//! let interp = TreeWalkInterpreter::new_tree_walker();
//! let result = interp.eval_str("(+ 1 2 3)").unwrap();
//! ```

// New pipeline-based interpreter (simpler API)
pub mod simple;
pub use simple::SimpleInterpreter;

// Re-export types from workspace crates for convenience
pub use patina_core::TaggedValue;
pub use patina_frontend::{DesugarError, Desugarer, LexError, Lexer, ParseError, Parser};
pub use patina_ir::CoreExpr;
pub use patina_pipeline::{Pipeline, PipelineError, StandardPipeline};
pub use patina_runtime::{Arity, Backend, Environment, Procedure, stdlib::test_increment_error};
pub use patina_tree_walker::{EvalError, Evaluator, TreeWalker};

/// High-level interpreter interface that combines parsing and evaluation
///
/// The interpreter is generic over the backend implementation, allowing
/// you to swap between different evaluation strategies (tree-walker, VM, JIT)
/// without changing your code.
///
/// # Type Parameters
///
/// - `B`: The backend implementation (must implement `Backend` trait)
///
/// # Example
///
/// ```ignore
/// use patina_interpreter::Interpreter;
/// use patina_tree_walker::TreeWalker;
///
/// // Create interpreter with tree-walking backend
/// let interp = Interpreter::new(TreeWalker::new());
/// let result = interp.eval_str("(+ 1 2)").unwrap();
/// ```
pub struct Interpreter<B: Backend> {
    backend: B,
}

impl<B: Backend> Interpreter<B> {
    /// Create a new interpreter with the given backend
    ///
    /// # Arguments
    ///
    /// - `backend`: The backend implementation to use for evaluation
    ///
    /// # Example
    ///
    /// ```ignore
    /// use patina_interpreter::Interpreter;
    /// use patina_tree_walker::TreeWalker;
    ///
    /// let backend = TreeWalker::new();
    /// let interp = Interpreter::new(backend);
    /// ```
    pub fn new(backend: B) -> Self {
        Interpreter { backend }
    }

    /// Evaluate a string containing Scheme code
    ///
    /// Uses the backend's evaluation strategy.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = interp.eval_str("(+ 1 2)").unwrap();
    /// ```
    pub fn eval_str(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        // Use global environment's heap for parsing to ensure TaggedValue indices are valid
        let heap = self.backend.global_env().heap();
        let mut parser = Parser::new_with_heap(input, heap.clone())?;
        let expr = parser.parse()?;
        // Drop parser to release any borrows before evaluation
        drop(parser);
        let result = self
            .backend
            .eval_global(expr)
            .map_err(InterpreterError::Backend)?;
        Ok(result)
    }

    /// Evaluate multiple expressions from a string, returning the last result
    ///
    /// This is useful for evaluating entire programs or test files.
    /// Each expression is parsed and evaluated in sequence, with the result
    /// of the last expression being returned.
    pub fn eval_program(&self, input: &str) -> Result<TaggedValue, InterpreterError<B::Error>> {
        let mut result = TaggedValue::UNSPECIFIED;
        // Use global environment's heap for parsing to ensure TaggedValue indices are valid
        let heap = self.backend.global_env().heap();
        let mut parser = Parser::new_with_heap(input, heap.clone())?;

        loop {
            // Check if we've reached EOF by attempting to parse
            match parser.parse() {
                Ok(expr) => {
                    result = self
                        .backend
                        .eval_global(expr)
                        .map_err(InterpreterError::Backend)?;
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(result)
    }

    /// Evaluate multiple expressions from a string, continuing on errors
    ///
    /// Unlike `eval_program`, this method does not stop on the first error.
    /// Instead, it prints errors to stderr and continues with the next expression.
    /// This is useful for test suites where you want to see all failures.
    ///
    /// Returns the last successfully evaluated result, or Unspecified if all failed.
    pub fn eval_program_resilient(&self, input: &str) -> TaggedValue {
        let mut result = TaggedValue::UNSPECIFIED;
        // Use global environment's heap for parsing to ensure TaggedValue indices are valid
        let heap = self.backend.global_env().heap();
        let mut parser = match Parser::new_with_heap(input, heap.clone()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                test_increment_error();
                return result;
            }
        };

        loop {
            // Check if we've reached EOF by attempting to parse
            match parser.parse() {
                Ok(expr) => match self.backend.eval_global(expr) {
                    Ok(val) => result = val,
                    Err(e) => {
                        // Print error and continue
                        eprintln!("Error: {}", e);
                        // Track this error in the test framework
                        test_increment_error();
                    }
                },
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => {
                    // Print parse error and continue
                    eprintln!("Error: {}", e);
                    test_increment_error();
                    // Try to recover by skipping to the next expression
                    // (for now, we just stop on parse errors)
                    break;
                }
            }
        }

        result
    }

    /// Get a reference to the underlying backend
    ///
    /// This allows access to backend-specific functionality that's not
    /// part of the generic `Backend` trait.
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

/// Convenience type alias for interpreter with tree-walking backend
///
/// This is the default backend and provides the same API as the previous
/// non-generic `Interpreter` implementation.
///
/// # Example
///
/// ```no_run
/// use patina_interpreter::TreeWalkInterpreter;
///
/// let interp = TreeWalkInterpreter::new_tree_walker();
/// let result = interp.eval_str("(+ 1 2 3)").unwrap();
/// ```
pub type TreeWalkInterpreter = Interpreter<TreeWalker>;

// Specialized implementation for TreeWalker backend
impl Interpreter<TreeWalker> {
    /// Create a new interpreter with the default TreeWalker backend
    ///
    /// This initializes an interpreter with full R7RS continuation support
    /// including call/cc, dynamic-wind, and exception handling.
    ///
    /// This is a convenience method that's equivalent to:
    /// ```ignore
    /// Interpreter::new(TreeWalker::new())
    /// ```
    pub fn new_tree_walker() -> Self {
        Self::new(TreeWalker::new())
    }

    /// Create an interpreter from an existing evaluator (TreeWalker-specific)
    ///
    /// This is useful for tests that need to configure the evaluator
    /// before creating the interpreter (e.g., adding search paths).
    ///
    /// This method is only available when using the TreeWalker backend.
    pub fn from_evaluator(evaluator: Evaluator) -> Self {
        Interpreter {
            backend: TreeWalker::from_evaluator(evaluator),
        }
    }

    /// Get a reference to the underlying evaluator (TreeWalker-specific)
    ///
    /// This provides access to evaluator-specific functionality.
    /// For generic backend access, use `backend()` instead.
    ///
    /// This method is only available when using the TreeWalker backend.
    pub fn evaluator(&self) -> &Evaluator {
        self.backend.evaluator()
    }

    /// Format a TaggedValue for display using write notation (machine-readable)
    ///
    /// Uses the datum writer which properly handles all TaggedValue types
    /// including heap pairs, vectors, strings, and circular structures.
    /// Multiple values (from `values`) are unpacked and displayed one per line.
    pub fn display_tagged(&self, tv: TaggedValue) -> String {
        use patina_tree_walker::eval::format_write_tagged;
        let heap = self.evaluator().global_env.heap();

        // Unpack multiple values (R7RS: each value displayed on its own line)
        let vals = heap.borrow().get_values(tv).map(|v| v.to_vec());
        if let Some(vals) = vals {
            return vals
                .iter()
                .map(|v| format_write_tagged(*v, heap))
                .collect::<Vec<_>>()
                .join("\n");
        }

        format_write_tagged(tv, heap)
    }
}

// Implement Default only for TreeWalker backend
impl Default for Interpreter<TreeWalker> {
    fn default() -> Self {
        Self::new_tree_walker()
    }
}

/// Combined error type for the interpreter
///
/// Generic over the backend error type, allowing different backends
/// to provide their own error types while maintaining a consistent
/// high-level error API.
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError<E: std::error::Error> {
    #[error("Parse error: {0}")]
    Parse(ParseError),

    #[error("Lex error: {0}")]
    Lex(LexError),

    #[error("Desugar error: {0}")]
    Desugar(DesugarError),

    #[error("Backend error: {0}")]
    Backend(E),
}

// From implementations for frontend errors
impl<E: std::error::Error> From<ParseError> for InterpreterError<E> {
    fn from(e: ParseError) -> Self {
        InterpreterError::Parse(e)
    }
}

impl<E: std::error::Error> From<LexError> for InterpreterError<E> {
    fn from(e: LexError) -> Self {
        InterpreterError::Lex(e)
    }
}

impl<E: std::error::Error> From<DesugarError> for InterpreterError<E> {
    fn from(e: DesugarError) -> Self {
        InterpreterError::Desugar(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_with_tree_walker() {
        let interp = Interpreter::new(TreeWalker::new());
        let result = interp.eval_str("42").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_tree_walk_interpreter_alias() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("42").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_interpreter_default() {
        let interp = TreeWalkInterpreter::default();
        let result = interp.eval_str("(+ 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_eval_program() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_eval_str_quote() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("'hello").unwrap();
        assert_eq!(interp.display_tagged(result), "hello");
    }

    #[test]
    fn test_eval_str_if() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("(if #t 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(1));

        let result = interp.eval_str("(if #f 1 2)").unwrap();
        assert_eq!(result.as_fixnum(), Some(2));
    }

    #[test]
    fn test_eval_lambda() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program("(define f (lambda (x) (+ x 1))) (f 41)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }

    #[test]
    fn test_eval_begin() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("(begin 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_eval_set() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_program("(define x 10) (set! x 42) x").unwrap();
        assert_eq!(result.as_fixnum(), Some(42));
    }
}
