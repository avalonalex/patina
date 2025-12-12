//! Continuation handling for CPS evaluation
//!
//! This module contains functions for:
//! - Continuation invocation and dispatch
//! - Continuation reification (converting to first-class values)
//! - Continuation binding capture and restore

use super::CpsEvaluator;
use super::types::{ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::CpsExpr;
use patina_core::value::{CpsContinuation, DynamicWindRecord, Value};
use patina_core::{Environment, ScopeSet};
use std::collections::HashMap;
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Capture continuation bindings for serialization into a CpsContinuation
    ///
    /// When call/cc captures the current continuation, we need to serialize
    /// the cont_env so it can be restored when the continuation is invoked.
    pub(super) fn capture_cont_bindings(
        cont_env: &HashMap<Rc<str>, ContValue>,
        dynamic_winds: &[DynamicWindRecord],
    ) -> Vec<(Rc<str>, Rc<CpsContinuation>)> {
        cont_env
            .iter()
            .filter_map(|(name, cont_val)| {
                match cont_val {
                    ContValue::Local {
                        param,
                        body,
                        env,
                        cont_env: nested_cont_env,
                    } => {
                        // Recursively capture nested cont_env
                        let nested_bindings =
                            Self::capture_cont_bindings(nested_cont_env, dynamic_winds);
                        Some((
                            name.clone(),
                            Rc::new(CpsContinuation {
                                body: body.clone(),
                                param: param.clone(),
                                env: env.clone(),
                                prompt_tag: None,
                                dynamic_winds: dynamic_winds.to_vec(),
                                captured_cont_bindings: nested_bindings,
                            }),
                        ))
                    }
                    ContValue::Captured(k) => Some((name.clone(), k.clone())),
                    ContValue::Halt => None, // Halt doesn't need to be captured

                    // DynamicWindCleanup needs special handling - serialize to CpsContinuation
                    ContValue::DynamicWindCleanup {
                        after,
                        wind_id,
                        original_cont,
                    } => {
                        // Recursively capture the original continuation
                        let orig_bindings = match original_cont.as_ref() {
                            ContValue::Local {
                                param,
                                body,
                                env,
                                cont_env: nested_cont_env,
                            } => {
                                let nested =
                                    Self::capture_cont_bindings(nested_cont_env, dynamic_winds);
                                vec![(
                                    Rc::from("__dw_original__") as Rc<str>,
                                    Rc::new(CpsContinuation {
                                        body: body.clone(),
                                        param: param.clone(),
                                        env: env.clone(),
                                        prompt_tag: None,
                                        dynamic_winds: dynamic_winds.to_vec(),
                                        captured_cont_bindings: nested,
                                    }),
                                )]
                            }
                            ContValue::DynamicWindCleanup { .. } => {
                                // Nested DynamicWindCleanup - recursively capture
                                if let Some((_, k)) = Self::capture_cont_bindings(
                                    &std::iter::once((
                                        Rc::from("__inner__") as Rc<str>,
                                        original_cont.as_ref().clone(),
                                    ))
                                    .collect(),
                                    dynamic_winds,
                                )
                                .into_iter()
                                .next()
                                {
                                    vec![(Rc::from("__dw_original__"), k)]
                                } else {
                                    vec![]
                                }
                            }
                            ContValue::Halt => vec![],
                            _ => vec![],
                        };

                        // Build the captured bindings
                        let mut bindings = orig_bindings;
                        bindings.push((
                            Rc::from("__dw_after__"),
                            Rc::new(CpsContinuation {
                                body: Rc::new(CpsExpr::Literal(Rc::new(after.clone()))),
                                param: Rc::from("__unused__"),
                                env: Rc::new(Environment::new()),
                                prompt_tag: None,
                                dynamic_winds: vec![],
                                captured_cont_bindings: vec![],
                            }),
                        ));
                        bindings.push((
                            Rc::from("__dw_wind_id__"),
                            Rc::new(CpsContinuation {
                                body: Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(
                                    *wind_id as i64,
                                )))),
                                param: Rc::from("__unused__"),
                                env: Rc::new(Environment::new()),
                                prompt_tag: None,
                                dynamic_winds: vec![],
                                captured_cont_bindings: vec![],
                            }),
                        ));

                        Some((
                            name.clone(),
                            Rc::new(CpsContinuation {
                                // Special marker body
                                body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                                    Value::Symbol(Rc::from("__dynamic_wind_cleanup__")),
                                ))))),
                                param: Rc::from("__dw_value__"),
                                env: Rc::new(Environment::new()),
                                prompt_tag: None,
                                dynamic_winds: dynamic_winds.to_vec(),
                                captured_cont_bindings: bindings,
                            }),
                        ))
                    }

                    // Other special continuations - not yet implemented
                    ContValue::CallWithValuesConsumer { .. }
                    | ContValue::ForceCache { .. }
                    | ContValue::ParameterizeCleanup { .. }
                    | ContValue::DynamicWindSetup { .. }
                    | ContValue::DynamicWindAfterDone { .. }
                    | ContValue::ExceptionHandlerCleanup { .. }
                    | ContValue::RaiseHandlerReturn { .. } => None,
                }
            })
            .collect()
    }

    /// Restore continuation bindings from a captured continuation
    ///
    /// When invoking a captured continuation, we need to restore the cont_env
    /// that was in scope at the point where the continuation was captured.
    pub(super) fn restore_cont_bindings(
        &self,
        captured: &[(Rc<str>, Rc<CpsContinuation>)],
    ) -> HashMap<Rc<str>, ContValue> {
        captured
            .iter()
            .filter(|(name, _)| {
                // Skip the special __dw_* bindings used to store DynamicWindCleanup state
                !name.starts_with("__dw_")
            })
            .filter_map(|(name, k)| {
                // Check if this is a serialized DynamicWindCleanup
                let is_dw_cleanup = matches!(
                    k.body.as_ref(),
                    CpsExpr::Halt(inner) if matches!(
                        inner.as_ref(),
                        CpsExpr::Literal(v) if matches!(v.as_ref(), Value::Symbol(s) if s.as_ref() == "__dynamic_wind_cleanup__")
                    )
                );

                if is_dw_cleanup {
                    // Restore as DynamicWindCleanup
                    match self.restore_dynamic_wind_cleanup(k) {
                        Ok(cont) => Some((name.clone(), cont)),
                        Err(_) => None,
                    }
                } else {
                    // Restore as Local with recursively restored cont_env
                    let restored_nested = self.restore_cont_bindings(&k.captured_cont_bindings);
                    Some((
                        name.clone(),
                        ContValue::Local {
                            param: k.param.clone(),
                            body: k.body.clone(),
                            env: k.env.clone(),
                            cont_env: restored_nested,
                        },
                    ))
                }
            })
            .collect()
    }

    /// Restore a DynamicWindCleanup continuation from captured bindings
    ///
    /// When call/cc captures a continuation that was a DynamicWindCleanup,
    /// we serialize its state into special bindings. This function reconstructs
    /// the original DynamicWindCleanup ContValue.
    pub(super) fn restore_dynamic_wind_cleanup(
        &self,
        k: &CpsContinuation,
    ) -> Result<ContValue, EvalError> {
        // Extract the after thunk
        let after = k
            .captured_cont_bindings
            .iter()
            .find(|(name, _)| name.as_ref() == "__dw_after__")
            .and_then(|(_, cont)| {
                // The after thunk is stored in the body as a Literal
                if let CpsExpr::Literal(v) = cont.body.as_ref() {
                    Some(v.as_ref().clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                EvalError::InternalError("DynamicWindCleanup missing after thunk".to_string())
            })?;

        // Extract the wind_id
        let wind_id = k
            .captured_cont_bindings
            .iter()
            .find(|(name, _)| name.as_ref() == "__dw_wind_id__")
            .and_then(|(_, cont)| {
                if let CpsExpr::Literal(v) = cont.body.as_ref() {
                    if let Value::Integer(id) = v.as_ref() {
                        Some(*id as u64)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                EvalError::InternalError("DynamicWindCleanup missing wind_id".to_string())
            })?;

        // Extract the original continuation
        let original_cont = k
            .captured_cont_bindings
            .iter()
            .find(|(name, _)| name.as_ref() == "__dw_original__")
            .map(|(_, cont)| {
                // Recursively restore if the original was also special
                let is_dw_cleanup = matches!(
                    cont.body.as_ref(),
                    CpsExpr::Halt(inner) if matches!(
                        inner.as_ref(),
                        CpsExpr::Literal(v) if matches!(v.as_ref(), Value::Symbol(s) if s.as_ref() == "__dynamic_wind_cleanup__")
                    )
                );
                if is_dw_cleanup {
                    self.restore_dynamic_wind_cleanup(cont)
                } else {
                    // Regular continuation
                    let restored_cont_env = self.restore_cont_bindings(&cont.captured_cont_bindings);
                    Ok(ContValue::Local {
                        param: cont.param.clone(),
                        body: cont.body.clone(),
                        env: cont.env.clone(),
                        cont_env: restored_cont_env,
                    })
                }
            })
            .transpose()?
            .ok_or_else(|| {
                EvalError::InternalError(
                    "DynamicWindCleanup missing original continuation".to_string(),
                )
            })?;

        Ok(ContValue::DynamicWindCleanup {
            after,
            wind_id,
            original_cont: Box::new(original_cont),
        })
    }

    /// Reify a continuation as a first-class Value
    pub(super) fn reify_continuation(
        &self,
        cont: &ContValue,
        cont_env: &HashMap<Rc<str>, ContValue>,
        dynamic_winds: &[DynamicWindRecord],
    ) -> Value {
        match cont {
            ContValue::Local {
                param,
                body,
                env,
                cont_env: local_cont_env,
            } => {
                // Capture the continuation environment so it can be restored when invoked
                let captured_bindings = Self::capture_cont_bindings(local_cont_env, dynamic_winds);
                Value::Continuation(Rc::new(CpsContinuation {
                    body: body.clone(),
                    param: param.clone(),
                    env: env.clone(),
                    prompt_tag: None,
                    dynamic_winds: dynamic_winds.to_vec(),
                    captured_cont_bindings: captured_bindings,
                }))
            }
            ContValue::Captured(k) => Value::Continuation(k.clone()),
            ContValue::Halt => {
                // Halt continuation - create a special marker
                Value::Continuation(Rc::new(CpsContinuation {
                    body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                        name: Rc::from("__halt_value__"),
                        scopes: ScopeSet::new(),
                    }))),
                    param: Rc::from("__halt_value__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: vec![],
                    captured_cont_bindings: Self::capture_cont_bindings(cont_env, dynamic_winds),
                }))
            }
            // Special continuations need proper handling when call/cc captures them
            ContValue::DynamicWindCleanup {
                after,
                wind_id,
                original_cont,
            } => {
                // Recursively reify the original continuation
                let reified_original =
                    self.reify_continuation(original_cont, cont_env, dynamic_winds);

                // Create a CpsContinuation that will recreate the DynamicWindCleanup state
                // when invoked. We store the after thunk, wind_id, and original continuation
                // as a special structure.
                Value::Continuation(Rc::new(CpsContinuation {
                    // Special marker body that indicates this is a DynamicWindCleanup wrapper
                    body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                        Value::Symbol(Rc::from("__dynamic_wind_cleanup__")),
                    ))))),
                    param: Rc::from("__dw_value__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: dynamic_winds.to_vec(),
                    // Store the after thunk, wind_id, and reified original continuation
                    // We serialize these into the captured_cont_bindings with special names
                    captured_cont_bindings: {
                        let mut bindings = Self::capture_cont_bindings(cont_env, dynamic_winds);
                        // Store after thunk as a special binding
                        bindings.push((
                            Rc::from("__dw_after__"),
                            Rc::new(CpsContinuation {
                                body: Rc::new(CpsExpr::Literal(Rc::new(after.clone()))),
                                param: Rc::from("__unused__"),
                                env: self.evaluator.global_env.clone(),
                                prompt_tag: None,
                                dynamic_winds: vec![],
                                captured_cont_bindings: vec![],
                            }),
                        ));
                        // Store wind_id
                        bindings.push((
                            Rc::from("__dw_wind_id__"),
                            Rc::new(CpsContinuation {
                                body: Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(
                                    *wind_id as i64,
                                )))),
                                param: Rc::from("__unused__"),
                                env: self.evaluator.global_env.clone(),
                                prompt_tag: None,
                                dynamic_winds: vec![],
                                captured_cont_bindings: vec![],
                            }),
                        ));
                        // Store original continuation
                        if let Value::Continuation(orig_k) = reified_original {
                            bindings.push((Rc::from("__dw_original__"), orig_k));
                        }
                        bindings
                    },
                }))
            }

            // Other special continuations that need similar treatment
            ContValue::CallWithValuesConsumer { .. }
            | ContValue::ForceCache { .. }
            | ContValue::ParameterizeCleanup { .. }
            | ContValue::DynamicWindSetup { .. }
            | ContValue::DynamicWindAfterDone { .. }
            | ContValue::ExceptionHandlerCleanup { .. }
            | ContValue::RaiseHandlerReturn { .. } => {
                // For now, these return a placeholder. They could be enhanced similarly.
                // TODO: Implement proper capture for these special continuations
                Value::Continuation(Rc::new(CpsContinuation {
                    body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                        Value::Unspecified,
                    ))))),
                    param: Rc::from("__special__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: vec![],
                    captured_cont_bindings: Self::capture_cont_bindings(cont_env, dynamic_winds),
                }))
            }
        }
    }

    /// Invoke a continuation with a value, returning the next trampoline step
    #[allow(clippy::too_many_arguments)]
    pub(super) fn invoke_continuation_step(
        &self,
        cont: ContValue,
        value: Value,
        _env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
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
                captured_env.define(param.to_string(), value);

                // Return Continue step instead of recursive call
                // IMPORTANT: Use the continuation's captured cont_env, not the current one!
                // This ensures the continuation body can look up continuations that were
                // in scope when the continuation was defined.
                Ok(StepResult::Continue {
                    expr: body.as_ref().clone(),
                    env: captured_env,
                    cont_env: captured_cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            ContValue::Captured(k) => {
                // This is a captured continuation being invoked
                // Run dynamic-wind handlers to travel from current to captured context
                self.run_wind_handlers(&dynamic_winds, &k.dynamic_winds)?;

                // Bind value and evaluate body
                let new_env = Rc::new(Environment::with_parent(k.env.clone()));
                new_env.define(k.param.to_string(), value);

                // Return Continue step instead of recursive call
                Ok(StepResult::Continue {
                    expr: k.body.as_ref().clone(),
                    env: new_env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds: k.dynamic_winds.clone(),
                    exception_handlers,
                })
            }

            ContValue::Halt => {
                // Program termination - return final value
                Ok(StepResult::Done(value))
            }

            ContValue::CallWithValuesConsumer {
                consumer,
                original_cont,
            } => {
                // Producer has returned a value - unpack multiple values and call consumer
                let consumer_args = match value {
                    Value::Values(vals) => vals,
                    other => vec![other],
                };

                // Call consumer with the unpacked values
                Ok(StepResult::ApplyProc {
                    proc: consumer,
                    args: consumer_args,
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
                // Thunk has returned a value
                // Check if result is a promise (delay-force pattern) - need to force recursively
                if let Value::Promise(_) = &value {
                    // Result is a promise - force it recursively before caching
                    // Create a new ForceCache that will cache the final result
                    let recursive_cont = ContValue::ForceCache {
                        promise,
                        original_cont,
                    };
                    return self.force_promise_cps(
                        value,
                        recursive_cont,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    );
                }

                // Cache the result (non-promise value)
                {
                    let mut state = promise.borrow_mut();
                    *state = patina_core::value::PromiseState::Forced(value.clone());
                }

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

            ContValue::ParameterizeCleanup {
                params,
                original_cont,
            } => {
                // Body has returned - pop parameter values from stacks
                for param in &params {
                    if let Value::Parameter { values, .. } = param {
                        values.borrow_mut().pop();
                    }
                }

                // Continue with the body result
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
            } => {
                if continuable {
                    // Handler returned from raise-continuable
                    // Use handler's return value as result
                    Ok(StepResult::InvokeContinuation {
                        cont: *original_cont,
                        value,
                        env: self.evaluator.global_env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    })
                } else {
                    // Handler returned from non-continuable raise
                    // This is an error - raise secondary exception through CPS
                    let secondary_exception =
                        Value::Exception(Rc::new(patina_core::ExceptionObject {
                            kind: patina_core::ExceptionKind::Error,
                            message: "exception handler returned from non-continuable exception"
                                .to_string(),
                            irritants: original_exception.into_iter().collect(),
                        }));

                    // Try to raise through the exception handler stack
                    if let Some(handler_entry) = exception_handlers.last().cloned() {
                        // Pop this handler (one-shot semantics)
                        let mut new_handlers = exception_handlers;
                        new_handlers.pop();

                        // Create continuation for when handler returns (recursively)
                        let handler_return_cont = ContValue::RaiseHandlerReturn {
                            continuable: false,
                            original_exception: Some(secondary_exception.clone()),
                            original_cont,
                        };

                        // Call handler with secondary exception
                        Ok(StepResult::ApplyProc {
                            proc: handler_entry.handler,
                            args: vec![secondary_exception],
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
