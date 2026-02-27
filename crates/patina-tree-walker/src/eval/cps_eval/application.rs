//! Procedure application for CPS evaluation
//!
//! This module contains `apply_cps_step` which handles CPS-mode procedure
//! application, including special handling for CPS-sensitive primitives like
//! `call-with-values`, `force`, `dynamic-wind`, and exception handling.

use super::CpsEvaluator;
use super::types::{ContValue, ExceptionHandler, PromptFrame, StepResult, set_pending_escape};
use crate::eval::error::EvalError;
use patina_core::cps_expr::CpsPrimitive;
use patina_core::tagged_value::TaggedValue;
use patina_core::{DynamicWindRecord, Procedure};
use patina_core::{Environment, ScopeSet};
use std::collections::HashMap;
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        let heap = self.evaluator.global_env.heap();

        // Helper closure to route catchable errors through exception handlers
        let route_error = |err: EvalError,
                           cont: ContValue,
                           cont_env: HashMap<Rc<str>, ContValue>,
                           prompt_stack: Vec<PromptFrame>,
                           dynamic_winds: Vec<DynamicWindRecord>,
                           exception_handlers: Vec<ExceptionHandler>| {
            self.maybe_route_error_through_cps(
                err,
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            )
        };

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
                    binding_scope,
                } => {
                    // Create new environment for the lambda
                    let new_env = Rc::new(Environment::with_parent(lambda_env.clone()));

                    // Check arity - route through exception handlers
                    let min_args = params.len();
                    if variadic.is_none() && args.len() != min_args {
                        return route_error(
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
                        return route_error(
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

                    // Bind fixed parameters with proper hygiene support
                    for (param, arg) in params.iter().zip(args.iter()) {
                        if !param.scopes.is_empty() {
                            // Macro-introduced parameter: use its explicit scopes
                            new_env.define_with_scopes(
                                param.name.to_string(),
                                param.scopes.clone(),
                                *arg,
                            );
                        } else {
                            // Non-macro parameter: simple binding + scoped binding if binding_scope present
                            new_env.define(param.name.to_string(), *arg);
                            // Also add scoped binding so macro-expanded refs can find it
                            if let Some(scope) = binding_scope {
                                let mut scopes = ScopeSet::new();
                                scopes.add_scope(*scope);
                                new_env.define_with_scopes(param.name.to_string(), scopes, *arg);
                            }
                        }
                    }

                    // Bind variadic parameter if present
                    if let Some(variadic_param) = variadic {
                        // Build rest list directly as TaggedValue (no conversion needed)
                        let heap = new_env.heap();
                        let rest_list = {
                            let mut heap_mut = heap.borrow_mut();
                            args[params.len()..]
                                .iter()
                                .rev()
                                .fold(TaggedValue::NULL, |acc, &arg| heap_mut.alloc_pair(arg, acc))
                        };
                        if !variadic_param.scopes.is_empty() {
                            new_env.define_with_scopes(
                                variadic_param.name.to_string(),
                                variadic_param.scopes.clone(),
                                rest_list,
                            );
                        } else {
                            new_env.define(variadic_param.name.to_string(), rest_list);
                            // Also add scoped binding so macro-expanded refs can find it
                            if let Some(scope) = binding_scope {
                                let mut scopes = ScopeSet::new();
                                scopes.add_scope(*scope);
                                new_env.define_with_scopes(
                                    variadic_param.name.to_string(),
                                    scopes,
                                    rest_list,
                                );
                            }
                        }
                    }

                    // CRITICAL: Start with a FRESH continuation environment for the lambda body!
                    // Only bind the continuation parameter - don't carry over stale continuations
                    // from the caller. The lambda body's let-cont expressions will create new
                    // local continuations as needed.
                    let mut new_cont_env = HashMap::new();
                    new_cont_env.insert(cont_param.clone(), cont);

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
            // Invoking a captured continuation - non-local control transfer
            if args.len() != 1 {
                return Err(EvalError::WrongArity {
                    expected: "1".to_string(),
                    actual: args.len(),
                });
            }

            // Get the single argument as TaggedValue directly
            let val_tagged = args.into_iter().next().unwrap();

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
            route_error(
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (call-with-values producer consumer)
        // In CPS: call producer with a continuation that will call consumer
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (force promise)
        // In CPS: if promise is forced, return value; else call thunk in CPS mode
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (dynamic-wind before body after)
        // Sets up handlers to be called when entering/leaving this dynamic extent
        if args.len() != 3 {
            return Err(EvalError::WrongArity {
                expected: "3".to_string(),
                actual: args.len(),
            });
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (with-exception-handler handler thunk)
        // Installs handler for duration of thunk's dynamic extent
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        // Both handler and thunk stay as TaggedValue
        let heap = self.evaluator.global_env.heap();
        let handler = args[0]; // Keep as TaggedValue
        let thunk = args[1]; // Keep as TaggedValue

        // Verify both are procedures (use heap type-checking methods directly)
        {
            let heap_ref = heap.borrow();
            if !heap_ref.is_procedure(handler) && !heap_ref.is_continuation(handler) {
                return Err(EvalError::TypeError(
                    "with-exception-handler: first argument must be a procedure".to_string(),
                ));
            }
            if !heap_ref.is_procedure(thunk) && !heap_ref.is_continuation(thunk) {
                return Err(EvalError::TypeError(
                    "with-exception-handler: second argument must be a procedure".to_string(),
                ));
            }
        }

        // Create cleanup continuation that pops the handler when thunk completes
        let cleanup_cont = ContValue::ExceptionHandlerCleanup {
            original_cont: Box::new(cont),
        };

        // Push the exception handler onto the stack
        let new_handler = ExceptionHandler { handler }; // TaggedValue
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
        continuable: bool,
    ) -> Result<StepResult, EvalError> {
        // (raise obj) or (raise-continuable obj)
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        // Exception already comes as TaggedValue - no conversion needed!
        let exception_tagged = args[0];

        if let Some(handler_entry) = exception_handlers.last().cloned() {
            // Pop this handler (one-shot semantics)
            let mut new_handlers = exception_handlers;
            new_handlers.pop();

            // Create continuation for when handler returns
            let handler_return_cont = ContValue::RaiseHandlerReturn {
                continuable,
                original_exception: if continuable {
                    None
                } else {
                    Some(exception_tagged)
                },
                original_cont: Box::new(cont),
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
            use crate::eval::primitives::io::datum_writer::format_display_tagged;
            use patina_core::ExceptionKind;
            let heap = self.evaluator.global_env.heap();
            let exception_str = format_display_tagged(exception_tagged, heap);
            let msg = if continuable {
                format!("unhandled continuable exception: {}", exception_str)
            } else {
                format!("unhandled exception: {}", exception_str)
            };
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (error message obj ...) - Create error object and raise it
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: args.len(),
            });
        }

        // First argument must be a string (the message)
        let heap = self.evaluator.global_env.heap();
        let message = heap.borrow().get_string_contents(args[0]).ok_or_else(|| {
            EvalError::TypeError("error: first argument must be a string".to_string())
        })?;

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
            use crate::eval::primitives::io::datum_writer::format_display_tagged;
            use patina_core::ExceptionKind;
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
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // (apply proc arg1 ... args)
        // The last argument must be a list
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
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
                return Err(EvalError::TypeError(format!(
                    "apply: expected list as last argument, got {}",
                    heap.borrow().type_name(last_arg)
                )));
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
        cont_env: HashMap<Rc<str>, ContValue>,
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
        // Extract the qualified name from the primitive procedure
        let (name, library) = match p.as_ref() {
            Procedure::Primitive { name, library, .. } => (*name, library),
            _ => {
                return Err(EvalError::TypeError(
                    "apply_other_primitive called with non-primitive procedure".to_string(),
                ));
            }
        };
        let qualified_name = format!("{}/{}", library.join("."), name);

        // Call the primitive using the registry's apply_tagged method
        let prim_result = self.evaluator.primitive_registry.apply_tagged(
            &qualified_name,
            args,
            self.evaluator,
            false,
        );

        match prim_result {
            Ok(eval_result) => {
                let result_tagged = match eval_result {
                    super::super::EvalResult::Tagged(tv) => tv,
                    super::super::EvalResult::TailCallPrimitive { .. } => {
                        return Err(EvalError::InternalError(
                            "Primitive returned tail call".to_string(),
                        ));
                    }
                };

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
        cont_env: HashMap<Rc<str>, ContValue>,
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
                    // Apply converter to new value using CPS machinery
                    self.apply_from_direct_tagged(conv, vec![args[0]])?
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
                return Err(EvalError::WrongArity {
                    expected: "0 or 1".to_string(),
                    actual: args.len(),
                });
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
        // Map CpsPrimitive to the corresponding primitive name and delegate
        let name = match op {
            CpsPrimitive::Add => "+",
            CpsPrimitive::Sub => "-",
            CpsPrimitive::Mul => "*",
            CpsPrimitive::Div => "/",
            CpsPrimitive::Quotient => "quotient",
            CpsPrimitive::Remainder => "remainder",
            CpsPrimitive::Modulo => "modulo",
            CpsPrimitive::NumEq => "=",
            CpsPrimitive::Lt => "<",
            CpsPrimitive::Gt => ">",
            CpsPrimitive::Lte => "<=",
            CpsPrimitive::Gte => ">=",
            CpsPrimitive::Cons => "cons",
            CpsPrimitive::Car => "car",
            CpsPrimitive::Cdr => "cdr",
            CpsPrimitive::List => "list",
            CpsPrimitive::IsNull => "null?",
            CpsPrimitive::IsPair => "pair?",
            CpsPrimitive::IsNumber => "number?",
            CpsPrimitive::IsBoolean => "boolean?",
            CpsPrimitive::IsString => "string?",
            CpsPrimitive::IsSymbol => "symbol?",
            CpsPrimitive::IsProcedure => "procedure?",
            CpsPrimitive::IsContinuation => "continuation?",
            CpsPrimitive::IsPromptTag => "prompt-tag?",
            CpsPrimitive::Eq => "eq?",
            CpsPrimitive::Eqv => "eqv?",
            CpsPrimitive::Equal => "equal?",
            CpsPrimitive::MakeVector => "make-vector",
            CpsPrimitive::VectorRef => "vector-ref",
            CpsPrimitive::VectorSet => "vector-set!",
            CpsPrimitive::VectorLength => "vector-length",
            CpsPrimitive::MakeString => "make-string",
            CpsPrimitive::StringRef => "string-ref",
            CpsPrimitive::StringLength => "string-length",
            CpsPrimitive::Display => "display",
            CpsPrimitive::Newline => "newline",
        };

        let heap = self.evaluator.global_env.heap();

        // Special handling for primitives that may not exist in the global env
        match op {
            CpsPrimitive::IsContinuation => {
                // continuation? is a new predicate
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                // Use heap's is_continuation directly - no conversion needed
                return Ok(TaggedValue::boolean(heap.borrow().is_continuation(args[0])));
            }
            CpsPrimitive::IsPromptTag => {
                // prompt-tag? is a new predicate
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                // Use heap's is_prompt_tag directly - no conversion needed
                return Ok(TaggedValue::boolean(heap.borrow().is_prompt_tag(args[0])));
            }
            _ => {}
        }

        // CPS primitives are all from scheme.base
        let qualified_name = format!("scheme.base/{}", name);
        let prim_result = self.evaluator.primitive_registry.apply_tagged(
            &qualified_name,
            args,
            self.evaluator,
            false,
        );

        match prim_result {
            Ok(eval_result) => match eval_result {
                super::super::EvalResult::Tagged(tv) => Ok(tv),
                super::super::EvalResult::TailCallPrimitive { .. } => {
                    Err(EvalError::InternalError(format!(
                        "Primitive {} returned unexpected result",
                        name
                    )))
                }
            },
            Err(e) => Err(e),
        }
    }
}
