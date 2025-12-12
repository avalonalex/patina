//! Dynamic-wind and promise handling for CPS evaluation
//!
//! This module contains functions for:
//! - Running dynamic-wind handlers when switching contexts
//! - Forcing promises in CPS mode
//! - Creating delimited continuations

use super::CpsEvaluator;
use super::types::{ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::CpsExpr;
use patina_core::value::{CpsContinuation, DynamicWindRecord, Value};
use std::collections::HashMap;
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
            self.evaluator.apply(wind.after.clone(), vec![], false)?;
        }

        // Run "before" handlers for winds we're entering
        // This enters from the common ancestor to the target
        for wind in to.iter().skip(common_len) {
            self.evaluator.apply(wind.before.clone(), vec![], false)?;
        }

        Ok(())
    }

    /// Force a promise in CPS mode
    ///
    /// If the promise is already forced, return the cached value.
    /// Otherwise, call the thunk (which may be a CPS lambda) and cache the result.
    pub(super) fn force_promise_cps(
        &self,
        value: Value,
        cont: ContValue,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        match value {
            Value::Promise(promise_ref) => {
                let state = promise_ref.borrow();
                match &*state {
                    patina_core::value::PromiseState::Forced(v) => {
                        // Already forced - return cached value
                        Ok(StepResult::InvokeContinuation {
                            cont,
                            value: v.clone(),
                            env: self.evaluator.global_env.clone(),
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        })
                    }
                    patina_core::value::PromiseState::Delayed(thunk) => {
                        // Need to force - call the thunk
                        let thunk = thunk.clone();
                        drop(state); // Release borrow before calling

                        // Create a continuation that will cache the result
                        let force_cont = ContValue::ForceCache {
                            promise: promise_ref.clone(),
                            original_cont: Box::new(cont),
                        };

                        // Call the thunk with no arguments
                        Ok(StepResult::ApplyProc {
                            proc: thunk,
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
            }
            // If not a promise, just return the value as-is
            // (make-promise can wrap non-promises)
            other => Ok(StepResult::InvokeContinuation {
                cont,
                value: other,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            }),
        }
    }

    /// Create a delimited continuation value
    pub(super) fn make_delimited_continuation(
        &self,
        _captured_frames: Vec<PromptFrame>,
        _current_winds: Vec<DynamicWindRecord>,
        _prompt_winds: Vec<DynamicWindRecord>,
    ) -> Value {
        // TODO: Implement proper delimited continuation capture
        // For now, return a placeholder
        Value::Continuation(Rc::new(CpsContinuation {
            body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Unspecified,
            ))))),
            param: Rc::from("__delimited__"),
            env: self.evaluator.global_env.clone(),
            prompt_tag: None,
            dynamic_winds: vec![],
            captured_cont_bindings: vec![],
        }))
    }

    /// Apply a procedure from direct (non-CPS) context
    ///
    /// This is used when calling a CPS procedure from code that wasn't compiled
    /// with CPS transformation (e.g., from primitives or library code).
    pub fn apply_from_direct(&self, proc: Value, args: Vec<Value>) -> Result<Value, EvalError> {
        let env = self.evaluator.global_env.clone();
        let cont_env = HashMap::new();
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
