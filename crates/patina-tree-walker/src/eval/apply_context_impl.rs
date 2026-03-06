//! ApplyContext implementation for the tree-walking Evaluator

use super::{EvalResult, Evaluator};
use patina_core::TaggedValue;
use patina_primitives::ApplyContext;
use patina_runtime::EvalError;
use patina_runtime::{Environment, Library, SharedHeap};
use std::rc::Rc;

impl ApplyContext for Evaluator {
    fn heap(&self) -> &SharedHeap {
        self.global_env.heap()
    }

    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        match self.apply(proc, args, false)? {
            EvalResult::Tagged(tv) => Ok(tv),
            EvalResult::TailCallPrimitive { proc, args } => {
                // Resolve tail call by recursing (shouldn't normally happen with in_tail=false)
                self.apply_proc(proc, args)
            }
        }
    }

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        use super::cps_eval::eval_cps;
        use patina_frontend::Desugarer;

        let desugarer = Desugarer::with_env(env.clone());
        let core_expr = desugarer
            .desugar_tagged(expr, self.global_env.heap())
            .map_err(|e| EvalError::InternalError(format!("eval: desugar error: {}", e)))?;

        eval_cps(&core_expr, env.clone(), self)
    }

    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.load_library(name)
            .map_err(|e| EvalError::InternalError(format!("cannot load library: {}", e)))
    }
}
