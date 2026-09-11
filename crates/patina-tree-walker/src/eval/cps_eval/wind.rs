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

        // Arrived. Which trampoline resumes `target` is the one its chain
        // ends in (`CpsContinuation::trampoline`): this one resumes it in
        // place, as a step; any other is reached by parking the escape and
        // unwinding the Rust stack — through every primitive whose callback
        // this is, each abandoned by the `?` on its loop — until the loop
        // whose id matches catches it (`run_trampoline`). Before this test
        // every arrival parked, so a continuation captured *inside* a
        // callback and invoked there escaped the primitive too, and the
        // outermost loop then ran the rest of the callback as the rest of
        // the program: the callback's value became the form's, and the
        // `define` around the primitive never bound anything.
        if target.trampoline == super::types::current_trampoline() {
            return Ok(StepResult::InvokeContinuation {
                cont: super::continuation::continuation_cont_value(&target),
                value,
                env: target.env.clone(),
                cont_env: target.captured_cont_env.clone(),
                prompt_stack: target.prompt_stack.clone(),
                dynamic_winds: target.dynamic_winds.clone(),
                exception_handlers: target.exception_handlers.clone(),
            });
        }
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

    /// Apply a procedure from outside any step — `Backend::apply`, and the
    /// direct-mode `Evaluator::apply`. The run starts with every stack empty
    /// because there is no caller's dynamic environment to inherit; a
    /// primitive's callback goes through [`Self::apply_from_direct_with`]
    /// instead, which inherits the step's.
    pub fn apply_from_direct_tagged(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        self.apply_from_direct_with(proc, args, Vec::new(), Vec::new(), Vec::new())
    }

    /// Run `proc` on a nested trampoline under the given dynamic environment
    /// — the three stacks of the step that is calling the primitive whose
    /// callback this is — and return what it delivers to its `Halt`.
    ///
    /// Inheriting the stacks is what lets a `raise` inside the callback find
    /// the handlers installed around the primitive, a `dynamic-wind` inside
    /// it sit on the right records, and an abort inside it find a prompt
    /// outside. Until 2026-09-10 the run fabricated empty stacks, and every
    /// one of those found nothing (PRD §6's "primitive's callback" entry).
    ///
    /// The run has its own trampoline id, so a continuation captured in the
    /// callback returns here, an outer continuation invoked in the callback
    /// escapes through the primitive, and the primitive is abandoned by the
    /// `?` on the step that made the call. Its `GcDeferGuard` is the loop's
    /// (`run_trampoline`): every nested trampoline defers for its extent.
    pub(super) fn apply_from_direct_with(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<TaggedValue, EvalError> {
        let env = self.evaluator.global_env.clone();
        let initial = StepResult::ApplyProc {
            proc,
            args,
            cont: ContValue::Halt,
            env,
            cont_env: ContEnv::new(),
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        };
        self.run_trampoline(initial, None)
    }
}
