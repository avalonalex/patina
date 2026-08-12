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
        use super::cps_eval::eval_cps;
        use patina_frontend::Desugarer;

        let desugarer = Desugarer::with_env(env.clone()).with_fs(self.fs.clone());
        // Bad syntax handed to the `eval` primitive is the caller's error,
        // raised while the program runs — catchable, like the VM's path.
        // (`EvalError::DesugarError` is only for the Backend::eval entry.)
        let core_expr = desugarer
            .desugar_tagged(expr, self.global_env.heap())
            .map_err(|e| EvalError::InvalidSyntax(format!("eval: desugar error: {}", e)))?;

        eval_cps(&core_expr, env.clone(), self)
    }

    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.load_library(name)
            .map_err(|e| EvalError::InternalError(format!("cannot load library: {}", e)))
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.global_env.clone()
    }
}
