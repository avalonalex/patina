//! Simple pipeline-based interpreter API
//!
//! This module provides a simpler interpreter interface using the new pipeline architecture.
//! Unlike the generic `Interpreter<B: Backend>`, this provides a concrete type that's easier
//! to use and reason about.
//!
//! # Example
//!
//! ```no_run
//! use patina_interpreter::SimpleInterpreter;
//!
//! let interp = SimpleInterpreter::new();
//! let result = interp.eval_str("(+ 1 2 3)").unwrap();
//! println!("Result: {}", result);
//! ```

use patina_pipeline::{Pipeline, PipelineError, StandardPipeline};
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Simple interpreter using the standard pipeline
///
/// This is a concrete interpreter type (not generic) that uses the
/// standard parse → eval pipeline. It's simpler to use than the
/// generic `Interpreter<B>` and is recommended for most use cases.
pub struct SimpleInterpreter {
    pipeline: StandardPipeline,
}

impl SimpleInterpreter {
    /// Create a new interpreter
    ///
    /// # Example
    ///
    /// ```no_run
    /// use patina_interpreter::SimpleInterpreter;
    ///
    /// let interp = SimpleInterpreter::new();
    /// ```
    pub fn new() -> Self {
        Self {
            pipeline: StandardPipeline::new(),
        }
    }

    /// Evaluate a single Scheme expression
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use patina_interpreter::SimpleInterpreter;
    /// let interp = SimpleInterpreter::new();
    /// let result = interp.eval_str("(+ 1 2 3)").unwrap();
    /// assert_eq!(result.to_string(), "6");
    /// ```
    pub fn eval_str(&self, code: &str) -> Result<Value, PipelineError> {
        let env = self.global_env();
        self.pipeline.eval(code, &env)
    }

    /// Evaluate a Scheme program (multiple expressions)
    ///
    /// Returns the value of the last expression.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use patina_interpreter::SimpleInterpreter;
    /// let interp = SimpleInterpreter::new();
    /// let code = r#"
    ///     (define x 10)
    ///     (define y 20)
    ///     (+ x y)
    /// "#;
    /// let result = interp.eval_program(code).unwrap();
    /// assert_eq!(result.to_string(), "30");
    /// ```
    pub fn eval_program(&self, code: &str) -> Result<Value, PipelineError> {
        let env = self.global_env();
        self.pipeline.eval_program(code, &env)
    }

    /// Get the global environment
    ///
    /// Useful for inspecting or modifying global bindings.
    pub fn global_env(&self) -> Rc<Environment> {
        self.pipeline.evaluator().global_env.clone()
    }

    /// Get a reference to the underlying evaluator
    ///
    /// Useful for advanced operations like library loading.
    pub fn evaluator(&self) -> &patina_tree_walker::Evaluator {
        self.pipeline.evaluator()
    }
}

impl Default for SimpleInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_str() {
        let interp = SimpleInterpreter::new();
        let result = interp.eval_str("(+ 1 2 3)").unwrap();
        assert_eq!(result.to_string(), "6");
    }

    #[test]
    fn test_eval_program() {
        let interp = SimpleInterpreter::new();
        let code = r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#;
        let result = interp.eval_program(code).unwrap();
        assert_eq!(result.to_string(), "30");
    }

    #[test]
    fn test_global_env() {
        let interp = SimpleInterpreter::new();
        let env = interp.global_env();

        // Global environment should have primitives loaded
        assert!(env.get("cons").is_some());
        assert!(env.get("+").is_some());
    }

    #[test]
    fn test_macros() {
        let interp = SimpleInterpreter::new();
        let result = interp.eval_str("(or #f #t)").unwrap();
        assert_eq!(result.to_string(), "#t");
    }
}
