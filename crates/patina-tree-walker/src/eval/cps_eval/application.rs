//! Procedure application for CPS evaluation
//!
//! This module contains `apply_cps_step` which handles CPS-mode procedure
//! application, including special handling for CPS-sensitive primitives like
//! `call-with-values`, `force`, `dynamic-wind`, and exception handling.

use super::CpsEvaluator;
use super::types::{
    ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult, set_pending_escape,
};
use crate::eval::error::EvalError;
use patina_core::cps_expr::CpsPrimitive;
use patina_core::tagged_value::TaggedValue;
use patina_core::{DynamicWindRecord, Procedure};
use patina_core::{Environment, ScopedParam};
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Apply a CPS procedure (returns StepResult for trampolining)
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_cps_step(
        &self,
        proc_tagged: TaggedValue,
        args: Vec<TaggedValue>,
        cont: ContValue,
        _env: Rc<Environment>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        let heap = self.evaluator.global_env.heap();

        // Extract all type checks upfront to ensure borrows are released before nested heap access
        // This avoids RefCell borrow conflicts when primitives need to borrow the heap
        let proc_opt = heap.borrow().get_procedure(proc_tagged);
        let cont_opt = heap.borrow().get_continuation(proc_tagged);
        let param_opt = heap.borrow().get_parameter(proc_tagged);

        // Dispatch based on extracted types
        if let Some(p) = proc_opt {
            match p.as_ref() {
                // CPS lambda: evaluate the CPS body with continuation bound
                Procedure::CpsLambda {
                    params,
                    variadic,
                    cont_param,
                    body,
                    env: lambda_env,
                    binding_scopes,
                } => {
                    // Create new environment for the lambda
                    let new_env = Rc::new(Environment::with_parent(lambda_env.clone()));

                    // Check arity - route through exception handlers
                    let min_args = params.len();
                    if variadic.is_none() && args.len() != min_args {
                        return self.maybe_route_error_through_cps(
                            EvalError::WrongArity {
                                expected: min_args.to_string(),
                                actual: args.len(),
                            },
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        );
                    }
                    if args.len() < min_args {
                        return self.maybe_route_error_through_cps(
                            EvalError::WrongArity {
                                expected: format!("at least {}", min_args),
                                actual: args.len(),
                            },
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        );
                    }

                    // One rule for every parameter, fixed or variadic. A
                    // parameter a macro introduced binds at its own scopes,
                    // which already say where it was written. One written in
                    // source binds by name, for the source references that
                    // reach it that way, and at the scopes it stands in, for
                    // the macro-introduced ones that resolve by scope.
                    let bind = |param: &ScopedParam, value: TaggedValue| {
                        if !param.scopes.is_empty() {
                            new_env.define_with_scopes(
                                param.name.to_string(),
                                param.scopes.clone(),
                                value,
                            );
                        } else if binding_scopes.is_empty() {
                            new_env.define(param.name.to_string(), value);
                        } else {
                            // One cell, reachable both ways: by name for the
                            // references written in source, and under the
                            // scopes it stands in for the macro-introduced
                            // ones. Two cells — a `define` *and* a
                            // `define_with_scopes` — is the freeze
                            // `define_scoped_definition`'s own doc warns
                            // about, where a `set!` through one path leaves
                            // the other stale. That is Larceny triage family
                            // 38's obstacle, and this is what removes it.
                            new_env.define_scoped_definition(
                                param.name.to_string(),
                                (**binding_scopes).clone(),
                                value,
                            );
                        }
                    };

                    for (param, arg) in params.iter().zip(args.iter()) {
                        bind(param, *arg);
                    }

                    if let Some(variadic_param) = variadic {
                        // Build rest list directly as TaggedValue (no conversion needed)
                        let heap = new_env.heap();
                        let rest_list = heap
                            .borrow_mut()
                            .list_from_iter(args[params.len()..].iter().copied());
                        bind(variadic_param, rest_list);
                    }

                    // CRITICAL: Start with a FRESH continuation environment for the lambda body!
                    // Only bind the continuation parameter - don't carry over stale continuations
                    // from the caller. The lambda body's let-cont expressions will create new
                    // local continuations as needed.
                    let new_cont_env = ContEnv::new().insert(cont_param.clone(), cont);

                    // Return Continue step instead of recursive call
                    Ok(StepResult::Continue {
                        expr: body.as_ref().clone(),
                        env: new_env,
                        cont_env: new_cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    })
                }

                Procedure::Primitive { name, .. } => {
                    // Handle CPS-sensitive primitives specially
                    // Helper methods now take Vec<TaggedValue> directly
                    match *name {
                        "call-with-values" => self.apply_call_with_values(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        "force" => self.apply_force(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        "dynamic-wind" => self.apply_dynamic_wind(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        "with-exception-handler" => self.apply_with_exception_handler(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        "raise" => self.apply_raise(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                            false,
                        ),

                        "raise-continuable" => self.apply_raise(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                            true,
                        ),

                        "error" => self.apply_error(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        "apply" => self.apply_apply(
                            args,
                            cont,
                            cont_env,
                            prompt_stack,
                            dynamic_winds,
                            exception_handlers,
                        ),

                        _ => {
                            // For other primitives, delegate to direct evaluator
                            self.apply_other_primitive(
                                p.clone(),
                                args,
                                cont,
                                cont_env,
                                prompt_stack,
                                dynamic_winds,
                                exception_handlers,
                            )
                        }
                    }
                }
            }
        } else if let Some(k) = cont_opt {
            // Invoking a captured continuation - non-local control transfer.
            //
            // `(k v)` delivers v; `(k)` and `(k v1 v2 …)` deliver a #<values>
            // object, through the same `values_from` the `values` primitive
            // itself uses, so a `call-with-values` around the capture unpacks
            // them and a plain continuation receives the object. Refusing anything but one
            // argument — which this did until 2026-08-25 — is not an arity
            // rule R7RS has: §6.10 lets a continuation accept as many values
            // as the context it was captured in. SRFI 1 relies on it, and the
            // whole n-ary half of `(scheme list)` was unusable here because
            // `%cars+cdrs` bails out with `(abort '() '())`.
            let val_tagged = heap.borrow_mut().values_from(args);

            // Run dynamic-wind handlers for continuation jump
            // This travels from current winds to the captured winds
            self.run_wind_handlers(&dynamic_winds, &k.dynamic_winds)?;

            // Store escape data and return error to propagate up
            set_pending_escape(val_tagged, k);
            Err(EvalError::ContinuationEscape)
        } else if let Some((values, converter)) = param_opt {
            // Parameters are callable - pass TaggedValue args directly
            self.apply_parameter(
                values,
                converter,
                args,
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            )
        } else {
            // Not a procedure - generate error with type name from heap
            let type_name = heap.borrow().type_name(proc_tagged);
            self.maybe_route_error_through_cps(
                EvalError::NotAProcedure(format!("#<{}>", type_name)),
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            )
        }
    }

    // Helper methods for specific primitive applications

    fn apply_call_with_values(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (call-with-values producer consumer)
        // In CPS: call producer with a continuation that will call consumer
        if args.len() != 2 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "2".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        // Both producer and consumer stay as TaggedValue
        let producer = args[0];
        let consumer = args[1];

        // We create a special "CallWithValuesConsumer" continuation that:
        // 1. Unpacks the values from producer
        // 2. Calls consumer with those values
        // 3. Passes result to the original continuation
        let values_cont = ContValue::CallWithValuesConsumer {
            consumer,
            original_cont: Box::new(cont),
        };

        // Call producer with the values continuation
        Ok(StepResult::ApplyProc {
            proc: producer,
            args: vec![],
            cont: values_cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }

    fn apply_force(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (force promise)
        // In CPS: if promise is forced, return value; else call thunk in CPS mode
        if args.len() != 1 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "1".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        // Pass TaggedValue directly - force_promise_cps handles extraction
        self.force_promise_cps(
            args[0],
            cont,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        )
    }

    fn apply_dynamic_wind(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (dynamic-wind before body after)
        // Sets up handlers to be called when entering/leaving this dynamic extent
        if args.len() != 3 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "3".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        // Keep all thunks as TaggedValue for ApplyProc
        let before = args[0]; // Keep as TaggedValue
        let body = args[1]; // Keep as TaggedValue
        let after = args[2]; // Keep as TaggedValue

        // Create the wind record with TaggedValue thunks directly
        let wind_record = DynamicWindRecord::new(before, after);
        let wind_id = wind_record.id;

        // Create cleanup continuation that will:
        // 1. Pop the wind record
        // 2. Call the after thunk
        // 3. Continue with the original continuation
        let cleanup_cont = ContValue::DynamicWindCleanup {
            after, // TaggedValue
            wind_id,
            original_cont: Box::new(cont),
        };

        // Create a continuation for after the "before" thunk completes
        let setup_cont = ContValue::DynamicWindSetup {
            wind_record,
            body, // TaggedValue
            cleanup_cont: Box::new(cleanup_cont),
        };

        // Call the before thunk - result is ignored
        Ok(StepResult::ApplyProc {
            proc: before,
            args: vec![],
            cont: setup_cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }

    fn apply_with_exception_handler(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (with-exception-handler handler thunk)
        // Installs handler for duration of thunk's dynamic extent
        if args.len() != 2 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "2".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        // Both handler and thunk stay as TaggedValue
        let heap = self.evaluator.global_env.heap();
        let handler = args[0]; // Keep as TaggedValue
        let thunk = args[1]; // Keep as TaggedValue

        // Verify both are procedures. Decide inside the borrow, route outside
        // it: `maybe_route_error_through_cps` allocates the exception object,
        // so it takes `borrow_mut()` and would panic against a live `borrow()`.
        let bad_argument = {
            let heap_ref = heap.borrow();
            if !heap_ref.is_callable(handler) {
                Some("with-exception-handler: first argument must be a procedure")
            } else if !heap_ref.is_callable(thunk) {
                Some("with-exception-handler: second argument must be a procedure")
            } else {
                None
            }
        };
        if let Some(message) = bad_argument {
            return self.maybe_route_error_through_cps(
                EvalError::TypeError(message.to_string()),
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }

        // Create cleanup continuation that pops the handler when thunk completes
        let cleanup_cont = ContValue::ExceptionHandlerCleanup {
            original_cont: Box::new(cont),
        };

        // Push the exception handler onto the stack. It records no wind depth:
        // a raise does not unwind, so there is nothing to unwind *to*.
        let new_handler = ExceptionHandler { handler };
        let mut new_exception_handlers = exception_handlers;
        new_exception_handlers.push(new_handler);

        // Call the thunk with cleanup continuation
        Ok(StepResult::ApplyProc {
            proc: thunk,
            args: vec![],
            cont: cleanup_cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers: new_exception_handlers,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_raise(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
        continuable: bool,
    ) -> Result<StepResult, EvalError> {
        // (raise obj) or (raise-continuable obj)
        if args.len() != 1 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "1".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        // Exception already comes as TaggedValue - no conversion needed!
        let exception_tagged = args[0];

        if let Some(handler_entry) = exception_handlers.last().cloned() {
            // Pop this handler (one-shot semantics for handler invocation)
            let mut new_handlers = exception_handlers;
            new_handlers.pop();

            // The wind stack is left as the raise found it. R7RS 6.11 calls
            // the handler in the dynamic environment of the `raise`, and a
            // raise crosses no dynamic extent, so no after-thunk is due here.
            // The unwind belongs to `guard`, whose `guard-k` jump runs the
            // after-thunks through the ordinary wind machinery (Track L
            // triage families 22 and 28).

            // Create continuation for when handler returns
            let handler_return_cont = ContValue::RaiseHandlerReturn {
                continuable,
                original_exception: if continuable {
                    None
                } else {
                    Some(exception_tagged)
                },
                original_cont: Box::new(cont),
                popped_handler: if continuable {
                    Some(handler_entry.clone())
                } else {
                    None
                },
            };

            // Handler is already TaggedValue - use directly for ApplyProc.proc
            // Call the handler in the raise's own dynamic extent
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception_tagged],
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
            use patina_primitives::primitives::io::datum_writer::format_display_tagged;
            let heap = self.evaluator.global_env.heap();
            let exception_str = format_display_tagged(exception_tagged, heap);
            // One wording for both — see the VM's `vm_raise_value` for why:
            // `guard` re-raises with `raise-continuable`, so "continuable" here
            // described the expansion rather than the user's code.
            let msg = format!("unhandled exception: {}", exception_str);
            Err(EvalError::SchemeException {
                kind: ExceptionKind::Error,
                message: msg,
                irritants_display: String::new(),
            })
        }
    }

    fn apply_error(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (error message obj ...) - Create error object and raise it
        if args.is_empty() {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "at least 1".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }

        // R7RS 6.11 says the message *should* be a string — advice, not a
        // requirement. A non-string is displayed instead of refused; see the
        // `error` primitive in patina-primitives for why. Extracted before
        // anything else takes `borrow_mut()` to allocate the exception.
        let heap = self.evaluator.global_env.heap();
        let message_opt = heap.borrow().get_string_contents(args[0]);
        let message = match message_opt {
            Some(m) => m,
            None => patina_primitives::primitives::io::datum_writer::format_display_tagged(
                args[0], heap,
            ),
        };

        // Remaining arguments are irritants - already TaggedValue!
        let irritants_tagged: Vec<TaggedValue> = args[1..].to_vec();

        // Create exception object directly as TaggedValue
        let exception_tagged = heap.borrow_mut().alloc_exception(
            patina_core::ExceptionKind::Error,
            message.clone(),
            irritants_tagged.clone(),
        );

        // Now do the same as raise (non-continuable)
        if let Some(handler_entry) = exception_handlers.last().cloned() {
            // Pop this handler (one-shot semantics)
            let mut new_handlers = exception_handlers;
            new_handlers.pop();

            // Create continuation for when handler returns
            let handler_return_cont = ContValue::RaiseHandlerReturn {
                continuable: false,
                original_exception: Some(exception_tagged),
                original_cont: Box::new(cont),
                popped_handler: None,
            };

            // Handler is already TaggedValue - use directly for ApplyProc.proc
            // Call handler with exception
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception_tagged],
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
            use patina_primitives::primitives::io::datum_writer::format_display_tagged;
            let irritants_display = irritants_tagged
                .iter()
                .map(|tv| format_display_tagged(*tv, heap))
                .collect::<Vec<_>>()
                .join(" ");
            Err(EvalError::SchemeException {
                kind: ExceptionKind::Error,
                message,
                irritants_display,
            })
        }
    }

    fn apply_apply(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (apply proc arg1 ... args)
        // The last argument must be a list
        if args.len() < 2 {
            let err = EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            };
            return self.maybe_route_error_through_cps(
                err,
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }

        let heap = self.evaluator.global_env.heap();
        let proc = args[0]; // Keep as TaggedValue for ApplyProc.proc
        let last_idx = args.len() - 1;

        // Flatten: take args[1..last] (already TaggedValue) and append the list in args[last]
        let mut flat_args_tagged: Vec<TaggedValue> = args[1..last_idx].to_vec();

        // Append the list - use heap's native methods for TaggedValue pairs
        let last_arg = args[last_idx];
        let mut current = last_arg;

        loop {
            if current.is_null() {
                break;
            }
            // Extract car and cdr using heap helper
            if let Some((car, cdr)) = heap.borrow().try_pair(current) {
                flat_args_tagged.push(car);
                current = cdr;
            } else {
                let err = EvalError::TypeError(format!(
                    "apply: last argument must be a list, got {}",
                    heap.borrow().type_name(last_arg)
                ));
                return self.maybe_route_error_through_cps(
                    err,
                    cont,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                );
            }
        }

        // Now apply the procedure with the flattened arguments
        Ok(StepResult::ApplyProc {
            proc,
            args: flat_args_tagged,
            cont,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_other_primitive(
        &self,
        p: Rc<Procedure>,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // For other primitives, use the primitive registry with TaggedValue args directly.
        // Higher-order primitives like map/for-each should be implemented
        // in Scheme (lib/scheme/base/) for proper CPS compatibility.
        //
        // IMPORTANT: Wrap primitive calls to catch I/O and read errors
        // and route them through the CPS exception handler stack.
        //
        // Extract pre-computed qualified name and index cache from the primitive
        let (qualified_name, registry_index) = match p.as_ref() {
            Procedure::Primitive {
                qualified_name,
                registry_index,
                ..
            } => (qualified_name.as_ref(), registry_index),
            _ => {
                // Unreachable: the only caller matches `Procedure::Primitive`
                // first. `InternalError`, not `TypeError`, because
                // `is_catchable()` treats the latter as a user condition — a
                // future routing sweep would otherwise let Scheme code catch
                // an interpreter bug.
                return Err(EvalError::InternalError(
                    "apply_other_primitive called with non-primitive procedure".to_string(),
                ));
            }
        };

        // Dispatch through the cached registry index — no name hashing. The
        // owned entry point moves `args` straight into higher-order handlers
        // instead of re-copying them at the registry boundary.
        let prim_result = self.evaluator.primitive_registry.apply_cached_owned(
            qualified_name,
            registry_index,
            args,
            self.evaluator,
        );

        match prim_result {
            Ok(result_tagged) => {
                // Return InvokeContinuation step instead of recursive call
                Ok(StepResult::InvokeContinuation {
                    cont,
                    value: result_tagged,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }
            Err(err) => {
                // Check if this error should be routed through CPS handlers
                self.maybe_route_error_through_cps(
                    err,
                    cont,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_parameter(
        &self,
        values: Rc<std::cell::RefCell<Vec<TaggedValue>>>,
        converter: Option<TaggedValue>,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Parameters can be called with 0 or 1 arguments:
        // (param)      => get current value (top of stack)
        // (param val)  => set value (replace top of stack after applying converter)
        let result_tagged = match args.len() {
            0 => {
                // Get current value (top of stack) - already TaggedValue
                let stack = values.borrow();
                *stack.last().ok_or_else(|| {
                    EvalError::InvalidSyntax("parameter stack is empty".to_string())
                })?
            }
            1 => {
                // Set value (replace top of stack after applying converter)
                let new_val = if let Some(conv) = converter {
                    // Apply converter to new value using CPS machinery. The
                    // converter is user code, so anything it raises must reach
                    // the handlers installed *here* — `apply_from_direct_tagged`
                    // runs it on a nested trampoline that starts with an empty
                    // handler stack, so it comes back as a Rust error.
                    match self.apply_from_direct_tagged(conv, vec![args[0]]) {
                        Ok(v) => v,
                        Err(err) => {
                            return self.maybe_route_error_through_cps(
                                err,
                                cont,
                                cont_env,
                                prompt_stack,
                                dynamic_winds,
                                exception_handlers,
                            );
                        }
                    }
                } else {
                    args[0]
                };

                // Set the new value (replace top of stack)
                let mut stack = values.borrow_mut();
                if let Some(top) = stack.last_mut() {
                    *top = new_val;
                }
                TaggedValue::UNSPECIFIED
            }
            _ => {
                return self.maybe_route_error_through_cps(
                    EvalError::WrongArity {
                        expected: "0 or 1".to_string(),
                        actual: args.len(),
                    },
                    cont,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                );
            }
        };

        Ok(StepResult::InvokeContinuation {
            cont,
            value: result_tagged,
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }

    /// Evaluate a CPS primitive operation
    ///
    /// Takes TaggedValue args directly and returns TaggedValue for efficiency.
    pub(super) fn eval_primop(
        &self,
        op: &CpsPrimitive,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, EvalError> {
        let heap = self.evaluator.global_env.heap();

        // Special handling for primitives that don't go through the registry
        match op {
            CpsPrimitive::IsContinuation => {
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                return Ok(TaggedValue::boolean(heap.borrow().is_continuation(args[0])));
            }
            CpsPrimitive::IsPromptTag => {
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                return Ok(TaggedValue::boolean(heap.borrow().is_prompt_tag(args[0])));
            }
            _ => {}
        }

        // Use pre-computed static qualified name — zero allocation
        let qualified_name = op.qualified_name().unwrap();
        self.evaluator
            .primitive_registry
            .apply_tagged(qualified_name, &args, self.evaluator)
    }
}
