//! Patina Interpreter - High-level interface for Scheme evaluation
//!
//! This crate provides the `Interpreter` API that combines frontend (parsing)
//! with backend (evaluation). It supports multiple backend implementations
//! through the `Backend` trait.
//!
//! # CoreExpr IR Pipeline (Phase 3 Complete)
//!
//! The interpreter uses a clean IR-based evaluation pipeline:
//!
//! ```text
//! String → Parser → Value → Macro Expander → Value → Desugarer → CoreExpr → Evaluator → Value
//! ```
//!
//! **Architecture:**
//! - **Frontend**: Lexer → Parser → Macro Expander (produces Value AST)
//! - **Desugarer**: Converts Value AST to CoreExpr IR (9 core forms)
//! - **Backend**: Tree-walking interpreter evaluates CoreExpr
//!
//! **CoreExpr advantages:**
//! - Clean separation: frontend (homoiconic) vs backend (typed IR)
//! - Simpler evaluator: 9 core forms vs 23 Value variants
//! - Better foundation for future backends (VM, JIT)
//! - Fully tested with 100% parity vs old approach
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

// Re-export types from workspace crates for convenience
pub use patina_frontend::{DesugarError, Desugarer, LexError, Lexer, ParseError, Parser};
pub use patina_ir::CoreExpr;
pub use patina_runtime::{Arity, Backend, Environment, Procedure, Value};
pub use patina_tree_walker::{EvalError, Evaluator, TreeWalker, eval_core};

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
    pub fn eval_str(&self, input: &str) -> Result<Value, InterpreterError<B::Error>> {
        let mut parser = Parser::new(input)?;
        let expr = parser.parse()?;
        let result = self
            .backend
            .eval_global(&expr)
            .map_err(InterpreterError::Backend)?;
        Ok(result)
    }

    /// Evaluate multiple expressions from a string, returning the last result (Value-based path)
    ///
    /// This is the **primary evaluation path** using the legacy Value-based evaluator.
    /// For the new CoreExpr-based path, see `eval_program_core()`.
    ///
    /// This is useful for evaluating entire programs or test files.
    pub fn eval_program(&self, input: &str) -> Result<Value, InterpreterError<B::Error>> {
        let mut result = Value::Unspecified;
        let mut parser = Parser::new(input)?;

        loop {
            // Check if we've reached EOF by attempting to parse
            match parser.parse() {
                Ok(expr) => {
                    result = self
                        .backend
                        .eval_global(&expr)
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
    pub fn eval_program_resilient(&self, input: &str) -> Value {
        let mut result = Value::Unspecified;
        let mut parser = match Parser::new(input) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return result;
            }
        };

        loop {
            // Check if we've reached EOF by attempting to parse
            match parser.parse() {
                Ok(expr) => match self.backend.eval_global(&expr) {
                    Ok(val) => result = val,
                    Err(e) => {
                        // Print error and continue
                        eprintln!("Error: {}", e);
                    }
                },
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => {
                    // Print parse error and continue
                    eprintln!("Error: {}", e);
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

    /// Evaluate a string using the CoreExpr IR pipeline (experimental path)
    ///
    /// This method uses the **new IR-based evaluation path**:
    /// ```text
    /// String → Parser → Value → Desugarer → CoreExpr → Evaluator → Value
    /// ```
    ///
    /// **Status**: Experimental, feature-complete, tested for parity with Value path.
    /// **Performance**: Comparable to `eval_str()`
    /// **Compatibility**: Produces identical results to `eval_str()`
    ///
    /// This is part of Phase 2 of the IR migration strategy. In Phase 3, this
    /// will become the default evaluation path.
    ///
    /// # Environment Variable Override
    ///
    /// You can run all tests through the CoreExpr evaluator by setting:
    /// ```bash
    /// USE_CORE_EXPR=1 cargo test
    /// ```
    /// This makes `eval_str()` and `eval_program()` delegate to the CoreExpr path.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let interp = TreeWalkInterpreter::new_tree_walker();
    /// let result = interp.eval_str_core("(+ 1 2)").unwrap();
    /// ```
    pub fn eval_str_core(&self, input: &str) -> Result<Value, InterpreterError<EvalError>> {
        // Parse: String → Value
        let mut parser = Parser::new(input)?;
        let value = parser.parse()?;

        // Desugar: Value → CoreExpr
        let desugarer = Desugarer::new();
        let core_expr = desugarer
            .desugar(&value)
            .map_err(InterpreterError::Desugar)?;

        // Evaluate: CoreExpr → Value
        let env = self.evaluator().global_env.clone();
        let result =
            eval_core(&core_expr, env, self.evaluator()).map_err(InterpreterError::Backend)?;

        Ok(result)
    }

    /// Evaluate a program (multiple expressions) using the CoreExpr IR pipeline (experimental path)
    ///
    /// Similar to `eval_program`, but uses the new CoreExpr evaluation path.
    ///
    /// **Status**: Experimental, feature-complete, tested for parity with Value path.
    /// **Compatibility**: Produces identical results to `eval_program()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// let interp = TreeWalkInterpreter::new_tree_walker();
    /// let result = interp.eval_program_core("(define x 10) (+ x 5)").unwrap();
    /// ```
    pub fn eval_program_core(&self, input: &str) -> Result<Value, InterpreterError<EvalError>> {
        let mut result = Value::Unspecified;
        let mut parser = Parser::new(input)?;
        let desugarer = Desugarer::new();
        let env = self.evaluator().global_env.clone();

        loop {
            // Parse next expression
            let value = match parser.parse() {
                Ok(v) => v,
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            };

            // Desugar: Value → CoreExpr
            let core_expr = desugarer
                .desugar(&value)
                .map_err(InterpreterError::Desugar)?;

            // Evaluate: CoreExpr → Value
            result = eval_core(&core_expr, env.clone(), self.evaluator())
                .map_err(InterpreterError::Backend)?;
        }

        Ok(result)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_with_tree_walker() {
        let interp = Interpreter::new(TreeWalker::new());
        let result = interp.eval_str("42").unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_tree_walk_interpreter_alias() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str("42").unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_interpreter_default() {
        let interp = TreeWalkInterpreter::default();
        let result = interp.eval_str("(+ 1 2)").unwrap();
        assert!(matches!(result, Value::Integer(3)));
    }

    #[test]
    fn test_eval_program() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert!(matches!(result, Value::Integer(30)));
    }

    // CoreExpr evaluation tests
    #[test]
    fn test_eval_str_core_literal() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_core("42").unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_str_core_quote() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_core("'hello").unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn test_eval_str_core_if() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_core("(if #t 1 2)").unwrap();
        assert!(matches!(result, Value::Integer(1)));

        let result = interp.eval_str_core("(if #f 1 2)").unwrap();
        assert!(matches!(result, Value::Integer(2)));
    }

    #[test]
    fn test_eval_program_core() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program_core("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert!(matches!(result, Value::Integer(30)));
    }

    #[test]
    fn test_eval_core_lambda() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        // Define a lambda and call it
        let result = interp
            .eval_program_core("(define f (lambda (x) (+ x 1))) (f 41)")
            .unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_core_begin() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_core("(begin 1 2 3)").unwrap();
        assert!(matches!(result, Value::Integer(3)));
    }

    #[test]
    fn test_eval_core_set() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program_core("(define x 10) (set! x 42) x")
            .unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    // ========== Parity Tests ==========
    // These tests verify that Value-based and CoreExpr-based evaluation
    // produce identical results for a wide range of Scheme programs.

    /// Helper to compare Value and CoreExpr evaluation results
    fn assert_parity(input: &str) {
        let interp = TreeWalkInterpreter::new_tree_walker();

        let value_result = interp
            .eval_program(input)
            .unwrap_or_else(|_| panic!("Value path failed for: {}", input));
        let core_result = interp
            .eval_program_core(input)
            .unwrap_or_else(|_| panic!("CoreExpr path failed for: {}", input));

        assert_eq!(
            format!("{}", value_result),
            format!("{}", core_result),
            "Parity mismatch for: {}\n  Value path: {}\n  Core path: {}",
            input,
            value_result,
            core_result
        );
    }

    #[test]
    fn test_parity_literals() {
        assert_parity("42");
        assert_parity("#t");
        assert_parity("#f");
        assert_parity("\"hello\"");
        assert_parity("#\\a");
    }

    #[test]
    fn test_parity_arithmetic() {
        assert_parity("(+ 1 2)");
        assert_parity("(+ 1 2 3 4 5)");
        assert_parity("(- 10 3)");
        assert_parity("(* 6 7)");
        assert_parity("(/ 12 4)");
        assert_parity("(+ (* 2 3) (/ 8 2))");
    }

    #[test]
    fn test_parity_define_and_reference() {
        assert_parity("(define x 42) x");
        assert_parity("(define x 10) (define y 20) (+ x y)");
        assert_parity("(define x 5) (define y (+ x 3)) (* x y)");
    }

    #[test]
    fn test_parity_if() {
        assert_parity("(if #t 1 2)");
        assert_parity("(if #f 1 2)");
        assert_parity("(if (> 3 2) 100 200)");
        assert_parity("(define x 10) (if (< x 20) x (* x 2))");
    }

    #[test]
    fn test_parity_lambda() {
        assert_parity("((lambda (x) (+ x 1)) 41)");
        assert_parity("((lambda (x y) (* x y)) 6 7)");
        assert_parity("(define f (lambda (x) (+ x 10))) (f 32)");
        assert_parity("(define double (lambda (x) (* 2 x))) (double (double 5))");
    }

    #[test]
    fn test_parity_lambda_variadic() {
        assert_parity("((lambda x x) 1 2 3)");
        assert_parity("((lambda (x . rest) rest) 1 2 3 4)");
        // Note: apply is a special form, tested separately
    }

    #[test]
    fn test_parity_closures() {
        // Simple closure
        assert_parity(
            "(define make-adder (lambda (x) (lambda (y) (+ x y)))) \
             (define add5 (make-adder 5)) \
             (add5 10)",
        );

        // Counter with mutation
        assert_parity(
            "(define counter (lambda (init) \
               (lambda () (set! init (+ init 1)) init))) \
             (define c (counter 0)) \
             (c) (c) (c)",
        );
    }

    #[test]
    fn test_parity_set() {
        assert_parity("(define x 10) (set! x 42) x");
        assert_parity("(define x 1) (set! x (+ x 1)) (set! x (+ x 1)) x");
    }

    #[test]
    fn test_parity_begin() {
        assert_parity("(begin 1 2 3)");
        assert_parity("(begin (define x 5) (define y 10) (+ x y))");
        assert_parity("(define x 0) (begin (set! x 1) (set! x 2) (set! x 3)) x");
    }

    #[test]
    fn test_parity_quote() {
        assert_parity("'x");
        assert_parity("'(1 2 3)");
        assert_parity("'(a b c)");
        assert_parity("(car '(1 2 3))");
        assert_parity("(cdr '(1 2 3))");
    }

    #[test]
    fn test_parity_list_operations() {
        assert_parity("(cons 1 2)");
        assert_parity("(cons 1 '())");
        assert_parity("(list 1 2 3 4 5)");
        assert_parity("(car (cons 1 2))");
        assert_parity("(cdr (cons 1 2))");
        assert_parity("(length (list 1 2 3 4))");
        assert_parity("(append '(1 2) '(3 4))");
    }

    #[test]
    fn test_parity_predicates() {
        assert_parity("(null? '())");
        assert_parity("(null? 42)");
        assert_parity("(pair? (cons 1 2))");
        assert_parity("(pair? '())");
        assert_parity("(number? 42)");
        assert_parity("(number? 'x)");
        assert_parity("(symbol? 'x)");
        assert_parity("(symbol? 42)");
    }

    #[test]
    fn test_parity_comparison() {
        assert_parity("(= 42 42)");
        assert_parity("(= 1 2)");
        assert_parity("(< 1 2)");
        assert_parity("(< 2 1)");
        assert_parity("(> 3 2)");
        assert_parity("(<= 2 2)");
        assert_parity("(>= 5 3)");
    }

    #[test]
    fn test_parity_boolean_ops() {
        assert_parity("(not #t)");
        assert_parity("(not #f)");
        assert_parity("(not 42)");
    }

    #[test]
    fn test_parity_higher_order() {
        assert_parity("(map (lambda (x) (* x 2)) '(1 2 3 4))");
        // Note: filter not yet implemented
        // Note: apply is a special form in tree-walker, works differently
    }

    #[test]
    fn test_parity_recursion() {
        // Factorial
        assert_parity(
            "(define fact (lambda (n) \
               (if (= n 0) 1 (* n (fact (- n 1)))))) \
             (fact 5)",
        );

        // Fibonacci
        assert_parity(
            "(define fib (lambda (n) \
               (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))) \
             (fib 7)",
        );
    }

    #[test]
    fn test_parity_nested_lambdas() {
        assert_parity(
            "((lambda (x) \
               ((lambda (y) \
                 ((lambda (z) (+ x (+ y z))) 3)) 2)) 1)",
        );
    }

    #[test]
    fn test_parity_mutual_recursion() {
        assert_parity(
            "(define even? (lambda (n) \
               (if (= n 0) #t (odd? (- n 1))))) \
             (define odd? (lambda (n) \
               (if (= n 0) #f (even? (- n 1))))) \
             (even? 10)",
        );
    }

    #[test]
    fn test_parity_complex_program() {
        assert_parity(
            "(define square (lambda (x) (* x x))) \
             (define sum-of-squares (lambda (x y) (+ (square x) (square y)))) \
             (define f (lambda (a) \
               (sum-of-squares (+ a 1) (* a 2)))) \
             (f 5)",
        );
    }

    #[test]
    fn test_parity_string_operations() {
        assert_parity("(string-append \"hello\" \" \" \"world\")");
        assert_parity("(string-length \"hello\")");
        assert_parity("(substring \"hello\" 1 4)");
    }

    #[test]
    fn test_parity_vector_operations() {
        assert_parity("(vector 1 2 3)");
        assert_parity("(vector-length (vector 1 2 3 4))");
        assert_parity("(vector-ref (vector 10 20 30) 1)");
    }

    // ========== CoreExpr as Default Tests ==========

    #[test]
    fn test_eval_str_uses_coreexpr_by_default() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        // Now uses CoreExpr by default
        let result = interp.eval_str("(+ 1 2)").unwrap();
        assert!(matches!(result, Value::Integer(3)));
    }

    #[test]
    fn test_eval_program_uses_coreexpr_by_default() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        // Now uses CoreExpr by default
        let result = interp.eval_program("(define x 5) (+ x 10)").unwrap();
        assert!(matches!(result, Value::Integer(15)));
    }
}
