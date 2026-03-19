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
pub use patina_frontend::{
    DesugarError, Desugarer, LexError, Lexer, ParseError, Parser, SourceMap,
};
pub use patina_ir::CoreExpr;
pub use patina_pipeline::{Pipeline, PipelineError, StandardPipeline};
pub use patina_runtime::{Arity, Backend, Environment, Procedure, stdlib::test_increment_error};
pub use patina_tree_walker::{EvalError, Evaluator, TreeWalker};

/// Format any `InterpreterError` with source context.
///
/// `EvalError` variants are formatted with caret context via `format_eval_error_with_source`.
/// Parse/lex/desugar errors fall back to their `Display` implementation.
pub fn format_interpreter_error(
    error: &InterpreterError<EvalError>,
    source_map: &SourceMap,
) -> String {
    match error {
        InterpreterError::Backend(eval_err) => format_eval_error_with_source(eval_err, source_map),
        other => other.to_string(),
    }
}

/// Format an `EvalError` with source context from a `SourceMap`.
///
/// When the error has a `WithLocation` variant, this prints a caret-style
/// context block showing the relevant source line and position:
///
/// ```text
///    1 | (define (foo) x)
///                     ^
/// Error: Undefined variable: x
///   at test.scm:1:15
/// ```
///
/// Falls back to `error.to_string()` when no source context is available.
pub fn format_eval_error_with_source(error: &EvalError, source_map: &SourceMap) -> String {
    if let Some(loc) = error.source_location() {
        let mut parts = vec![error.to_string()];
        parts.push(format!("  at {}", loc));
        if let Some(ctx) = source_map.format_context(loc) {
            parts.push(ctx);
        }
        // Phase 4: macro expansion chain
        if let Some(names) = source_map.get_expansions(loc) {
            if names.len() == 1 {
                parts.push(format!("  macro expansion: {}", names[0]));
            } else {
                parts.push(format!(
                    "  macro expansion chain: {}",
                    names.join(" \u{2192} ")
                ));
            }
        }
        parts.join("\n")
    } else {
        error.to_string()
    }
}

// Re-export HasSourceLocation from patina-core for convenience.
pub use patina_core::error::HasSourceLocation;

/// Format any backend error that implements `HasSourceLocation` with source context.
///
/// This is the generic version of `format_interpreter_error` that works with
/// any backend error type (tree-walker, VM, etc.).
pub fn format_backend_error_with_source<E: std::error::Error + HasSourceLocation>(
    error: &InterpreterError<E>,
    source_map: &SourceMap,
) -> String {
    match error {
        InterpreterError::Backend(backend_err) => {
            if let Some(loc) = backend_err.source_location() {
                let mut parts = vec![backend_err.to_string()];
                parts.push(format!("  at {}", loc));
                if let Some(ctx) = source_map.format_context(loc) {
                    parts.push(ctx);
                }
                if let Some(names) = source_map.get_expansions(loc) {
                    if names.len() == 1 {
                        parts.push(format!("  macro expansion: {}", names[0]));
                    } else {
                        parts.push(format!(
                            "  macro expansion chain: {}",
                            names.join(" \u{2192} ")
                        ));
                    }
                }
                parts.join("\n")
            } else {
                backend_err.to_string()
            }
        }
        other => other.to_string(),
    }
}

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

    /// Create an interpreter with a custom filesystem.
    pub fn new_tree_walker_with_fs(fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        Self::new(TreeWalker::with_fs(fs))
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

    /// Evaluate a string with source map tracking (TreeWalker-specific)
    ///
    /// Source positions from the parser are attached to CoreExpr nodes,
    /// enabling better error messages with source locations.
    pub fn eval_str_tracked(
        &self,
        input: &str,
    ) -> Result<TaggedValue, InterpreterError<EvalError>> {
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("<eval>");
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, source_map.clone())?;
        let expr = parser.parse()?;
        drop(parser);
        let global = self.backend.global_env().clone();
        let result = self
            .backend
            .eval_with_source_map(expr, &global, &source_map)
            .map_err(InterpreterError::Backend)?;
        Ok(result)
    }

    /// Evaluate a program with source map tracking (TreeWalker-specific)
    ///
    /// Source positions from the parser are attached to CoreExpr nodes.
    pub fn eval_program_tracked(
        &self,
        input: &str,
    ) -> Result<TaggedValue, InterpreterError<EvalError>> {
        let mut result = TaggedValue::UNSPECIFIED;
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("<eval>");
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, source_map.clone())?;
        let global = self.backend.global_env().clone();

        loop {
            match parser.parse() {
                Ok(expr) => {
                    result = self
                        .backend
                        .eval_with_source_map(expr, &global, &source_map)
                        .map_err(InterpreterError::Backend)?;
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(result)
    }

    /// Evaluate a program with source map tracking, continuing on errors
    pub fn eval_program_resilient_tracked(&self, input: &str) -> TaggedValue {
        let mut result = TaggedValue::UNSPECIFIED;
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("<eval>");
        let mut parser =
            match Parser::new_with_source_map(input, heap.clone(), source_name, source_map.clone())
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    test_increment_error();
                    return result;
                }
            };
        let global = self.backend.global_env().clone();

        loop {
            match parser.parse() {
                Ok(expr) => {
                    match self
                        .backend
                        .eval_with_source_map(expr, &global, &source_map)
                    {
                        Ok(val) => result = val,
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            test_increment_error();
                        }
                    }
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    test_increment_error();
                    break;
                }
            }
        }

        result
    }

    /// Evaluate a string with a named source and return the source map for error formatting.
    pub fn eval_str_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> (
        Result<TaggedValue, InterpreterError<EvalError>>,
        std::rc::Rc<std::cell::RefCell<SourceMap>>,
    ) {
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
        let mut parser =
            match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
                Ok(p) => p,
                Err(e) => return (Err(e.into()), source_map),
            };
        let expr = match parser.parse() {
            Ok(e) => e,
            Err(e) => return (Err(e.into()), source_map),
        };
        drop(parser);
        let global = self.backend.global_env().clone();
        let result = self
            .backend
            .eval_with_source_map(expr, &global, &source_map)
            .map_err(InterpreterError::Backend);
        (result, source_map)
    }

    /// Evaluate a program (multiple expressions) with a named source; return source map.
    pub fn eval_program_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> (
        Result<TaggedValue, InterpreterError<EvalError>>,
        std::rc::Rc<std::cell::RefCell<SourceMap>>,
    ) {
        let mut result = TaggedValue::UNSPECIFIED;
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
        let mut parser =
            match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
                Ok(p) => p,
                Err(e) => return (Err(e.into()), source_map),
            };
        let global = self.backend.global_env().clone();
        loop {
            match parser.parse() {
                Ok(expr) => {
                    match self
                        .backend
                        .eval_with_source_map(expr, &global, &source_map)
                        .map_err(InterpreterError::Backend)
                    {
                        Ok(val) => result = val,
                        Err(e) => return (Err(e), source_map),
                    }
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return (Err(e.into()), source_map),
            }
        }
        (Ok(result), source_map)
    }

    /// Evaluate a program resiliently with a named source; prints rich errors and continues.
    pub fn eval_program_resilient_with_source_name(
        &self,
        input: &str,
        source_name: &str,
    ) -> TaggedValue {
        let mut result = TaggedValue::UNSPECIFIED;
        let heap = self.backend.global_env().heap();
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
        let mut parser =
            match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    test_increment_error();
                    return result;
                }
            };
        let global = self.backend.global_env().clone();
        loop {
            match parser.parse() {
                Ok(expr) => {
                    match self
                        .backend
                        .eval_with_source_map(expr, &global, &source_map)
                    {
                        Ok(val) => result = val,
                        Err(e) => {
                            eprintln!(
                                "Error: {}",
                                format_eval_error_with_source(&e, &source_map.borrow())
                            );
                            test_increment_error();
                        }
                    }
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    test_increment_error();
                    break;
                }
            }
        }
        result
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

    #[test]
    fn test_eval_str_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_str_tracked("(+ 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(6));
    }

    #[test]
    fn test_eval_program_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp
            .eval_program_tracked("(define x 10) (define y 20) (+ x y)")
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_eval_program_resilient_tracked() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let result = interp.eval_program_resilient_tracked("(+ 1 2)");
        assert_eq!(result.as_fixnum(), Some(3));
    }

    #[test]
    fn test_source_info_desugarer_integration() {
        // Verify that source info flows from parser through desugarer
        let heap = patina_core::new_shared_heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let mut parser =
            Parser::new_with_source_map("(+ 1 2)", heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);

        // Desugar with source map
        let backend = TreeWalker::new();
        let env = backend.global_env().clone();
        let desugarer = Desugarer::with_env_and_source_map(env, sm);
        let internal_heap = backend.global_env().heap();
        let core_expr = desugarer.desugar_tagged(expr, internal_heap).unwrap();

        // The CoreExpr should have source info from the list form
        assert!(
            core_expr.source.is_some(),
            "CoreExpr should have source location"
        );
        let loc = core_expr.source.as_ref().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);
        assert_eq!(&*loc.source, "test.scm");
    }

    #[test]
    fn test_eval_error_with_location_tracked() {
        // When using tracked eval, undefined variable errors inside call forms carry source location.
        // We use a call form because list forms have their source position recorded by the parser.
        let interp = TreeWalkInterpreter::new_tree_walker();
        // (list undefined-variable) is a call form at 1:1 — the App CpsExpr carries that source
        let err = interp
            .eval_str_tracked("(list undefined-variable)")
            .unwrap_err();
        if let InterpreterError::Backend(eval_err) = &err {
            // The error should carry a source location from the call site
            assert!(
                eval_err.source_location().is_some(),
                "tracked eval error should have source location, got: {eval_err}"
            );
            let loc = eval_err.source_location().unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 1);
        } else {
            panic!("Expected backend error, got: {err:?}");
        }
    }

    #[test]
    fn test_format_eval_error_with_source() {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("<eval>");
        let input = "(+ 1 bad-var)";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.evaluator().global_env.clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        if let Err(eval_err) = result {
            let formatted = format_eval_error_with_source(&eval_err, &sm.borrow());
            // Should include the caret context since source text was stored
            assert!(
                formatted.contains("bad-var") || formatted.contains("at"),
                "formatted error should contain context: {formatted}"
            );
        } else {
            panic!("Expected eval error for undefined variable");
        }
    }

    #[test]
    fn test_source_map_format_context() {
        let mut sm = SourceMap::new();
        sm.set_source_text("(define (foo) x)\n(foo)".to_string());
        let loc = patina_core::error::SourceLocation::new("<test>", 1, 15);
        let ctx = sm.format_context(&loc);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert!(ctx.contains("(define (foo) x)"), "should show source line");
        assert!(ctx.contains('^'), "should show caret");
    }

    #[test]
    fn test_expansion_chain_single_macro() {
        // (let ((x 1)) (+ x y)) with undefined y should show "macro expansion: let"
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(let ((x 1)) (+ x y))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err(), "expected error for undefined y");
        let eval_err = result.unwrap_err();
        let formatted = format_eval_error_with_source(&eval_err, &sm.borrow());
        assert!(
            formatted.contains("macro expansion: let"),
            "should show macro expansion chain, got: {formatted}"
        );
    }

    #[test]
    fn test_expansion_chain_nested_macros() {
        // (cond (#t (+ 0 y))) — y is in a call position so it carries a source location.
        // cond expands to an if form; both cond and (potentially) if should appear in chain.
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(cond (#t (+ 0 y)))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err(), "expected error for undefined y");
        let eval_err = result.unwrap_err();
        let formatted = format_eval_error_with_source(&eval_err, &sm.borrow());
        // cond expands first — at minimum "macro expansion: cond" or a chain should appear
        assert!(
            formatted.contains("macro expansion"),
            "should show macro expansion info, got: {formatted}"
        );
        assert!(
            formatted.contains("cond"),
            "should mention cond in expansion, got: {formatted}"
        );
    }

    #[test]
    fn test_source_stamp_inner_forms() {
        // Inner pairs of a let expansion should have source in SourceMap after expansion
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(let ((x 1)) (+ x 2))";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name.clone(), sm.clone())
                .unwrap();
        let _expr = parser.parse().unwrap();
        // After parsing, the let form itself is in the source map
        let sm_ref = sm.borrow();
        // After expansion (triggered by eval), expansion_records should be populated
        // We verify the record_expansion mechanism via the SourceMap API directly
        let loc = patina_core::error::SourceLocation::new("test.scm", 1, 1);
        drop(sm_ref);
        // Trigger full eval so expansion happens
        let mut parser2 =
            Parser::new_with_source_map(input, heap.clone(), source_name.clone(), sm.clone())
                .unwrap();
        let expr2 = parser2.parse().unwrap();
        drop(parser2);
        let global = interp.backend().global_env().clone();
        let _ = interp.backend().eval_with_source_map(expr2, &global, &sm);
        // After eval, expansion records for (1,1) should contain "let"
        let sm_ref = sm.borrow();
        let expansions = sm_ref.get_expansions(&loc);
        assert!(
            expansions.is_some(),
            "expansion records should be populated for the let call site"
        );
        assert!(
            expansions.unwrap().contains(&"let".to_string()),
            "should record 'let' expansion"
        );
    }

    #[test]
    fn test_no_regression_unexpanded_errors() {
        // Errors from non-macro code still show location without expansion line
        let interp = TreeWalkInterpreter::new_tree_walker();
        let heap = interp.evaluator().global_env.heap();
        let sm = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let source_name: std::rc::Rc<str> = std::rc::Rc::from("test.scm");
        let input = "(+ 1 undefined-var)";
        let mut parser =
            Parser::new_with_source_map(input, heap.clone(), source_name, sm.clone()).unwrap();
        let expr = parser.parse().unwrap();
        drop(parser);
        let global = interp.backend().global_env().clone();
        let result = interp.backend().eval_with_source_map(expr, &global, &sm);
        assert!(result.is_err());
        let eval_err = result.unwrap_err();
        let formatted = format_eval_error_with_source(&eval_err, &sm.borrow());
        // Should contain error message but NOT "macro expansion" line
        assert!(
            formatted.contains("undefined-var"),
            "should show variable name: {formatted}"
        );
        assert!(
            !formatted.contains("macro expansion"),
            "non-macro error should not show expansion chain: {formatted}"
        );
    }
}
