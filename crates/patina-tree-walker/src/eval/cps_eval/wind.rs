//! Dynamic-wind and promise handling for CPS evaluation
//!
//! This module contains functions for:
//! - Running dynamic-wind handlers when switching contexts
//! - Forcing promises in CPS mode
//! - Creating delimited continuations

use super::CpsEvaluator;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Run dynamic-wind handlers when switching from one continuation to another
    ///
    /// This implements the "travel to point" algorithm from chibi-scheme:
    /// 1. Find the common prefix of the two wind stacks (by ID)
    /// 2. Run "after" handlers for winds being exited (from current to common, in reverse)
    /// 3. Run "before" handlers for winds being entered (from common to target)
    pub(super) fn run_wind_handlers(
        &self,
        from: &[DynamicWindRecord],
        to: &[DynamicWindRecord],
    ) -> Result<(), EvalError> {
        // Find the common prefix by comparing IDs
        let common_len = from
            .iter()
            .zip(to.iter())
            .take_while(|(a, b)| a.id == b.id)
            .count();

        // Run "after" handlers for winds we're leaving (in reverse order)
        // This exits from the innermost to the common ancestor
        for wind in from.iter().skip(common_len).rev() {
            // Use CPS machinery directly with TaggedValue - no conversion needed
            self.apply_from_direct_tagged(wind.after, vec![])?;
        }

        // Run "before" handlers for winds we're entering
        // This enters from the common ancestor to the target
        for wind in to.iter().skip(common_len) {
            // Use CPS machinery directly with TaggedValue - no conversion needed
            self.apply_from_direct_tagged(wind.before, vec![])?;
        }

        Ok(())
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

    /// Create a delimited continuation value as Rc<CpsContinuation>
    pub(super) fn make_delimited_continuation(
        &self,
        _captured_frames: Vec<PromptFrame>,
        _current_winds: Vec<DynamicWindRecord>,
        _prompt_winds: Vec<DynamicWindRecord>,
    ) -> Rc<CpsContinuation> {
        // TODO: Implement proper delimited continuation capture
        // For now, return a placeholder
        Rc::new(CpsContinuation {
            body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Literal(
                patina_core::TaggedValue::UNSPECIFIED,
            )))),
            param: Rc::from("__delimited__"),
            env: self.evaluator.global_env.clone(),
            prompt_tag: None,
            dynamic_winds: vec![],
            captured_cont_env: ContEnv::new(),
            resume: None,
        })
    }

    /// Create a delimited continuation value, returning TaggedValue directly
    pub(super) fn make_delimited_continuation_tagged(
        &self,
        captured_frames: Vec<PromptFrame>,
        current_winds: Vec<DynamicWindRecord>,
        prompt_winds: Vec<DynamicWindRecord>,
    ) -> TaggedValue {
        let k = self.make_delimited_continuation(captured_frames, current_winds, prompt_winds);
        self.evaluator
            .global_env
            .heap()
            .borrow_mut()
            .alloc_continuation(k)
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
