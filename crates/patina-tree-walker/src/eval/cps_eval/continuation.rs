//! Continuation handling for CPS evaluation
//!
//! This module contains functions for:
//! - Continuation invocation and dispatch
//! - Continuation reification (converting to first-class values)
//! - Continuation binding capture and restore

use super::CpsEvaluator;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::heap::SharedHeap;
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use patina_core::{Environment, ScopeSet};
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Capture continuation bindings for serialization into a CpsContinuation
    ///
    /// When call/cc captures the current continuation, we need to serialize
    /// the cont_env so it can be restored when the continuation is invoked.
    pub(super) fn capture_cont_bindings(
        cont_env: &ContEnv,
        dynamic_winds: &[DynamicWindRecord],
        heap: &SharedHeap,
    ) -> Vec<(Rc<str>, Rc<CpsContinuation>)> {
        cont_env
            .iter()
            .filter_map(|(name, cont_val)| Self::capture_one(name, cont_val, dynamic_winds, heap))
            .collect()
    }

    /// Serialize one continuation-environment entry.
    ///
    /// Split out so the nested cases can recurse on a single binding instead of
    /// building a throwaway one-entry `ContEnv` to call back through.
    fn capture_one(
        name: &Rc<str>,
        cont_val: &ContValue,
        dynamic_winds: &[DynamicWindRecord],
        heap: &SharedHeap,
    ) -> Option<(Rc<str>, Rc<CpsContinuation>)> {
        {
            match cont_val {
                ContValue::Local {
                    param,
                    body,
                    env,
                    cont_env: nested_cont_env,
                } => {
                    // Recursively capture nested cont_env
                    let nested_bindings =
                        Self::capture_cont_bindings(nested_cont_env, dynamic_winds, heap);
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
                                Self::capture_cont_bindings(nested_cont_env, dynamic_winds, heap);
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
                            let inner = Rc::from("__dw_original__");
                            Self::capture_one(&inner, original_cont, dynamic_winds, heap)
                                .map(|(n, k)| vec![(n, k)])
                                .unwrap_or_default()
                        }
                        ContValue::Halt => vec![],
                        _ => vec![],
                    };

                    // Build the captured bindings
                    let mut bindings = orig_bindings;
                    bindings.push((
                        Rc::from("__dw_after__"),
                        Rc::new(CpsContinuation {
                            body: CpsExpr::rc(CpsExprKind::Literal(*after)), // already TaggedValue
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
                            body: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::fixnum(
                                *wind_id as i64,
                            ))),
                            param: Rc::from("__unused__"),
                            env: Rc::new(Environment::new()),
                            prompt_tag: None,
                            dynamic_winds: vec![],
                            captured_cont_bindings: vec![],
                        }),
                    ));

                    // Create marker symbol
                    let marker = heap.borrow_mut().intern_symbol("__dynamic_wind_cleanup__");
                    Some((
                        name.clone(),
                        Rc::new(CpsContinuation {
                            // Special marker body
                            body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(
                                CpsExprKind::Literal(marker),
                            ))),
                            param: Rc::from("__dw_value__"),
                            env: Rc::new(Environment::new()),
                            prompt_tag: None,
                            dynamic_winds: dynamic_winds.to_vec(),
                            captured_cont_bindings: bindings,
                        }),
                    ))
                }

                // Effect-carrying wrappers: capture straight through to the
                // continuation underneath. Dropping the entry instead
                // strands the binder its body refers to -- see
                // tests/nested_exception_handlers.rs.
                //
                // The wrapper's effect is NOT preserved, and neither is the
                // dynamic state around it: the escape path in mod.rs resets
                // `exception_handlers` and `prompt_stack` to empty on
                // re-entry rather than restoring what was captured, because
                // `CpsContinuation` has nowhere to store them. Re-entering
                // a continuation captured under a handler therefore loses
                // that handler. Pre-existing and separately tracked; the VM
                // does restore both (see `VmContinuation`).
                other => other
                    .wrapped_cont()
                    .and_then(|inner| Self::capture_one(name, inner, dynamic_winds, heap)),
            }
        }
    }

    /// Restore continuation bindings from a captured continuation
    ///
    /// When invoking a captured continuation, we need to restore the cont_env
    /// that was in scope at the point where the continuation was captured.
    pub(super) fn restore_cont_bindings(
        &self,
        captured: &[(Rc<str>, Rc<CpsContinuation>)],
    ) -> ContEnv {
        let mut env = ContEnv::new();
        for (name, k) in captured {
            // Skip the special __dw_* bindings used to store DynamicWindCleanup state
            if name.starts_with("__dw_") {
                continue;
            }

            // Check if this is a serialized DynamicWindCleanup
            let heap = self.evaluator.global_env.heap();
            let marker = heap.borrow_mut().intern_symbol("__dynamic_wind_cleanup__");
            let is_dw_cleanup = matches!(
                &k.body.as_ref().kind,
                CpsExprKind::Halt(inner) if matches!(
                    &inner.as_ref().kind,
                    CpsExprKind::Literal(v) if *v == marker
                )
            );

            let cont = if is_dw_cleanup {
                // Restore as DynamicWindCleanup
                match self.restore_dynamic_wind_cleanup(k) {
                    Ok(cont) => cont,
                    Err(_) => continue,
                }
            } else {
                // Restore as Local with recursively restored cont_env
                let restored_nested = self.restore_cont_bindings(&k.captured_cont_bindings);
                ContValue::Local {
                    param: k.param.clone(),
                    body: k.body.clone(),
                    env: k.env.clone(),
                    cont_env: restored_nested,
                }
            };

            env = env.insert(name.clone(), cont);
        }
        env
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
        let heap = self.evaluator.global_env.heap();

        // Extract the after thunk (as TaggedValue, matches ContValue::DynamicWindCleanup)
        let after = k
            .captured_cont_bindings
            .iter()
            .find(|(name, _)| name.as_ref() == "__dw_after__")
            .and_then(|(_, cont)| {
                // The after thunk is stored in the body as a Literal (already TaggedValue)
                if let CpsExprKind::Literal(v) = &cont.body.as_ref().kind {
                    Some(*v) // Return TaggedValue directly
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
                if let CpsExprKind::Literal(v) = &cont.body.as_ref().kind {
                    if v.is_fixnum() {
                        Some(v.as_fixnum_unchecked() as u64)
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
        let marker = heap.borrow_mut().intern_symbol("__dynamic_wind_cleanup__");
        let original_cont = k
            .captured_cont_bindings
            .iter()
            .find(|(name, _)| name.as_ref() == "__dw_original__")
            .map(|(_, cont)| {
                // Recursively restore if the original was also special
                let is_dw_cleanup = matches!(
                    &cont.body.as_ref().kind,
                    CpsExprKind::Halt(inner) if matches!(
                        &inner.as_ref().kind,
                        CpsExprKind::Literal(v) if *v == marker
                    )
                );
                if is_dw_cleanup {
                    self.restore_dynamic_wind_cleanup(cont)
                } else {
                    // Regular continuation
                    let restored_cont_env =
                        self.restore_cont_bindings(&cont.captured_cont_bindings);
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

    /// Reify a continuation as a first-class Rc<CpsContinuation>
    pub(super) fn reify_continuation(
        &self,
        cont: &ContValue,
        cont_env: &ContEnv,
        dynamic_winds: &[DynamicWindRecord],
    ) -> Rc<CpsContinuation> {
        let heap = self.evaluator.global_env.heap();

        match cont {
            ContValue::Local {
                param,
                body,
                env,
                cont_env: local_cont_env,
            } => {
                // Capture the continuation environment so it can be restored when invoked
                let captured_bindings =
                    Self::capture_cont_bindings(local_cont_env, dynamic_winds, heap);
                Rc::new(CpsContinuation {
                    body: body.clone(),
                    param: param.clone(),
                    env: env.clone(),
                    prompt_tag: None,
                    dynamic_winds: dynamic_winds.to_vec(),
                    captured_cont_bindings: captured_bindings,
                })
            }
            ContValue::Captured(k) => k.clone(),
            ContValue::Halt => {
                // Halt continuation - create a special marker
                Rc::new(CpsContinuation {
                    body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Var {
                        name: Rc::from("__halt_value__"),
                        scopes: ScopeSet::new(),
                    }))),
                    param: Rc::from("__halt_value__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: vec![],
                    captured_cont_bindings: Self::capture_cont_bindings(
                        cont_env,
                        dynamic_winds,
                        heap,
                    ),
                })
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

                // Create marker symbol
                let marker = heap.borrow_mut().intern_symbol("__dynamic_wind_cleanup__");

                // Create a CpsContinuation that will recreate the DynamicWindCleanup state
                // when invoked. We store the after thunk, wind_id, and original continuation
                // as a special structure.
                Rc::new(CpsContinuation {
                    // Special marker body that indicates this is a DynamicWindCleanup wrapper
                    body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Literal(marker)))),
                    param: Rc::from("__dw_value__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: dynamic_winds.to_vec(),
                    // Store the after thunk, wind_id, and reified original continuation
                    // We serialize these into the captured_cont_bindings with special names
                    captured_cont_bindings: {
                        let mut bindings =
                            Self::capture_cont_bindings(cont_env, dynamic_winds, heap);
                        // Store after thunk as a special binding (already TaggedValue)
                        bindings.push((
                            Rc::from("__dw_after__"),
                            Rc::new(CpsContinuation {
                                body: CpsExpr::rc(CpsExprKind::Literal(*after)), // already TaggedValue
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
                                body: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::fixnum(
                                    *wind_id as i64,
                                ))),
                                param: Rc::from("__unused__"),
                                env: self.evaluator.global_env.clone(),
                                prompt_tag: None,
                                dynamic_winds: vec![],
                                captured_cont_bindings: vec![],
                            }),
                        ));
                        // Store original continuation
                        bindings.push((Rc::from("__dw_original__"), reified_original));
                        bindings
                    },
                })
            }

            // Effect-carrying wrappers reify as the continuation they wrap, by
            // the same rule `capture_cont_bindings` uses. Five of these used to
            // reify as a placeholder that discarded the continuation and
            // returned unspecified.
            other => match other.wrapped_cont() {
                Some(inner) => self.reify_continuation(inner, cont_env, dynamic_winds),
                // Only reachable if a new ContValue variant is added and
                // `wrapped_cont` returns None for it; keep the old placeholder
                // rather than panicking in the evaluator.
                None => Rc::new(CpsContinuation {
                    body: CpsExpr::rc(CpsExprKind::Halt(CpsExpr::rc(CpsExprKind::Literal(
                        TaggedValue::UNSPECIFIED,
                    )))),
                    param: Rc::from("__special__"),
                    env: self.evaluator.global_env.clone(),
                    prompt_tag: None,
                    dynamic_winds: vec![],
                    captured_cont_bindings: Self::capture_cont_bindings(
                        cont_env,
                        dynamic_winds,
                        heap,
                    ),
                }),
            },
        }
    }

    /// Reify a continuation as a TaggedValue, allocating natively on the heap
    pub(super) fn reify_continuation_tagged(
        &self,
        cont: &ContValue,
        cont_env: &ContEnv,
        dynamic_winds: &[DynamicWindRecord],
    ) -> TaggedValue {
        let k = self.reify_continuation(cont, cont_env, dynamic_winds);
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
                // Thunk has returned a value
                let heap = self.evaluator.global_env.heap();

                // Check if result is a promise (delay-force pattern) - need to force recursively
                // Use heap method to check without full conversion
                if heap.borrow().is_promise(value) {
                    // Result is a promise - force it recursively before caching
                    // Create a new ForceCache that will cache the final result
                    let recursive_cont = ContValue::ForceCache {
                        promise,
                        original_cont,
                    };
                    return self.force_promise_cps(
                        value, // Pass TaggedValue directly
                        recursive_cont,
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    );
                }

                // Cache the result (non-promise value)
                // PromiseState now stores TaggedValue directly - no conversion needed
                {
                    let mut state = promise.borrow_mut();
                    *state = patina_core::PromiseState::Forced(value);
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
