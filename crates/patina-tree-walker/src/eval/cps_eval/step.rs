//! Single-step CPS evaluation
//!
//! This module contains the core `eval_one_step` function that handles
//! all CpsExpr forms, returning a StepResult for the trampoline loop.

use super::CpsEvaluator;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::DynamicWindRecord;
use patina_core::Environment;
use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::tagged_value::TaggedValue;
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Evaluate a single CPS expression step (non-recursive)
    ///
    /// This is the core evaluation function that handles all CpsExpr forms.
    /// Returns a StepResult indicating either a final value or the next step.
    /// The caller (eval) processes these steps in a trampoline loop.
    pub(super) fn eval_one_step(
        &self,
        expr: &CpsExpr,
        env: Rc<Environment>,
        mut cont_env: ContEnv,
        mut prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Process LetVal/LetCont/If/Set/Define/Prompt in a local loop
        // since they just update state and continue with a new expression
        let mut current_expr = expr.clone();
        let mut current_env = env.clone();
        let current_winds = dynamic_winds;

        // Track the "definition environment" - where `define` should create bindings.
        // This is the environment passed in (from lambda entry or top-level),
        // NOT the environment created by LetVal (which is just for temporaries).
        // - For top-level expressions, this is global_env
        // - For lambda bodies, this is the lambda's body environment
        let def_env = env;

        loop {
            match &current_expr.kind {
                // ==================== Trivial Expressions ====================
                // These evaluate immediately and return Done
                CpsExprKind::Literal(v) => {
                    return Ok(StepResult::Done(*v));
                }

                CpsExprKind::Var { name, scopes } => {
                    match self.lookup_var_tagged(name, scopes, &current_env) {
                        Ok(tagged) => {
                            return Ok(StepResult::Done(tagged));
                        }
                        Err(err) => {
                            // Attach source location from the Var expression, if available
                            let err = err.at_opt(current_expr.source.clone());
                            // Route undefined variable errors through exception handlers
                            // We need a continuation to deliver the error to.
                            // For a bare Var (not in an App), we use a Halt continuation
                            // so the error can be caught by guard/with-exception-handler.
                            let halt_cont = ContValue::Halt;
                            return self.maybe_route_error_through_cps(
                                err,
                                halt_cont,
                                cont_env,
                                prompt_stack,
                                current_winds,
                                exception_handlers,
                            );
                        }
                    }
                }

                CpsExprKind::ContRef(k) => {
                    let cont = cont_env
                        .get(k)
                        .ok_or_else(|| EvalError::UndefinedVariable(k.to_string()))?;
                    let tagged = self.reify_continuation_tagged(cont, &cont_env, &current_winds);
                    return Ok(StepResult::Done(tagged));
                }

                CpsExprKind::Lambda {
                    params,
                    variadic,
                    cont_param,
                    body,
                    binding_scope,
                } => {
                    let tagged = self.make_cps_closure_tagged(
                        params,
                        variadic.as_ref(),
                        cont_param,
                        body,
                        &current_env,
                        *binding_scope,
                    );
                    return Ok(StepResult::Done(tagged));
                }

                // ==================== Expressions that update state and continue ====================
                // These are handled in the inner loop
                CpsExprKind::LetVal { name, value, body } => {
                    // Same routing as the `Var` and `App` arms: the CPS transform
                    // binds an application's operator and operands through
                    // `LetVal`, so this is where `(undefined-fn)`'s lookup
                    // actually fails, and propagating here is what let it escape
                    // `guard`.
                    let val = match self.eval_trivial_tagged(value, &current_env, &cont_env) {
                        Ok(v) => v,
                        Err(e) => {
                            return self.maybe_route_error_through_cps(
                                e.at_opt(current_expr.source.clone()),
                                ContValue::Halt,
                                cont_env,
                                prompt_stack,
                                current_winds,
                                exception_handlers,
                            );
                        }
                    };
                    let new_env = Rc::new(Environment::with_parent(current_env.clone()));
                    new_env.define(name.to_string(), val);
                    current_expr = body.as_ref().clone();
                    current_env = new_env;
                }

                CpsExprKind::LetCont {
                    name,
                    param,
                    cont_body,
                    body,
                } => {
                    let cont = ContValue::Local {
                        param: param.clone(),
                        body: cont_body.clone(),
                        env: current_env.clone(),
                        cont_env: cont_env.clone(),
                    };
                    cont_env = cont_env.insert(name.clone(), cont);
                    current_expr = body.as_ref().clone();
                }

                CpsExprKind::If {
                    test,
                    consequent,
                    alternate,
                } => {
                    let test_val = self.eval_trivial_tagged(test, &current_env, &cont_env)?;
                    current_expr = if test_val.is_truthy() {
                        consequent.as_ref().clone()
                    } else {
                        alternate.as_ref().clone()
                    };
                }

                CpsExprKind::Set {
                    var,
                    scopes,
                    value,
                    cont,
                } => {
                    let val = self.eval_trivial_tagged(value, &current_env, &cont_env)?;
                    self.set_var_tagged(var, scopes, val, &current_env)?;
                    current_expr = cont.as_ref().clone();
                }

                CpsExprKind::Define { name, value, cont } => {
                    let val = self.eval_trivial_tagged(value, &current_env, &cont_env)?;
                    // Define in the "definition environment", not current_env
                    // - For top-level: def_env is global_env
                    // - For lambda body: def_env is the lambda's body environment
                    // This matches direct evaluator behavior where internal defines
                    // go to the lambda's body scope, not to LetVal temporaries
                    def_env.define(name.to_string(), val);
                    current_expr = cont.as_ref().clone();
                }

                CpsExprKind::Prompt { tag, body, cont } => {
                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    prompt_stack.push(PromptFrame {
                        tag: Rc::new(tag.clone()),
                        cont: k,
                        dynamic_winds: current_winds.clone(),
                    });

                    current_expr = body.as_ref().clone();
                }

                // Note: CpsExpr::Parameterize has been removed.
                // Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)

                // ==================== Expressions that return StepResult ====================
                // These require trampolining to avoid stack growth
                CpsExprKind::App { func, args, cont } => {
                    // Evaluate func to TaggedValue directly for ApplyProc.proc
                    // Attach call-site source to any lookup errors (e.g. undefined function)
                    let call_source = current_expr.source.clone();
                    // Route through the handlers rather than propagating as a Rust
                    // error, exactly as the `Var` arm above does. Without this the
                    // operator position was the one place an unbound variable
                    // escaped `guard`: `undefined-var` was catchable but
                    // `(undefined-fn)` was not, on the tree-walker only. chibi,
                    // Gauche and Chez all catch both, and so does the VM.
                    let proc = match self.eval_trivial_tagged(func, &current_env, &cont_env) {
                        Ok(p) => p,
                        Err(e) => {
                            return self.maybe_route_error_through_cps(
                                e.at_opt(call_source.clone()),
                                ContValue::Halt,
                                cont_env,
                                prompt_stack,
                                current_winds,
                                exception_handlers,
                            );
                        }
                    };
                    let arg_values: Result<Vec<TaggedValue>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial_tagged(arg, &current_env, &cont_env))
                        .collect();
                    let arg_values = match arg_values {
                        Ok(v) => v,
                        Err(e) => {
                            return self.maybe_route_error_through_cps(
                                e.at_opt(call_source.clone()),
                                ContValue::Halt,
                                cont_env,
                                prompt_stack,
                                current_winds,
                                exception_handlers,
                            );
                        }
                    };

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    return Ok(StepResult::ApplyProc {
                        proc,
                        args: arg_values,
                        cont: k,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Apply { func, args, cont } => {
                    // Evaluate func to TaggedValue directly for ApplyProc.proc
                    let proc = self.eval_trivial_tagged(func, &current_env, &cont_env)?;
                    let heap = self.evaluator.global_env.heap();

                    // Evaluate args to TaggedValue and flatten the last list
                    let arg_tagged: Result<Vec<TaggedValue>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial_tagged(arg, &current_env, &cont_env))
                        .collect();
                    let mut arg_tagged = arg_tagged?;

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    // Flatten the last argument (must be a list)
                    let flat_args: Vec<TaggedValue> = if !arg_tagged.is_empty() {
                        let last_arg = arg_tagged.pop().unwrap();
                        let mut result: Vec<TaggedValue> = arg_tagged;
                        let mut current = last_arg;

                        loop {
                            if current.is_null() {
                                break;
                            }
                            // Extract car and cdr using heap helper
                            if let Some((car, cdr)) = heap.borrow().try_pair(current) {
                                result.push(car);
                                current = cdr;
                            } else {
                                let err = EvalError::TypeError(format!(
                                    "apply: last argument must be a list, got {}",
                                    heap.borrow().type_name(last_arg)
                                ))
                                .at_opt(current_expr.source.clone());
                                return self.maybe_route_error_through_cps(
                                    err,
                                    k,
                                    cont_env,
                                    prompt_stack,
                                    current_winds,
                                    exception_handlers,
                                );
                            }
                        }
                        result
                    } else {
                        vec![]
                    };

                    return Ok(StepResult::ApplyProc {
                        proc,
                        args: flat_args,
                        cont: k,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Continue { cont, value } => {
                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    // Evaluate value, routing any errors through exception handlers
                    let val_tagged = match self.eval_trivial_tagged(value, &current_env, &cont_env)
                    {
                        Ok(v) => v,
                        Err(err) => {
                            return self.maybe_route_error_through_cps(
                                err,
                                k,
                                cont_env,
                                prompt_stack,
                                current_winds,
                                exception_handlers,
                            );
                        }
                    };

                    return Ok(StepResult::InvokeContinuation {
                        cont: k,
                        value: val_tagged,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::CallCC { proc, cont } => {
                    // Evaluate proc to TaggedValue directly for ApplyProc.proc
                    let procedure = self.eval_trivial_tagged(proc, &current_env, &cont_env)?;

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    let captured_k_tagged =
                        self.reify_continuation_tagged(&k, &cont_env, &current_winds);

                    return Ok(StepResult::ApplyProc {
                        proc: procedure,
                        args: vec![captured_k_tagged],
                        cont: k,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Control { tag, proc } => {
                    // Evaluate proc to TaggedValue directly for ApplyProc.proc
                    let procedure = self.eval_trivial_tagged(proc, &current_env, &cont_env)?;

                    let prompt_idx = prompt_stack
                        .iter()
                        .rposition(|frame| frame.tag.as_ref() == tag)
                        .ok_or_else(|| {
                            EvalError::InternalError(format!("No prompt found for tag: {}", tag))
                        })?;

                    let captured_frames: Vec<PromptFrame> =
                        prompt_stack.drain(prompt_idx + 1..).collect();

                    let prompt_frame = prompt_stack.pop().unwrap();
                    let prompt_cont = prompt_frame.cont;

                    let delimited_k_tagged = self.make_delimited_continuation_tagged(
                        captured_frames,
                        current_winds.clone(),
                        prompt_frame.dynamic_winds.clone(),
                    );

                    return Ok(StepResult::ApplyProc {
                        proc: procedure,
                        args: vec![delimited_k_tagged],
                        cont: prompt_cont,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: prompt_frame.dynamic_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Abort { tag, value } => {
                    let val_tagged = self.eval_trivial_tagged(value, &current_env, &cont_env)?;

                    let prompt_idx = prompt_stack
                        .iter()
                        .rposition(|frame| frame.tag.as_ref() == tag)
                        .ok_or_else(|| {
                            EvalError::InternalError(format!("No prompt found for tag: {}", tag))
                        })?;

                    prompt_stack.truncate(prompt_idx);
                    let prompt_frame = prompt_stack.pop().unwrap();

                    self.run_wind_handlers(&current_winds, &prompt_frame.dynamic_winds)?;

                    return Ok(StepResult::InvokeContinuation {
                        cont: prompt_frame.cont,
                        value: val_tagged,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: prompt_frame.dynamic_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Quasiquote { template, cont } => {
                    // Evaluate quasiquote template - now works with TaggedValue directly
                    let result = super::quasiquote::eval_quasiquote_in_env(
                        self.evaluator,
                        *template,
                        &current_env,
                    )?;

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    return Ok(StepResult::InvokeContinuation {
                        cont: k,
                        value: result,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::PrimOp { op, args, cont } => {
                    // Evaluate args directly to TaggedValue
                    let arg_values: Result<Vec<TaggedValue>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial_tagged(arg, &current_env, &cont_env))
                        .collect();
                    let arg_values = arg_values?;

                    // eval_primop now takes and returns TaggedValue
                    let result_tagged = self.eval_primop(op, arg_values)?;

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    return Ok(StepResult::InvokeContinuation {
                        cont: k,
                        value: result_tagged,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExprKind::Halt(value) => {
                    let tagged = self.eval_trivial_tagged(value, &current_env, &cont_env)?;
                    return Ok(StepResult::Done(tagged));
                }
            }
        }
    }
}
