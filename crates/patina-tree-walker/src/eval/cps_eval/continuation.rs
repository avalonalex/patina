//! Continuation handling for CPS evaluation
//!
//! This module contains functions for:
//! - Continuation invocation and dispatch
//! - Continuation reification (converting to first-class values)
//! - Decoding a reified continuation for re-entry (`continuation_cont_value`)

use super::CpsEvaluator;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use patina_core::{Environment, ScopeSet};
use std::rc::Rc;

/// The `ContValue` a reified `CpsContinuation` stands for: the stored
/// effect-carrying wrapper when present, else a `Local` rebuilt from the
/// flattened fields.
///
/// The single decoder for both re-entry paths — the escape handler in `mod.rs`
/// and the `Captured` arm of `invoke_continuation_step` — so the two cannot
/// drift apart.
pub(super) fn continuation_cont_value(k: &CpsContinuation) -> ContValue {
    match &k.resume {
        Some(cont) => cont.clone(),
        None => ContValue::Local {
            param: k.param.clone(),
            body: k.body.clone(),
            env: k.env.clone(),
            cont_env: k.captured_cont_env.clone(),
        },
    }
}

impl<'a> CpsEvaluator<'a> {
    pub(super) fn reify_continuation(
        &self,
        cont: &ContValue,
        cont_env: &ContEnv,
        dynamic_winds: &[DynamicWindRecord],
        exception_handlers: &[ExceptionHandler],
    ) -> Rc<CpsContinuation> {
        match cont {
            ContValue::Local {
                param,
                body,
                env,
                cont_env: local_cont_env,
            } => Rc::new(CpsContinuation {
                body: body.clone(),
                param: param.clone(),
                env: env.clone(),
                prompt_tag: None,
                dynamic_winds: dynamic_winds.to_vec(),
                exception_handlers: exception_handlers.to_vec(),
                captured_cont_env: local_cont_env.clone(),
                resume: None,
            }),

            // Already reified, including its handler stack: re-reifying is
            // the identity, so `exception_handlers` is deliberately unread
            // here. The continuation names the environment it was *captured*
            // in, not the one it is being handed around in.
            ContValue::Captured(k) => k.clone(),

            ContValue::Halt => Rc::new(CpsContinuation {
                body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
                    name: Rc::from("__halt_value__"),
                    scopes: ScopeSet::new(),
                }))),
                param: Rc::from("__halt_value__"),
                env: self.evaluator.global_env.clone(),
                prompt_tag: None,
                dynamic_winds: vec![],
                // Halt ends the program; nothing runs after it, so it names no
                // handlers rather than the caller's.
                exception_handlers: vec![],
                captured_cont_env: cont_env.clone(),
                resume: None,
            }),

            // Effect-carrying wrappers are stored whole in `resume`.
            //
            // Flattening them to the continuation underneath is wrong: re-entering
            // a DynamicWindCleanup has to re-establish the cleanup so the after
            // thunk still runs, not just jump past it. That is what the
            // `__dw_after__` / `__dw_wind_id__` / `__dw_original__` sentinel
            // encoding existed to preserve, in ~90 lines across three files,
            // because the storage type could not hold a ContValue. It can now.
            other => Rc::new(CpsContinuation {
                body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
                    name: Rc::from("__resume_value__"),
                    scopes: ScopeSet::new(),
                }))),
                param: Rc::from("__resume_value__"),
                env: self.evaluator.global_env.clone(),
                prompt_tag: None,
                dynamic_winds: dynamic_winds.to_vec(),
                exception_handlers: exception_handlers.to_vec(),
                captured_cont_env: cont_env.clone(),
                resume: Some(other.clone()),
            }),
        }
    }

    pub(super) fn reify_continuation_tagged(
        &self,
        cont: &ContValue,
        cont_env: &ContEnv,
        dynamic_winds: &[DynamicWindRecord],
        exception_handlers: &[ExceptionHandler],
    ) -> TaggedValue {
        let k = self.reify_continuation(cont, cont_env, dynamic_winds, exception_handlers);
        self.evaluator
            .global_env
            .heap()
            .borrow_mut()
            .alloc_continuation(k)
    }

    /// Invoke a continuation with a value, returning the next trampoline step
    ///
    /// Takes TaggedValue for efficiency - converts to Value only where needed.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn invoke_continuation_step(
        &self,
        cont: ContValue,
        value: TaggedValue,
        _env: Rc<Environment>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        match cont {
            ContValue::Local {
                param,
                body,
                env: captured_env,
                cont_env: captured_cont_env,
            } => {
                // Bind the value to the parameter in the captured value environment
                // NOTE: CPS continuations are administrative - they don't create new scopes
                // like Scheme lambdas do. We define in the captured environment directly.
                // This is important because `Define` inside a continuation body should
                // go to the original lexical scope, not an artificially created child scope.
                captured_env.define(param, value);

                // Return Continue step instead of recursive call
                // IMPORTANT: Use the continuation's captured cont_env, not the current one!
                // This ensures the continuation body can look up continuations that were
                // in scope when the continuation was defined.
                Ok(StepResult::Continue {
                    expr: body,
                    env: captured_env,
                    cont_env: captured_cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            ContValue::Captured(k) => {
                // Delivering to a captured continuation is a jump to it: the
                // same wind travel and the same re-entry (the escape handler
                // in `mod.rs`, which decodes through `continuation_cont_value`
                // and restores the environment the continuation names) as
                // applying it. Before this delegation the arm evaluated
                // `k.body` with the *caller's* cont_env and ignored `resume`
                // entirely — a resume-carrying continuation would have
                // evaluated its sentinel Halt body and died on
                // `__resume_value__`. Unreachable today (nothing constructs
                // `Captured`), load-bearing for Q2's first-class continuation
                // work.
                self.jump_to_continuation(value, k, cont_env, prompt_stack, dynamic_winds)
            }

            ContValue::Halt => {
                // Program termination - return final value as TaggedValue
                Ok(StepResult::Done(value))
            }

            ContValue::CallWithValuesConsumer {
                consumer,
                original_cont,
            } => {
                // Producer has returned a value - unpack multiple values and call consumer
                // Use heap methods to check and extract Values type without full conversion
                let heap = self.evaluator.global_env.heap();
                let consumer_args_tagged: Vec<TaggedValue> =
                    if let Some(vals) = heap.borrow_mut().get_values_as_tagged(value) {
                        vals
                    } else {
                        vec![value] // Not a Values type - use single value directly
                    };

                // Consumer is already TaggedValue - use directly for ApplyProc.proc
                // Call consumer with the unpacked values
                Ok(StepResult::ApplyProc {
                    proc: consumer,
                    args: consumer_args_tagged,
                    cont: *original_cont,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            ContValue::ForceCache {
                promise,
                original_cont,
            } => {
                // Thunk has returned a value. The promise's box is looked up
                // now, by object — see the variant's doc comment.
                let heap = self.evaluator.global_env.heap();
                let cell = heap.borrow().get_promise(promise).expect("still a promise");

                // The thunk may have forced this very promise re-entrantly;
                // R7RS 7.3 keeps the value it left ("unless (promise-done?
                // promise)") and discards the thunk's result.
                if let patina_core::PromiseState::Forced(cached) = *cell.borrow() {
                    return Ok(StepResult::InvokeContinuation {
                        cont: *original_cont,
                        value: cached,
                        env: self.evaluator.global_env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    });
                }

                // A `delay-force` thunk yields another promise: this promise
                // takes over the inner's state, the inner is re-pointed at
                // this box (`promise_update`), and forcing goes round again
                // *with the same continuation* — not a new ForceCache wrapped
                // around the old one per link, which made a chain of a
                // hundred thousand a hundred-thousand-deep continuation.
                if heap.borrow().is_promise(value) {
                    heap.borrow_mut().promise_update(promise, value);
                    return self.force_promise_cps(
                        promise,
                        *original_cont,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    );
                }

                // Cache the result (non-promise value)
                *cell.borrow_mut() = patina_core::PromiseState::Forced(value);

                // Continue with the forced value
                Ok(StepResult::InvokeContinuation {
                    cont: *original_cont,
                    value,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            // Note: ParameterizeCleanup has been removed.
            // Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
            ContValue::DynamicWindSetup {
                wind_record,
                body,
                cleanup_cont,
            } => {
                // "Before" thunk has returned (value is ignored)
                // Now push the wind record and call the body

                // Push the wind record
                let mut new_winds = dynamic_winds;
                new_winds.push(wind_record);

                // Body is already TaggedValue - use directly for ApplyProc.proc
                // Call the body thunk with the cleanup continuation
                Ok(StepResult::ApplyProc {
                    proc: body,
                    args: vec![],
                    cont: *cleanup_cont,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds: new_winds,
                    exception_handlers,
                })
            }

            ContValue::DynamicWindCleanup {
                after,
                wind_id,
                original_cont,
            } => {
                // Body has returned - pop wind record and call after thunk
                let mut new_winds = dynamic_winds;

                // Verify and pop the expected wind record
                // If IDs don't match, the wind was already unwound by a continuation jump
                if new_winds.last().is_some_and(|last| last.id == wind_id) {
                    new_winds.pop();
                }

                // Call the after thunk, then continue with the original value
                // We need another continuation to pass value through after the after thunk
                let after_done_cont = ContValue::DynamicWindAfterDone {
                    result_value: value,
                    original_cont,
                };

                // After is already TaggedValue - use directly for ApplyProc.proc
                Ok(StepResult::ApplyProc {
                    proc: after,
                    args: vec![],
                    cont: after_done_cont,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds: new_winds,
                    exception_handlers,
                })
            }

            ContValue::DynamicWindAfterDone {
                result_value,
                original_cont,
            } => {
                // "After" thunk has returned (value is ignored)
                // Continue with the saved body result
                Ok(StepResult::InvokeContinuation {
                    cont: *original_cont,
                    value: result_value,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            ContValue::Jump {
                entered,
                value: jump_value,
                target,
            } => {
                // A wind thunk on the way to `target` has returned (its value
                // is ignored). A before-thunk's extent is entered now that the
                // thunk is done, so a raise inside it did not put the record
                // on the stack the raise's escape unwinds. Then travel on from
                // the live stack: the thunk ran with the stack below its
                // record, and anything it did to the stack is balanced.
                //
                // `exception_handlers` is dropped on purpose. It is the stack
                // the thunk just ran under — its own record's, installed by
                // `jump_to_continuation` — and it belongs to no one else: the
                // next thunk gets *its* record's stack, and arrival restores
                // `target`'s from the continuation (`mod.rs`). Carrying it
                // forward would leak one `dynamic-wind` call's handlers into
                // the next thunk on the path.
                let mut new_winds = dynamic_winds;
                if let Some(record) = entered {
                    new_winds.push(record);
                }
                self.jump_to_continuation(jump_value, target, cont_env, prompt_stack, new_winds)
            }

            ContValue::ExceptionHandlerCleanup { original_cont } => {
                // Thunk completed normally - pop the exception handler and continue
                // Note: The handler was already pushed when we installed this continuation,
                // so it should already be popped from exception_handlers when the thunk ran.
                // Just continue with the value.
                let mut new_handlers = exception_handlers;
                new_handlers.pop(); // Pop the handler that was installed

                Ok(StepResult::InvokeContinuation {
                    cont: *original_cont,
                    value,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers: new_handlers,
                })
            }

            ContValue::RaiseHandlerReturn {
                continuable,
                original_exception,
                original_cont,
                popped_handler,
            } => {
                if continuable {
                    // Handler returned from raise-continuable
                    // Re-push the handler so it remains active for the rest of the
                    // thunk's dynamic extent (R7RS §6.11)
                    let mut restored_handlers = exception_handlers;
                    if let Some(handler) = popped_handler {
                        restored_handlers.push(handler);
                    }
                    // Use handler's return value as result
                    Ok(StepResult::InvokeContinuation {
                        cont: *original_cont,
                        value,
                        env: self.evaluator.global_env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers: restored_handlers,
                    })
                } else {
                    // Handler returned from non-continuable raise
                    // This is an error - raise secondary exception through CPS
                    // Create secondary exception directly as TaggedValue
                    let irritants: Vec<TaggedValue> = original_exception.into_iter().collect();
                    let secondary_exception_tagged = self
                        .evaluator
                        .global_env
                        .heap()
                        .borrow_mut()
                        .alloc_exception(
                            patina_core::ExceptionKind::Error,
                            "exception handler returned from non-continuable exception".to_string(),
                            irritants,
                        );

                    // Try to raise through the exception handler stack
                    if let Some(handler_entry) = exception_handlers.last().cloned() {
                        // Pop this handler (one-shot semantics)
                        let mut new_handlers = exception_handlers;
                        new_handlers.pop();

                        // Create continuation for when handler returns (recursively)
                        let handler_return_cont = ContValue::RaiseHandlerReturn {
                            continuable: false,
                            original_exception: Some(secondary_exception_tagged),
                            original_cont,
                            popped_handler: None,
                        };

                        // Handler is already TaggedValue - use directly for ApplyProc.proc
                        // Call handler with secondary exception
                        Ok(StepResult::ApplyProc {
                            proc: handler_entry.handler,
                            args: vec![secondary_exception_tagged],
                            cont: handler_return_cont,
                            env: self.evaluator.global_env.clone(),
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers: new_handlers,
                        })
                    } else {
                        // No handler - propagate to Rust level
                        use patina_core::ExceptionKind;
                        Err(EvalError::SchemeException {
                            kind: ExceptionKind::Error,
                            message: "exception handler returned from non-continuable exception"
                                .to_string(),
                            irritants_display: String::new(),
                        })
                    }
                }
            }
        }
    }
}
