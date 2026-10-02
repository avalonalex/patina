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

    #[expect(
        clippy::disallowed_methods,
        reason = "the detached context, reached only by an embedder's call through `ApplyContext \
                  for Evaluator` (nothing in the workspace makes one). Holds nothing: `proc` and \
                  `args` move into the run. No loop runs above it, so the run may collect while a \
                  primitive that called back through it holds values (`member`'s list, \
                  `%parameterize-swap!`'s old values): unprotected"
    )]
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

    #[expect(
        clippy::disallowed_methods,
        reason = "the detached context's `eval`, reached only by an embedder's call: holds \
                  nothing, the datum moves into the expansion. No loop runs above it, so its run \
                  is outermost"
    )]
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

    #[expect(
        clippy::disallowed_methods,
        reason = "the detached context's load, reached only by an embedder's call: holds nothing"
    )]
    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        self.load_library(name)
            .map_err(patina_runtime::LibraryError::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.global_env.clone()
    }
}
