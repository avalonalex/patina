//! Single-step CPS evaluation
//!
//! This module contains the core `eval_one_step` function that handles
//! all CpsExpr forms, returning a StepResult for the trampoline loop.

use super::CpsEvaluator;
use super::types::{ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::Environment;
use patina_core::cps_expr::CpsExpr;
use patina_core::value::{DynamicWindRecord, Value};
use std::collections::HashMap;
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
        mut cont_env: HashMap<Rc<str>, ContValue>,
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
            match &current_expr {
                // ==================== Trivial Expressions ====================
                // These evaluate immediately and return Done
                CpsExpr::Literal(v) => {
                    return Ok(StepResult::Done(v.as_ref().clone()));
                }

                CpsExpr::Var { name, scopes } => {
                    match self.lookup_var(name, scopes, &current_env) {
                        Ok(value) => return Ok(StepResult::Done(value)),
                        Err(err) => {
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

                CpsExpr::ContRef(k) => {
                    let cont = cont_env
                        .get(k)
                        .ok_or_else(|| EvalError::UndefinedVariable(k.to_string()))?;
                    return Ok(StepResult::Done(self.reify_continuation(
                        cont,
                        &cont_env,
                        &current_winds,
                    )));
                }

                CpsExpr::Lambda {
                    params,
                    variadic,
                    cont_param,
                    body,
                    binding_scope,
                } => {
                    let closure = self.make_cps_closure(
                        params,
                        variadic.as_ref(),
                        cont_param,
                        body,
                        &current_env,
                        *binding_scope,
                    );
                    return Ok(StepResult::Done(closure));
                }

                // ==================== Expressions that update state and continue ====================
                // These are handled in the inner loop
                CpsExpr::LetVal { name, value, body } => {
                    let val = self.eval_trivial(value, &current_env, &cont_env)?;
                    let new_env = Rc::new(Environment::with_parent(current_env.clone()));
                    new_env.define(name.to_string(), val);
                    current_expr = body.as_ref().clone();
                    current_env = new_env;
                }

                CpsExpr::LetCont {
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
                    cont_env.insert(name.clone(), cont);
                    current_expr = body.as_ref().clone();
                }

                CpsExpr::If {
                    test,
                    consequent,
                    alternate,
                } => {
                    let test_val = self.eval_trivial(test, &current_env, &cont_env)?;
                    let is_true = !matches!(test_val, Value::Boolean(false));
                    current_expr = if is_true {
                        consequent.as_ref().clone()
                    } else {
                        alternate.as_ref().clone()
                    };
                }

                CpsExpr::Set {
                    var,
                    scopes,
                    value,
                    cont,
                } => {
                    let val = self.eval_trivial(value, &current_env, &cont_env)?;
                    self.set_var(var, scopes, val, &current_env)?;
                    current_expr = cont.as_ref().clone();
                }

                CpsExpr::Define { name, value, cont } => {
                    let val = self.eval_trivial(value, &current_env, &cont_env)?;
                    // Define in the "definition environment", not current_env
                    // - For top-level: def_env is global_env
                    // - For lambda body: def_env is the lambda's body environment
                    // This matches direct evaluator behavior where internal defines
                    // go to the lambda's body scope, not to LetVal temporaries
                    def_env.define(name.to_string(), val);
                    current_expr = cont.as_ref().clone();
                }

                CpsExpr::Prompt { tag, body, cont } => {
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
                CpsExpr::App { func, args, cont } => {
                    let proc = self.eval_trivial(func, &current_env, &cont_env)?;
                    let arg_values: Result<Vec<Value>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial(arg, &current_env, &cont_env))
                        .collect();
                    let arg_values = arg_values?;

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

                CpsExpr::Apply { func, args, cont } => {
                    let proc = self.eval_trivial(func, &current_env, &cont_env)?;
                    let arg_values: Result<Vec<Value>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial(arg, &current_env, &cont_env))
                        .collect();
                    let mut arg_values = arg_values?;

                    // Flatten the last argument (must be a list)
                    if let Some(last_arg) = arg_values.pop() {
                        let mut current = last_arg.clone();
                        loop {
                            match current {
                                Value::Null => break,
                                Value::Pair(pair) => {
                                    let borrowed = pair.borrow();
                                    arg_values.push(borrowed.0.clone());
                                    current = borrowed.1.clone();
                                }
                                _ => {
                                    return Err(EvalError::TypeError(format!(
                                        "apply: last argument must be a list, got {:?}",
                                        last_arg
                                    )));
                                }
                            }
                        }
                    }

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

                CpsExpr::Continue { cont, value } => {
                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    // Evaluate value, routing any errors through exception handlers
                    let val = match self.eval_trivial(value, &current_env, &cont_env) {
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
                        value: val,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExpr::CallCC { proc, cont } => {
                    let procedure = self.eval_trivial(proc, &current_env, &cont_env)?;

                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    let captured_k = self.reify_continuation(&k, &cont_env, &current_winds);

                    return Ok(StepResult::ApplyProc {
                        proc: procedure,
                        args: vec![captured_k],
                        cont: k,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: current_winds,
                        exception_handlers,
                    });
                }

                CpsExpr::Control { tag, proc } => {
                    let procedure = self.eval_trivial(proc, &current_env, &cont_env)?;

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

                    let delimited_k = self.make_delimited_continuation(
                        captured_frames,
                        current_winds.clone(),
                        prompt_frame.dynamic_winds.clone(),
                    );

                    return Ok(StepResult::ApplyProc {
                        proc: procedure,
                        args: vec![delimited_k],
                        cont: prompt_cont,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: prompt_frame.dynamic_winds,
                        exception_handlers,
                    });
                }

                CpsExpr::Abort { tag, value } => {
                    let val = self.eval_trivial(value, &current_env, &cont_env)?;

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
                        value: val,
                        env: current_env,
                        cont_env,
                        prompt_stack,
                        dynamic_winds: prompt_frame.dynamic_winds,
                        exception_handlers,
                    });
                }

                CpsExpr::Quasiquote { template, cont } => {
                    // Evaluate quasiquote template
                    let result = super::quasiquote::eval_quasiquote_in_env(
                        self.evaluator,
                        template,
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

                CpsExpr::PrimOp { op, args, cont } => {
                    let arg_values: Result<Vec<Value>, _> = args
                        .iter()
                        .map(|arg| self.eval_trivial(arg, &current_env, &cont_env))
                        .collect();
                    let arg_values = arg_values?;

                    let result = self.eval_primop(op, arg_values)?;

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

                CpsExpr::Halt(value) => {
                    let result = self.eval_trivial(value, &current_env, &cont_env)?;
                    return Ok(StepResult::Done(result));
                }
            }
        }
    }
}
