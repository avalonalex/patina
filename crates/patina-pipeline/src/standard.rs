//! Standard pipeline implementation
//!
//! This module provides the standard Scheme evaluation pipeline:
//! 1. Parse (lexer + parser)
//! 2. Desugar (convert to CoreExpr IR, expanding macros)
//! 3. CPS evaluation (full continuation support)
//!
//! All evaluation goes through CPS transformation for proper call/cc support.

use crate::error::{PipelineError, PipelineResult};
use crate::pipeline::{EvaluationStrategy, Pipeline};
use patina_frontend::{Desugarer, parser::Parser};
use patina_runtime::{Environment, Value};
use patina_tree_walker::eval::{Evaluator, eval_cps};
use std::rc::Rc;

/// Standard pipeline: parse → desugar → CPS eval
///
/// All evaluation uses CPS transformation for full continuation support.
pub struct StandardPipeline {
    evaluator: Evaluator,
    strategy: EvaluationStrategy,
}

impl StandardPipeline {
    /// Create a new standard pipeline
    pub fn new() -> Self {
        Self {
            evaluator: Evaluator::new(),
            strategy: EvaluationStrategy::CoreExpr,
        }
    }

    /// Create a pipeline with an existing evaluator
    pub fn with_evaluator(evaluator: Evaluator) -> Self {
        Self {
            evaluator,
            strategy: EvaluationStrategy::CoreExpr,
        }
    }

    /// Get a reference to the evaluator
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

        // Step 2: Desugar (with macro expansion)
        let desugarer = Desugarer::with_env(env.clone());
        let core_expr = desugarer
            .desugar(&expr)
            .map_err(|e| PipelineError::Desugaring(e.to_string()))?;

        // Step 3: CPS evaluation
        let result = eval_cps(&core_expr, env.clone(), &self.evaluator)
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

            // Step 3: Desugar (with macro expansion)
            let desugarer = Desugarer::with_env(env.clone());
            let core_expr = desugarer
                .desugar(&expr)
                .map_err(|e| PipelineError::Desugaring(e.to_string()))?;

            // Step 4: CPS evaluation
            last_result = eval_cps(&core_expr, env.clone(), &self.evaluator)
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
