//! ApplyContext implementation for the tree-walking Evaluator

use super::{EvalResult, Evaluator};
use patina_core::{GcDeferGuard, TaggedValue};
use patina_primitives::ApplyContext;
use patina_runtime::EvalError;
use patina_runtime::{Environment, Library};
use std::rc::Rc;

impl ApplyContext for Evaluator {
    /// A primitive that takes the context, called where no step runs (an
    /// embedder's call through this detached context) or by a step through
    /// `CallbackContext`, which comes here (#706).
    fn call_cx(
        &self,
        handler: patina_primitives::CxHandler,
        args: &[TaggedValue],
    ) -> Result<TaggedValue, EvalError> {
        // SAFETY: a primitive that takes the context cannot call back, so
        // it reaches no safe point: the tree-walker's are at the top of its
        // trampoline, between steps, and this call is inside one step or
        // outside any run. Nothing collects while it runs. Its arguments
        // are the call's, read from the step as it was made.
        unsafe {
            patina_core::heap::cx::enter_call(
                &self.heap,
                &self.mutator,
                &self.instance,
                args,
                |cx, args| handler(cx, args).map(patina_core::Value::tagged),
            )
        }
    }

    /// A higher-order primitive's call back (#706): [`Self::apply_proc`],
    /// with its value branded in the primitive's context.
    fn apply_in<'gc>(
        &self,
        _cx: &mut patina_core::Cx<'gc>,
        proc: patina_core::Value<'gc>,
        args: &[patina_core::Value<'gc>],
    ) -> Result<patina_core::Value<'gc>, EvalError> {
        let args = patina_core::Value::tagged_slice(args).to_vec();
        #[expect(
            clippy::disallowed_methods,
            reason = "the call back `apply_in` is for, deferred as `apply_proc`'s own reason \
                      says; each of its callers gives what it holds across it, at its call"
        )]
        let result = self.apply_proc(proc.tagged(), args)?;
        // SAFETY: `apply_proc` runs the call under `GcDeferGuard::holding`,
        // so nothing collects, and the word it answered stays live until the
        // primitive's context, whose brand `'gc` is, closes.
        Ok(unsafe { patina_core::Value::from_tagged(result) })
    }

    /// A resumable primitive's half or a higher-order primitive, in a context
    /// made as [`Self::call_cx`] makes one (#706).
    fn step_cx(
        &self,
        args: &[TaggedValue],
        step: patina_primitives::StepFn<'_>,
    ) -> Result<patina_primitives::TaggedStep, EvalError> {
        // SAFETY: as `call_cx`'s. A resumable primitive's half answers a step
        // rather than calling back; a higher-order primitive calls back on a
        // nested trampoline (`apply_in`), as `eval`'s check runs one in
        // expanding a form, and a nested trampoline keeps a `NoGcScope`:
        // nothing collects while the primitive runs. Its arguments are the
        // call's, or the state and result its `ResumePrimitive` continuation
        // held.
        unsafe {
            patina_core::heap::cx::enter_call(
                &self.heap,
                &self.mutator,
                &self.instance,
                args,
                |cx, args| step(cx, args),
            )
        }
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
        let _gc_defer = GcDeferGuard::holding(&self.heap);
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
        let _gc_defer = GcDeferGuard::holding(&self.heap);
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
    fn load_scheme_library(
        &self,
        _cx: &mut patina_core::Cx<'_>,
        name: &[String],
    ) -> Result<Rc<Library>, EvalError> {
        let _gc_defer = GcDeferGuard::holding(&self.heap);
        self.load_library(name)
            .map_err(patina_runtime::LibraryError::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<Environment> {
        self.global_env.clone()
    }
}
