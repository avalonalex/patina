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
use patina_core::TaggedValue;
use patina_frontend::{Desugarer, parser::Parser};
use patina_runtime::Environment;
use patina_tree_walker::eval::{Evaluator, eval_cps};
use std::rc::Rc;

/// Standard pipeline: parse → desugar → CPS eval
///
/// All evaluation uses CPS transformation for full continuation support.
pub struct StandardPipeline {
    evaluator: Evaluator,
}

impl StandardPipeline {
    /// Create a new standard pipeline
    pub fn new() -> Self {
        Self {
            evaluator: Evaluator::new(),
        }
    }

    /// Create a pipeline with an existing evaluator
    pub fn with_evaluator(evaluator: Evaluator) -> Self {
        Self { evaluator }
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
    fn eval(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<TaggedValue> {
        // Use the global environment's heap for parsing to ensure TaggedValue indices are valid
        let heap = self.evaluator.global_env.heap();

        // Step 1: Parse (returns TaggedValue in the shared heap)
        let mut parser = Parser::new_with_heap(code, heap.clone())
            .map_err(|e| PipelineError::Frontend(e.into()))?;
        let expr = parser
            .parse()
            .map_err(|e| PipelineError::Frontend(e.into()))?;

        // Step 2: Desugar (with macro expansion) using TaggedValue directly
        // desugar_tagged manages heap borrows internally
        let desugarer = Desugarer::with_env(env.clone());
        let core_expr = desugarer
            .desugar_tagged(expr, heap)
            .map_err(|e| PipelineError::Desugaring(e.to_string()))?;

        // Step 3: CPS evaluation (may mutate heap via define/set!)
        let result_tagged = eval_cps(&core_expr, env.clone(), &self.evaluator)
            .map_err(|e| PipelineError::Evaluation(e.to_string()))?;

        Ok(result_tagged)
    }

    fn eval_program(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<TaggedValue> {
        // Use the global environment's heap for parsing to ensure TaggedValue indices are valid
        let heap = self.evaluator.global_env.heap();

        // Step 1: Parse all expressions (using shared heap)
        let mut parser = Parser::new_with_heap(code, heap.clone())
            .map_err(|e| PipelineError::Frontend(e.into()))?;
        let mut last_result = TaggedValue::NULL;

        // Step 2: Evaluate each expression in sequence
        loop {
            // Try to parse next expression (returns TaggedValue)
            let expr = match parser.parse() {
                Ok(expr) => expr,
                Err(patina_frontend::parser::ParseError::UnexpectedEof) => break,
                Err(e) => return Err(PipelineError::Frontend(e.into())),
            };

            // Step 3: Desugar (with macro expansion) using TaggedValue directly
            // desugar_tagged manages heap borrows internally
            let desugarer = Desugarer::with_env(env.clone());
            let core_expr = desugarer
                .desugar_tagged(expr, heap)
                .map_err(|e| PipelineError::Desugaring(e.to_string()))?;

            // Step 4: CPS evaluation (may mutate heap via define/set!)
            last_result = eval_cps(&core_expr, env.clone(), &self.evaluator)
                .map_err(|e| PipelineError::Evaluation(e.to_string()))?;
        }

        Ok(last_result)
    }

    fn strategy(&self) -> EvaluationStrategy {
        EvaluationStrategy::CoreExpr
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
        assert_eq!(result.as_fixnum(), Some(6));
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
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_macro_expansion() {
        let pipeline = StandardPipeline::new();
        let env = pipeline.evaluator().global_env.clone();

        // Test that macros work (expanded by evaluator)
        let result = pipeline.eval("(or #f #t)", &env).unwrap();
        assert_eq!(result, TaggedValue::TRUE);
    }
}
