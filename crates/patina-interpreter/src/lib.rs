//! Patina Interpreter - High-level interface for Scheme evaluation
//!
//! This crate provides the `Interpreter` API that combines frontend (parsing)
//! with backend (evaluation). It supports multiple backend implementations.
//!
//! # Example
//!
//! ```no_run
//! use patina_interpreter::Interpreter;
//!
//! let mut interp = Interpreter::new();
//! let result = interp.eval_str("(+ 1 2 3)").unwrap();
//! println!("Result: {}", result);
//! ```

// Re-export types from workspace crates for convenience
pub use patina_frontend::{LexError, Lexer, ParseError, Parser};
pub use patina_runtime::{Arity, Environment, Procedure, Value};
pub use patina_tree_walker::{EvalError, Evaluator};

/// High-level interpreter interface that combines parsing and evaluation
pub struct Interpreter {
    evaluator: Evaluator,
}

impl Interpreter {
    /// Create a new interpreter with a fresh environment
    pub fn new() -> Self {
        Interpreter {
            evaluator: Evaluator::new(),
        }
    }

    /// Evaluate a string containing Scheme code
    ///
    /// # Example
    ///
    /// ```no_run
    /// use patina_interpreter::Interpreter;
    ///
    /// let mut interp = Interpreter::new();
    /// let result = interp.eval_str("(+ 1 2)").unwrap();
    /// println!("Result: {}", result);
    /// ```
    pub fn eval_str(&self, input: &str) -> Result<Value, InterpreterError> {
        let mut parser = Parser::new(input)?;
        let expr = parser.parse()?;
        let result = self.evaluator.eval(&expr)?;
        Ok(result)
    }

    /// Evaluate multiple expressions from a string, returning the last result
    ///
    /// This is useful for evaluating entire programs or test files.
    pub fn eval_program(&self, input: &str) -> Result<Value, InterpreterError> {
        let mut result = Value::Unspecified;
        let mut parser = Parser::new(input)?;

        loop {
            // Check if we've reached EOF by attempting to parse
            match parser.parse() {
                Ok(expr) => {
                    result = self.evaluator.eval(&expr)?;
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(result)
    }

    /// Get a reference to the underlying evaluator
    pub fn evaluator(&self) -> &Evaluator {
        &self.evaluator
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined error type for the interpreter
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError {
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Evaluation error: {0}")]
    Eval(#[from] EvalError),
}
