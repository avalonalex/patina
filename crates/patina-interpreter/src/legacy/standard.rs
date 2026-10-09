//! Legacy tree-walker adapter over the common interpreter.
#![allow(deprecated)]

use super::{EvaluationStrategy, Pipeline, PipelineResult};
use crate::{Environment, Evaluator, TaggedValue, TreeWalkInterpreter};
use std::rc::Rc;

/// Compatibility adapter for the historical tree-walker pipeline.
///
/// Its supplied environments must use the evaluator's heap. New hosts should
/// use `VmInterpreter` or `TreeWalkInterpreter` for typed, source-aware errors.
#[deprecated(
    note = "use VmInterpreter or TreeWalkInterpreter; this adapter always uses the tree-walker"
)]
pub struct StandardPipeline {
    interpreter: TreeWalkInterpreter,
}

impl StandardPipeline {
    pub fn new() -> Self {
        Self {
            interpreter: TreeWalkInterpreter::new_tree_walker(),
        }
    }

    pub fn with_evaluator(evaluator: Evaluator) -> Self {
        Self {
            interpreter: TreeWalkInterpreter::from_evaluator(evaluator),
        }
    }

    pub fn evaluator(&self) -> &Evaluator {
        self.interpreter.evaluator()
    }

    /// Have the heap trace `env`, which the caller holds by its own `Rc`, for
    /// as long as anything holds it: nothing else roots an environment a host
    /// built between calls (#620). An environment of another heap, which this
    /// adapter never supported, is left as it was.
    fn track(&self, env: &Rc<Environment>) {
        let global = self.interpreter.global_env();
        let heap = global.heap();
        if Rc::ptr_eq(env.heap(), heap) {
            heap.borrow().track_environment(env);
        }
    }
}

impl Default for StandardPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline for StandardPipeline {
    #[expect(
        clippy::disallowed_methods,
        reason = "the deprecated adapter over `Interpreter::eval_str_in_env`: holds nothing"
    )]
    fn eval(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<TaggedValue> {
        self.track(env);
        self.interpreter
            .eval_str_in_env(code, "<eval>", env)
            .0
            .map_err(Into::into)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the deprecated adapter over `Interpreter::eval_program_in_env`: holds nothing"
    )]
    fn eval_program(&self, code: &str, env: &Rc<Environment>) -> PipelineResult<TaggedValue> {
        self.track(env);
        self.interpreter
            .eval_program_in_env(code, "<eval>", &mut false, env)
            .0
            .map(|value| self.interpreter.raw_value(&value))
            .map_err(Into::into)
    }

    fn strategy(&self) -> EvaluationStrategy {
        EvaluationStrategy::CoreExpr
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests evaluate through the pipeline from outside any loop, as an embedder \
              does, and none reads a value from one evaluation after another (#605's shape)"
)]
mod tests {
    use super::*;

    #[test]
    fn spliced_import_supplies_syntax_before_later_forms_expand() {
        let pipeline = StandardPipeline::new();
        let env = pipeline.evaluator().global_env.clone();
        let result = pipeline
            .eval_program(
                "(begin (import (srfi 8)) (receive (a b) (values 1 2) (+ a b)))",
                &env,
            )
            .unwrap();
        assert_eq!(result.as_fixnum(), Some(3));
    }

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
