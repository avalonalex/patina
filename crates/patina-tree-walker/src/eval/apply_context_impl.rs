//! ApplyContext implementation for the tree-walking Evaluator

use super::{EvalResult, Evaluator};
use patina_core::TaggedValue;
use patina_primitives::ApplyContext;
use patina_runtime::EvalError;
use patina_runtime::{Environment, FileSystem, Library, SharedHeap};
use std::rc::Rc;
use std::sync::Arc;

impl ApplyContext for Evaluator {
    fn heap(&self) -> &SharedHeap {
        self.global_env.heap()
    }

    fn fs(&self) -> &Arc<dyn FileSystem> {
        &self.fs
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
        // A call from outside any step: the same path a step's `eval` takes
        // (`cps_eval/callback.rs`), with nothing to inherit.
        let cps = super::cps_eval::CpsEvaluator::new(self);
        super::cps_eval::CallbackContext::detached(&cps).eval_expr(expr, env)
    }

    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.load_library(name)
            .map_err(|e| EvalError::InternalError(format!("cannot load library: {}", e)))
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.global_env.clone()
    }
}
