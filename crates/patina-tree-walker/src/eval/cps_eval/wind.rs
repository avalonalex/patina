//! Dynamic-wind and promise handling for CPS evaluation
//!
//! This module contains functions for:
//! - Jumping to a captured continuation, running the wind thunks on the way
//! - Forcing promises in CPS mode

use super::CpsEvaluator;
use super::types::{
    ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult, set_pending_escape,
};
use crate::eval::error::EvalError;
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use std::rc::Rc;

/// How many leading records two wind stacks share (R7RS §6.10's common
/// prefix), by identity of the `dynamic-wind` call.
fn common_wind_prefix(from: &[DynamicWindRecord], to: &[DynamicWindRecord]) -> usize {
    from.iter()
        .zip(to.iter())
        .take_while(|(a, b)| a.id == b.id)
        .count()
}

impl<'a> CpsEvaluator<'a> {
    /// Take the next step of a jump to `target`: run one wind thunk between
    /// the live wind stack and the target's, or, with none left, park the
    /// escape and unwind the Rust stack to the outermost trampoline.
    ///
    /// `target` is a full continuation or an abort's landing (`prompts.rs`),
    /// never a composable one: those *return* to their invoker, so they are
    /// entered by `resume_composable` rather than jumped to. `prompt_stack`
    /// rides along untouched — a thunk on the way may itself abort, and it
    /// needs the prompt it aborts to still findable; arrival replaces the
    /// stack with the target's.
    ///
    /// This is chibi's "travel to point", one thunk per step: leave the
    /// innermost extent not shared with the target (pop its record, run its
    /// `after`), until the live stack is a prefix of the target's; then enter
    /// the target's remaining extents outermost first (run `before`, push the
    /// record). The step after each thunk is `ContValue::Jump`, which comes
    /// back here.
    ///
    /// Each thunk runs in the dynamic environment of its own `dynamic-wind`
    /// call (R7RS §6.10): the wind stack below its record, and the handler
    /// stack the record captured. So a raise in an after-thunk reaches the
    /// `guard` whose escape is running it, the guard's handler fires a second
    /// time, and its second jump — starting from the stack this one had got
    /// to — abandons this jump and runs the after-thunks still outstanding.
    /// That is the `finally` rule Track L §6 asked for: the after-thunk's
    /// exception replaces the one in flight, and unwinding continues.
    ///
    /// Popping the record *before* running its after-thunk is what makes the
    /// second jump terminate: the thunk is not on the stack it jumps from.
    pub(super) fn jump_to_continuation(
        &self,
        value: TaggedValue,
        target: Rc<CpsContinuation>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        mut dynamic_winds: Vec<DynamicWindRecord>,
    ) -> Result<StepResult, EvalError> {
        let common = common_wind_prefix(&dynamic_winds, &target.dynamic_winds);

        if dynamic_winds.len() > common {
            let record = dynamic_winds.pop().expect("longer than its prefix");
            let handlers = record.handlers.to_vec();
            return Ok(StepResult::ApplyProc {
                proc: record.after,
                args: vec![],
                cont: ContValue::Jump {
                    entered: None,
                    value,
                    target,
                },
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers: handlers,
            });
        }

        if let Some(record) = target.dynamic_winds.get(dynamic_winds.len()) {
            let record = record.clone();
            let handlers = record.handlers.to_vec();
            return Ok(StepResult::ApplyProc {
                proc: record.before,
                args: vec![],
                cont: ContValue::Jump {
                    entered: Some(record),
                    value,
                    target,
                },
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers: handlers,
            });
        }

        // Arrived. The trampoline that catches this resumes `target` with the
        // environment it captured (`mod.rs`). Every `apply_from_direct_tagged`
        // between here and there unwinds on the way — its loop `?`s each step
        // — but a nested `eval_cps` run (the `eval` primitive, through
        // `ApplyContext::eval_expr`) has its own copy of the catching arm and
        // resumes `target` *inside* itself, so the escape never leaves it and
        // the primitive returns the nested run's `Halt` value instead. That
        // is one face of PRD §6's open "primitive's callback" entry.
        set_pending_escape(value, target);
        Err(EvalError::ContinuationEscape)
    }

    /// Force a promise in CPS mode
    ///
    /// If the promise is already forced, return the cached value.
    /// Otherwise, call the thunk (which may be a CPS lambda) and cache the result.
    pub(super) fn force_promise_cps(
        &self,
        value_tagged: TaggedValue,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        let heap = self.evaluator.global_env.heap();

        // Try to extract promise from TaggedValue
        let promise_opt = heap.borrow().get_promise(value_tagged);

        if let Some(promise_ref) = promise_opt {
            let state = promise_ref.borrow();
            match *state {
                patina_core::PromiseState::Forced(v_tagged) => {
                    // Already forced - return cached TaggedValue directly
                    Ok(StepResult::InvokeContinuation {
                        cont,
                        value: v_tagged,
                        env: self.evaluator.global_env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    })
                }
                patina_core::PromiseState::Delayed(thunk_tagged) => {
                    // Need to force - call the thunk
                    drop(state); // Release borrow before calling

                    // Create a continuation that will cache the result
                    let force_cont = ContValue::ForceCache {
                        promise: value_tagged,
                        original_cont: Box::new(cont),
                    };

                    // Thunk is already TaggedValue - use directly
                    Ok(StepResult::ApplyProc {
                        proc: thunk_tagged,
                        args: vec![],
                        cont: force_cont,
                        env: self.evaluator.global_env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    })
                }
            }
        } else {
            // If not a promise, just return the value as-is
            // (make-promise can wrap non-promises)
            Ok(StepResult::InvokeContinuation {
                cont,
                value: value_tagged,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            })
        }
    }

    /// Apply a procedure from direct (non-CPS) context
    ///
    /// This is the primary entry point for calling procedures from code that
    /// wasn't compiled with CPS transformation.
    pub fn apply_from_direct_tagged(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        // This is a second trampoline and deliberately has **no safe point**:
        // its `current_step` is a Rust local that no root provider sees. The
        // guard makes "every trampoline defers for its extent" structural
        // rather than relying on this always being entered from within
        // `eval_in_env` (`docs/GC_DESIGN.md` §7).
        let _gc_defer = patina_core::GcDeferGuard::new(self.evaluator.global_env.heap());

        let env = self.evaluator.global_env.clone();
        let cont_env = ContEnv::new();
        let prompt_stack = Vec::new();
        let dynamic_winds = Vec::new();
        let exception_handlers = Vec::new();

        // Start with ApplyProc step with Halt continuation
        let mut current_step = self.apply_cps_step(
            proc,
            args,
            ContValue::Halt,
            env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        )?;

        // Run trampoline until done
        loop {
            match current_step {
                StepResult::Done(value) => {
                    return Ok(value);
                }

                StepResult::Continue {
                    expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => {
                    current_step = self.eval_one_step(
                        &expr,
                        env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    )?;
                }

                StepResult::InvokeContinuation {
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => {
                    current_step = self.invoke_continuation_step(
                        cont,
                        value,
                        env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    )?;
                }

                StepResult::ApplyProc {
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => {
                    current_step = self.apply_cps_step(
                        proc,
                        args,
                        cont,
                        env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    )?;
                }
            }
        }
    }
}
