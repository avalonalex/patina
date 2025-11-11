//! Patina - A Scheme R7RS interpreter in Rust
//!
//! This library provides the core functionality for parsing and evaluating
//! Scheme code. It can be used programmatically for testing or embedded
//! in other applications.
//!
//! # Example
//!
//! ```no_run
//! use patina::{Interpreter, Value};
//!
//! let mut interp = Interpreter::new();
//! let result = interp.eval_str("(+ 1 2 3)").unwrap();
//! assert!(matches!(result, Value::Integer(6)));
//! ```

mod env;
mod eval;
mod lexer;
pub mod macro_system;
mod parser;
pub mod repl;
mod value;

// Re-export main types for public API
pub use eval::{EvalError, Evaluator};
pub use lexer::{LexError, Lexer, Token};
pub use parser::{ParseError, Parser};
pub use value::{Arity, Procedure, Value};

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
    /// use patina::Interpreter;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_basic_arithmetic() {
        let interp = Interpreter::new();
        let result = interp.eval_str("(+ 1 2 3)").unwrap();
        assert!(matches!(result, Value::Integer(6)));
    }

    #[test]
    fn test_interpreter_define_and_use() {
        let interp = Interpreter::new();
        interp.eval_str("(define x 42)").unwrap();
        let result = interp.eval_str("x").unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_program() {
        let interp = Interpreter::new();
        let result = interp
            .eval_program(
                r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#,
            )
            .unwrap();
        assert!(matches!(result, Value::Integer(30)));
    }

    #[test]
    fn test_macro_when() {
        let interp = Interpreter::new();

        // Define the when macro
        let define_result = interp.eval_str(
            r#"
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))
"#,
        );

        if let Err(e) = define_result {
            panic!("Failed to define when macro: {}", e);
        }

        // Test single body
        let result = interp.eval_str("(when #t 42)");
        match &result {
            Ok(val) => println!("when macro result: {}", val),
            Err(e) => panic!("when macro expansion error: {}", e),
        }
        let result = result.unwrap();
        assert!(matches!(result, Value::Integer(42)));

        // Test multiple body forms
        let result = interp.eval_str("(when #t 1 2 3)").unwrap();
        assert!(matches!(result, Value::Integer(3)));

        // Test false condition
        let result = interp.eval_str("(when #f 42)").unwrap();
        assert!(matches!(result, Value::Unspecified));
    }

    #[test]
    fn test_macro_unless() {
        let interp = Interpreter::new();

        // Define the unless macro
        interp
            .eval_str(
                r#"
(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))
"#,
            )
            .unwrap();

        // Test with false condition (should execute)
        let result = interp.eval_str("(unless #f 42)").unwrap();
        assert!(matches!(result, Value::Integer(42)));

        // Test with true condition (should not execute)
        let result = interp.eval_str("(unless #t 42)").unwrap();
        assert!(matches!(result, Value::Unspecified));
    }
}

#[cfg(test)]
mod test_let_values_debug {
    use super::*;
    
    #[test]
    fn test_gcd_with_let_values() {
        let interp = Interpreter::new();
        let result = interp.eval_program(r#"
            (define (quotient-and-remainder a b)
              (values (quotient a b) (remainder a b)))

            (define (gcd a b)
              (if (= b 0)
                  a
                  (let-values (((q r) (quotient-and-remainder a b)))
                    (gcd b r))))

            (gcd 48 18)
        "#).unwrap();
        
        assert_eq!(format!("{}", result), "6");
    }
}
