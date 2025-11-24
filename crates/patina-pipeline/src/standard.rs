//! Standard pipeline implementation
//!
//! This module provides the standard Scheme evaluation pipeline:
//! 1. Parse (lexer + parser)
//! 2. Evaluation (tree-walker with inline macro expansion)
//!
//! Macro expansion is currently handled by the evaluator during evaluation.
//! The evaluator expands macros as it encounters them, using the runtime
//! Environment directly (no separate macro environment needed).

use crate::error::{PipelineError, PipelineResult};
use crate::pipeline::{EvaluationStrategy, Pipeline};
use patina_frontend::parser::Parser;
use patina_runtime::{Environment, Value};
use patina_tree_walker::eval::Evaluator;
use std::rc::Rc;

/// Standard pipeline: parse → eval (with inline macro expansion)
///
/// Macros are expanded during evaluation as they are encountered.
/// The evaluator uses the runtime Environment directly to check for
/// macro bindings and expand them.
pub struct StandardPipeline {
    evaluator: Evaluator,
    strategy: EvaluationStrategy,
}

impl StandardPipeline {
    /// Create a new standard pipeline
    pub fn new() -> Self {
        Self {
            evaluator: Evaluator::new(),
            strategy: EvaluationStrategy::Direct,
        }
    }

    /// Create a pipeline with an existing evaluator
    ///
    /// Useful for testing or when you want to reuse an evaluator
    /// with pre-loaded libraries.
    pub fn with_evaluator(evaluator: Evaluator) -> Self {
        Self {
            evaluator,
            strategy: EvaluationStrategy::Direct,
        }
    }

    /// Get a reference to the evaluator
    ///
    /// Useful for accessing the global environment or other evaluator state
    pub fn evaluator(&self) -> &Evaluator {
        &self.evaluator
    }
}

impl Default for StandardPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline for StandardPipeline {
    fn eval(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<Value> {
        // Step 1: Parse
        let mut parser = Parser::new(code).map_err(|e| PipelineError::Frontend(e.into()))?;
        let expr = parser
            .parse()
            .map_err(|e| PipelineError::Frontend(e.into()))?;

        // Step 2: Macro expansion (currently done by evaluator)
        // TODO: Add explicit macro expansion here once evaluator is updated
        //
        // let macro_env = self.build_macro_env(env);
        // let expanded = expand_macros(&expr, &macro_env)?;

        // Step 3: Evaluate
        let result = self
            .evaluator
            .eval_in_env(&expr, env)
            .map_err(|e| PipelineError::Evaluation(e.to_string()))?;

        Ok(result)
    }

    fn eval_program(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<Value> {
        // Step 1: Parse all expressions
        let mut parser = Parser::new(code).map_err(|e| PipelineError::Frontend(e.into()))?;
        let mut last_result = Value::Null;

        // Step 2: Evaluate each expression in sequence
        loop {
            // Try to parse next expression
            let expr = match parser.parse() {
                Ok(expr) => expr,
                Err(patina_frontend::parser::ParseError::UnexpectedEof) => break,
                Err(e) => return Err(PipelineError::Frontend(e.into())),
            };

            // Step 3: Macro expansion (currently done by evaluator)
            // TODO: Add explicit macro expansion here

            // Step 4: Evaluate
            last_result = self
                .evaluator
                .eval_in_env(&expr, env)
                .map_err(|e| PipelineError::Evaluation(e.to_string()))?;
        }

        Ok(last_result)
    }

    fn strategy(&self) -> EvaluationStrategy {
        self.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_evaluation() {
        let pipeline = StandardPipeline::new();
        let env = pipeline.evaluator().global_env.clone();

        let result = pipeline.eval("(+ 1 2 3)", &env).unwrap();
        assert_eq!(result.to_string(), "6");
    }

    #[test]
    fn test_program_evaluation() {
        let pipeline = StandardPipeline::new();
        let env = pipeline.evaluator().global_env.clone();

        let code = r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#;

        let result = pipeline.eval_program(code, &env).unwrap();
        assert_eq!(result.to_string(), "30");
    }

    #[test]
    fn test_macro_expansion() {
        let pipeline = StandardPipeline::new();
        let env = pipeline.evaluator().global_env.clone();

        // Test that macros work (expanded by evaluator)
        let result = pipeline.eval("(or #f #t)", &env).unwrap();
        assert_eq!(result.to_string(), "#t");
    }
}
