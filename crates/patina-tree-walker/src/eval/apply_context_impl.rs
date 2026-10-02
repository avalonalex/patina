//! ApplyContext implementation for the tree-walking Evaluator

use super::{EvalResult, Evaluator};
use patina_core::{GcDeferGuard, TaggedValue};
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

    /// The detached context: an embedder's call through `ApplyContext for
    /// Evaluator` (`Interpreter::evaluator()`), with no machine loop above
    /// it, so nothing would defer the run it starts. A primitive applied here
    /// calls back through this same context and holds its own values in Rust
    /// meanwhile (`member`'s list, `%parameterize-swap!`'s old values), so
    /// each method defers as a holder for its extent, as a machine's nested
    /// loop defers beneath a primitive (#622).
    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        let _gc_defer = GcDeferGuard::holding(self.heap());
        let (mut proc, mut args) = (proc, args);
        loop {
            #[expect(
                clippy::disallowed_methods,
                reason = "holds nothing of its own: `proc` and `args` move into the run. The \
                          primitive that called back, if one did, holds its values across it, \
                          safe under this function's `GcDeferGuard::holding`: no collection \
                          runs until the detached call returns"
            )]
            let applied = self.apply(proc, args, false)?;
            match applied {
                EvalResult::Tagged(tv) => return Ok(tv),
                // Not expected with `in_tail_position` false; run it here.
                EvalResult::TailCallPrimitive {
                    proc: next,
                    args: next_args,
                } => (proc, args) = (next, next_args),
            }
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the detached context's `eval`: holds nothing of its own, the datum moves into \
                  the expansion; a primitive that called back holds its values across it, safe \
                  under this function's `GcDeferGuard::holding`"
    )]
    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<Environment>,
    ) -> Result<TaggedValue, EvalError> {
        let _gc_defer = GcDeferGuard::holding(self.heap());
        // A call from outside any step: the same path a step's `eval` takes
        // (`cps_eval/callback.rs`), with nothing to inherit.
        let cps = super::cps_eval::CpsEvaluator::new(self);
        super::cps_eval::CallbackContext::detached(&cps).eval_expr(expr, env)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the detached context's load: holds nothing of its own; a primitive that called \
                  back holds its values across it, safe under this function's \
                  `GcDeferGuard::holding`"
    )]
    fn load_scheme_library(&self, name: &[String]) -> Result<Rc<Library>, EvalError> {
        let _gc_defer = GcDeferGuard::holding(self.heap());
        self.load_library(name)
            .map_err(patina_runtime::LibraryError::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.global_env.clone()
    }
}
