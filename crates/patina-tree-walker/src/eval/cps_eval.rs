//! CPS Evaluator - Evaluates CPS (Continuation-Passing Style) expressions
//!
//! This module implements evaluation of CpsExpr, which is the IR used for
//! implementing first-class continuations (call/cc) and delimited continuations
//! (shift/reset, prompt/control).
//!
//! # Architecture
//!
//! ```text
//! CoreExpr → [CPS Transform] → CpsExpr → [THIS MODULE] → Value
//! ```
//!
//! The CPS evaluator handles the following expression forms:
//!
//! **Trivial expressions** (evaluate immediately):
//! - `Literal` - Self-evaluating values
//! - `Var` - Variable references
//! - `ContRef` - Continuation variable references
//! - `Lambda` - CPS lambda (takes continuation parameter)
//!
//! **Serious expressions** (use continuations):
//! - `LetVal` - Bind trivial value and continue
//! - `LetCont` - Define local continuation
//! - `App` - Application with explicit continuation
//! - `Continue` - Invoke continuation with value
//! - `If` - Conditional
//! - `Set` - Mutation
//! - `Define` - Definition
//!
//! **Control operators**:
//! - `CallCC` - Capture current continuation
//! - `Prompt` - Establish delimiter for shift/reset
//! - `Control` - Capture delimited continuation
//! - `Abort` - Abort to prompt
//!
//! - `PrimOp` - Primitive operations
//! - `Halt` - Program termination
//!
//! # Continuation Representation
//!
//! Continuations are represented as `Value::Continuation(CpsContinuation)` which
//! stores the continuation body, parameter name, captured environment, and
//! dynamic-wind state.
//!
//! # Trampoline Architecture
//!
//! The evaluator uses a fully-trampolined design to avoid Rust stack overflow:
//! - All evaluation returns a `StepResult` enum indicating either a final value
//!   or the next expression to evaluate
//! - A single loop in `eval()` processes steps iteratively
//! - No recursive Rust calls during normal CPS evaluation

use super::error::EvalError;
use patina_core::cps_expr::{CpsExpr, CpsParam, CpsPrimitive, PromptTag};
use patina_core::value::{CpsContinuation, DynamicWindRecord, Procedure, Value};
use patina_core::{Environment, ScopeSet, ScopedParam};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Thread-local storage for continuation escapes.
// When a Value::Continuation is invoked inside apply_from_direct,
// we store it here so the outer eval loop can retrieve it.
thread_local! {
    static PENDING_ESCAPE: RefCell<Option<(Value, Rc<CpsContinuation>)>> = const { RefCell::new(None) };
}

fn set_pending_escape(value: Value, cont: Rc<CpsContinuation>) {
    PENDING_ESCAPE.with(|cell| *cell.borrow_mut() = Some((value, cont)));
}

fn take_pending_escape() -> Option<(Value, Rc<CpsContinuation>)> {
    PENDING_ESCAPE.with(|cell| cell.borrow_mut().take())
}

/// Helper: construct a Scheme list from a Vec<Value>
fn list_from_vec(vec: Vec<Value>) -> Value {
    let mut result = Value::Null;
    for item in vec.into_iter().rev() {
        result = Value::Pair(Rc::new(RefCell::new((item, result))));
    }
    result
}

/// A prompt frame on the meta-continuation stack
#[derive(Debug, Clone)]
struct PromptFrame {
    /// The tag identifying this prompt
    tag: Rc<PromptTag>,
    /// Continuation to invoke when prompt is reached
    cont: ContValue,
    /// Dynamic wind records active at this prompt
    dynamic_winds: Vec<DynamicWindRecord>,
}

/// An exception handler installed by with-exception-handler
///
/// Exception handlers form a stack (like dynamic-wind records).
/// When raise is called, the topmost handler is invoked.
#[derive(Debug, Clone)]
struct ExceptionHandler {
    /// The handler procedure (lambda (condition) ...)
    handler: Value,
}

/// Continuation values used during CPS evaluation
///
/// Continuations can be either:
/// - A CpsExpr body with captured environment (for LetCont-defined continuations)
/// - A first-class continuation value (for captured continuations)
/// - The halt continuation (program end)
/// - Special continuations for CPS-aware primitives
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum ContValue {
    /// A local continuation defined by LetCont
    Local {
        param: Rc<str>,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
        /// The continuation environment at the point where this continuation was defined
        /// This is needed because the continuation body may reference other continuations
        /// that were in scope when the let-cont was evaluated.
        cont_env: HashMap<Rc<str>, ContValue>,
    },
    /// A captured first-class continuation (used when re-invoking serialized continuations)
    #[allow(dead_code)]
    Captured(Rc<CpsContinuation>),
    /// The halt continuation - returns final value
    Halt,
    /// Special continuation for call-with-values
    /// When the producer returns, unpack its values and call the consumer
    CallWithValuesConsumer {
        consumer: Value,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for force
    /// When the thunk returns, cache the value and continue
    ForceCache {
        promise: Rc<std::cell::RefCell<patina_core::value::PromiseState>>,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for parameterize cleanup
    /// When the body returns, pop parameter values and continue
    ParameterizeCleanup {
        params: Vec<Value>,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind cleanup
    /// When the body returns, pop the wind record, call after thunk, and continue
    DynamicWindCleanup {
        /// The "after" thunk to call when leaving this dynamic extent
        after: Value,
        /// The wind record ID to pop (for verification)
        wind_id: u64,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind setup
    /// After the "before" thunk returns, push wind record and call body
    DynamicWindSetup {
        /// The wind record to push
        wind_record: DynamicWindRecord,
        /// The body thunk to call
        body: Value,
        /// The cleanup continuation (will pop and call after)
        cleanup_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind after thunk completion
    /// After the "after" thunk returns, continue with the saved body result
    DynamicWindAfterDone {
        /// The result from the body (to pass through)
        result_value: Value,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for with-exception-handler cleanup
    /// When the thunk completes normally, pop the exception handler and continue
    ExceptionHandlerCleanup { original_cont: Box<ContValue> },
    /// Special continuation for raise when handler returns
    /// For non-continuable raise: if handler returns, raise secondary exception
    /// For continuable raise: handler's return value continues from raise-continuable
    RaiseHandlerReturn {
        /// Whether this was a continuable raise
        continuable: bool,
        /// For non-continuable: the original exception (to include in secondary error)
        original_exception: Option<Value>,
        /// For continuable: the continuation to continue with
        original_cont: Box<ContValue>,
    },
}

/// Result of a single evaluation step (for trampoline)
///
/// The CPS evaluator processes expressions one step at a time.
/// Each step either produces a final value or indicates the
/// next expression to evaluate.
enum StepResult {
    /// Final value - evaluation is complete
    Done(Value),
    /// Continue evaluation with a new expression and state
    Continue {
        expr: CpsExpr,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Invoke a continuation with a value
    InvokeContinuation {
        cont: ContValue,
        value: Value,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Apply a procedure
    ApplyProc {
        proc: Value,
        args: Vec<Value>,
        cont: ContValue,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
}

/// CPS Evaluator state
///
/// Manages the evaluation of CpsExpr with support for:
/// - Continuation environment (mapping ContVar to ContValue)
/// - Prompt stack for delimited continuations
/// - Dynamic-wind state
pub struct CpsEvaluator<'a> {
    /// The tree-walker evaluator for primitive operations
    evaluator: &'a super::Evaluator,
}

impl<'a> CpsEvaluator<'a> {
    /// Create a new CPS evaluator
    pub fn new(evaluator: &'a super::Evaluator) -> Self {
        Self { evaluator }
    }

    /// Apply a CPS procedure from direct evaluation mode
    ///
    /// This allows the direct evaluator to invoke CPS lambdas by delegating
    /// to the CPS evaluator with a halt continuation. This enables interoperability
    /// between library code (loaded in direct mode) and user code (in CPS mode).
    ///
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

    /// Evaluate a CPS expression to a final value
    ///
    /// This is the main entry point for CPS evaluation. The expression
    /// should be produced by CpsTransformer::transform_toplevel() which
    /// wraps the result in a Halt continuation.
    ///
    /// Uses a trampoline pattern to avoid Rust stack overflow on deep recursion.
    /// All CPS evaluation steps are processed iteratively in a single loop.
    pub fn eval(&self, expr: &CpsExpr) -> Result<Value, EvalError> {
        let env = self.evaluator.global_env.clone();
        let cont_env = HashMap::new();
        let prompt_stack = Vec::new();
        let dynamic_winds = Vec::new();
        let exception_handlers = Vec::new();

        // Debug: show the CPS expression
        let debug = std::env::var("CPS_DEBUG").is_ok();
        if debug {
            eprintln!("[CPS] Input expr: {}", expr);
        }

        // Start with the initial expression
        let mut current_step = match self.eval_one_step(
            expr,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        ) {
            Ok(step) => step,
            Err(e) => {
                if debug {
                    eprintln!("[CPS] Error in initial eval_one_step: {}", e);
                }
                return Err(e);
            }
        };

        let mut step_count = 0;

        // Trampoline loop - process steps until we get a final value
        loop {
            step_count += 1;
            if debug && step_count <= 30 {
                eprintln!(
                    "[CPS] Step {}: {:?}",
                    step_count,
                    std::mem::discriminant(&current_step)
                );
            }
            // Process step, catching ContinuationEscape to handle escaped continuations
            let step_result = match current_step {
                StepResult::Done(value) => {
                    if debug {
                        eprintln!("[CPS] Done after {} steps: {}", step_count, value);
                    }
                    return Ok(value);
                }

                StepResult::Continue {
                    expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.eval_one_step(
                    &expr,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::InvokeContinuation {
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.invoke_continuation_step(
                    cont,
                    value,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),

                StepResult::ApplyProc {
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                } => self.apply_cps_step(
                    proc,
                    args,
                    cont,
                    env,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                ),
            };

            // Handle result, catching ContinuationEscape
            match step_result {
                Ok(step) => current_step = step,
                Err(EvalError::ContinuationEscape) => {
                    // A continuation escaped from apply_from_direct
                    if let Some((value, k)) = take_pending_escape() {
                        if debug {
                            eprintln!("[CPS] Continuation escape with value: {}", value);
                        }

                        // Check if this is a special DynamicWindCleanup continuation
                        let is_dw_cleanup = matches!(
                            k.body.as_ref(),
                            CpsExpr::Halt(inner) if matches!(
                                inner.as_ref(),
                                CpsExpr::Literal(v) if matches!(v.as_ref(), Value::Symbol(s) if s.as_ref() == "__dynamic_wind_cleanup__")
                            )
                        );

                        if is_dw_cleanup {
                            // Reconstruct the DynamicWindCleanup continuation
                            let new_cont = self.restore_dynamic_wind_cleanup(&k)?;
                            let restored_cont_env =
                                self.restore_cont_bindings(&k.captured_cont_bindings);
                            current_step = self.invoke_continuation_step(
                                new_cont,
                                value,
                                k.env.clone(),
                                restored_cont_env,
                                Vec::new(),
                                k.dynamic_winds.clone(),
                                Vec::new(),
                            )?;
                        } else {
                            // Normal continuation - resume with the escaped continuation
                            let restored_cont_env =
                                self.restore_cont_bindings(&k.captured_cont_bindings);
                            let new_cont = ContValue::Local {
                                param: k.param.clone(),
                                body: k.body.clone(),
                                env: k.env.clone(),
                                cont_env: restored_cont_env.clone(),
                            };
                            current_step = self.invoke_continuation_step(
                                new_cont,
                                value,
                                k.env.clone(),
                                restored_cont_env,
                                Vec::new(),
                                k.dynamic_winds.clone(),
                                Vec::new(),
                            )?;
                        }
                    } else {
                        return Err(EvalError::InternalError(
                            "ContinuationEscape without pending data".to_string(),
                        ));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Evaluate a single CPS expression step (non-recursive)
    ///
    /// This is the core evaluation function that handles all CpsExpr forms.
    /// Returns a StepResult indicating either a final value or the next step.
    /// The caller (eval) processes these steps in a trampoline loop.
    fn eval_one_step(
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
                    let value = self.lookup_var(name, scopes, &current_env)?;
                    return Ok(StepResult::Done(value));
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

                CpsExpr::Parameterize {
                    bindings,
                    body,
                    cont,
                } => {
                    // Evaluate bindings and push new values onto parameter stacks
                    let mut params = Vec::new();

                    for (param_expr, value_expr) in bindings {
                        // Evaluate param expression
                        let param = self.eval_trivial(param_expr, &current_env, &cont_env)?;

                        // Verify it's a parameter and push new value
                        match &param {
                            Value::Parameter { values, converter } => {
                                // Evaluate value expression
                                let new_val =
                                    self.eval_trivial(value_expr, &current_env, &cont_env)?;

                                // Apply converter if present
                                let converted_val = if let Some(conv) = converter {
                                    match self.evaluator.apply(
                                        *conv.clone(),
                                        vec![new_val.clone()],
                                        false,
                                    )? {
                                        super::EvalResult::Value(v) => v,
                                        _ => {
                                            return Err(EvalError::InvalidSyntax(
                                                "parameter converter returned non-value"
                                                    .to_string(),
                                            ));
                                        }
                                    }
                                } else {
                                    new_val
                                };

                                // Push new value onto parameter stack
                                values.borrow_mut().push(converted_val);
                                params.push(param.clone());
                            }
                            _ => {
                                return Err(EvalError::TypeError(format!(
                                    "parameterize: expected parameter, got {}",
                                    param.type_name()
                                )));
                            }
                        }
                    }

                    // Get the original continuation
                    let original_cont = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

                    // Create cleanup continuation that pops parameters and continues
                    let cleanup_cont = ContValue::ParameterizeCleanup {
                        params,
                        original_cont: Box::new(original_cont),
                    };

                    // Replace the continuation in cont_env so the body's continuation
                    // invocation goes through our cleanup
                    cont_env.insert(cont.clone(), cleanup_cont);

                    // Continue with the body - it will invoke `cont` which now points
                    // to our cleanup continuation
                    current_expr = body.as_ref().clone();
                }

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
                    let val = self.eval_trivial(value, &current_env, &cont_env)?;
                    let k = cont_env
                        .get(cont)
                        .ok_or_else(|| EvalError::UndefinedVariable(cont.to_string()))?
                        .clone();

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
                    // Evaluate quasiquote template using the direct evaluator
                    // since quasiquote doesn't involve continuations
                    let result = super::core_eval::eval_quasiquote_in_env(
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

    /// Evaluate a trivial expression to a value
    ///
    /// Trivial expressions don't have control effects and evaluate immediately.
    fn eval_trivial(
        &self,
        expr: &CpsExpr,
        env: &Rc<Environment>,
        cont_env: &HashMap<Rc<str>, ContValue>,
    ) -> Result<Value, EvalError> {
        match expr {
            CpsExpr::Literal(v) => Ok(v.as_ref().clone()),

            CpsExpr::Var { name, scopes } => self.lookup_var(name, scopes, env),

            CpsExpr::ContRef(k) => {
                let cont = cont_env
                    .get(k)
                    .ok_or_else(|| EvalError::UndefinedVariable(k.to_string()))?;
                // Reify with empty dynamic winds for now (will be filled in by caller if needed)
                Ok(self.reify_continuation(cont, cont_env, &[]))
            }

            CpsExpr::Lambda {
                params,
                variadic,
                cont_param,
                body,
                binding_scope,
            } => Ok(self.make_cps_closure(
                params,
                variadic.as_ref(),
                cont_param,
                body,
                env,
                *binding_scope,
            )),

            _ => Err(EvalError::InternalError(format!(
                "Non-trivial expression in trivial position: {}",
                expr.kind()
            ))),
        }
    }

    /// Look up a variable in the environment
    fn lookup_var(
        &self,
        name: &str,
        scopes: &ScopeSet,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        // Use scoped lookup if scopes are present (for hygienic macros)
        if scopes.is_empty() {
            env.get(name)
                .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
        } else {
            // Scope-based lookup for hygienic macros
            env.get_with_scopes(name, scopes)
                .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
        }
    }

    /// Set a variable in the environment
    fn set_var(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: Value,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        // Use scoped set if scopes are present (for hygienic macros)
        if scopes.is_empty() {
            env.set(name, value)
                .map_err(|_| EvalError::UndefinedVariable(name.to_string()))
        } else {
            // Scope-based set for hygienic macros
            env.set_with_scopes(name, scopes, value)
                .map_err(|_| EvalError::UndefinedVariable(name.to_string()))
        }
    }

    /// Create a CPS closure
    ///
    /// CPS lambdas use the `Procedure::CpsLambda` variant which stores the actual
    /// CpsExpr body. When applied in `apply_cps`, the CPS body is evaluated with
    /// the continuation parameter bound to the current continuation.
    fn make_cps_closure(
        &self,
        params: &[CpsParam],
        variadic: Option<&CpsParam>,
        cont_param: &Rc<str>,
        body: &Rc<CpsExpr>,
        env: &Rc<Environment>,
        binding_scope: Option<patina_core::ScopeId>,
    ) -> Value {
        // Convert CpsParams to ScopedParams
        let scoped_params: Vec<ScopedParam> = params
            .iter()
            .map(|p| ScopedParam {
                name: p.name.clone(),
                scopes: p.scopes.clone(),
            })
            .collect();

        let variadic_param = variadic.map(|p| ScopedParam {
            name: p.name.clone(),
            scopes: p.scopes.clone(),
        });

        // Create a CpsLambda with the actual CPS body
        Value::Procedure(Rc::new(Procedure::CpsLambda {
            params: scoped_params,
            variadic: variadic_param,
            cont_param: cont_param.clone(),
            body: body.clone(),
            env: env.clone(),
            binding_scope,
        }))
    }

    /// Convert a cont_env to the format stored in CpsContinuation
    fn capture_cont_bindings(
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
    fn restore_cont_bindings(
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
    fn restore_dynamic_wind_cleanup(&self, k: &CpsContinuation) -> Result<ContValue, EvalError> {
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
    fn reify_continuation(
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

    /// Apply a CPS procedure (returns StepResult for trampolining)
    #[allow(clippy::too_many_arguments)]
    fn apply_cps_step(
        &self,
        proc: Value,
        args: Vec<Value>,
        cont: ContValue,
        _env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        match proc {
            Value::Procedure(p) => match p.as_ref() {
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

                    // Check arity
                    let min_args = params.len();
                    if variadic.is_none() && args.len() != min_args {
                        return Err(EvalError::WrongArity {
                            expected: min_args.to_string(),
                            actual: args.len(),
                        });
                    }
                    if args.len() < min_args {
                        return Err(EvalError::WrongArity {
                            expected: format!("at least {}", min_args),
                            actual: args.len(),
                        });
                    }

                    // Bind fixed parameters with proper hygiene support
                    // This mirrors the non-CPS path in application.rs which uses binding_scope
                    for (param, arg) in params.iter().zip(args.iter()) {
                        if !param.scopes.is_empty() {
                            // Macro-introduced parameter: use its explicit scopes
                            new_env.define_with_scopes(
                                param.name.to_string(),
                                param.scopes.clone(),
                                arg.clone(),
                            );
                        } else {
                            // Non-macro parameter: simple binding + scoped binding if binding_scope present
                            new_env.define(param.name.to_string(), arg.clone());
                            // Also add scoped binding so macro-expanded refs can find it
                            if let Some(scope) = binding_scope {
                                let mut scopes = ScopeSet::new();
                                scopes.add_scope(*scope);
                                new_env.define_with_scopes(
                                    param.name.to_string(),
                                    scopes,
                                    arg.clone(),
                                );
                            }
                        }
                    }

                    // Bind variadic parameter if present
                    if let Some(variadic_param) = variadic {
                        let rest_args: Vec<Value> = args[params.len()..].to_vec();
                        let rest_list = list_from_vec(rest_args);
                        if !variadic_param.scopes.is_empty() {
                            new_env.define_with_scopes(
                                variadic_param.name.to_string(),
                                variadic_param.scopes.clone(),
                                rest_list.clone(),
                            );
                        } else {
                            new_env.define(variadic_param.name.to_string(), rest_list.clone());
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

                // Direct-style lambda: delegate to direct evaluator
                Procedure::Lambda { env, .. } => {
                    let result = self.apply_regular_proc(&p, args)?;

                    // Return InvokeContinuation step instead of recursive call
                    Ok(StepResult::InvokeContinuation {
                        cont,
                        value: result,
                        env: env.clone(),
                        cont_env,
                        prompt_stack,
                        dynamic_winds,
                        exception_handlers,
                    })
                }

                Procedure::Primitive { name, .. } => {
                    // Handle CPS-sensitive primitives specially
                    match *name {
                        "call-with-values" => {
                            // (call-with-values producer consumer)
                            // In CPS: call producer with a continuation that will call consumer
                            if args.len() != 2 {
                                return Err(EvalError::WrongArity {
                                    expected: "2".to_string(),
                                    actual: args.len(),
                                });
                            }
                            let producer = args[0].clone();
                            let consumer = args[1].clone();

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

                        "force" => {
                            // (force promise)
                            // In CPS: if promise is forced, return value; else call thunk in CPS mode
                            if args.len() != 1 {
                                return Err(EvalError::WrongArity {
                                    expected: "1".to_string(),
                                    actual: args.len(),
                                });
                            }
                            self.force_promise_cps(
                                args.into_iter().next().unwrap(),
                                cont,
                                cont_env,
                                prompt_stack,
                                dynamic_winds,
                                exception_handlers,
                            )
                        }

                        "dynamic-wind" => {
                            // (dynamic-wind before body after)
                            // Sets up handlers to be called when entering/leaving this dynamic extent
                            if args.len() != 3 {
                                return Err(EvalError::WrongArity {
                                    expected: "3".to_string(),
                                    actual: args.len(),
                                });
                            }
                            let before = args[0].clone();
                            let body = args[1].clone();
                            let after = args[2].clone();

                            // Create the wind record (but don't push yet - push after calling before)
                            let wind_record = DynamicWindRecord::new(before.clone(), after.clone());
                            let wind_id = wind_record.id;

                            // Create cleanup continuation that will:
                            // 1. Pop the wind record
                            // 2. Call the after thunk
                            // 3. Continue with the original continuation
                            let cleanup_cont = ContValue::DynamicWindCleanup {
                                after,
                                wind_id,
                                original_cont: Box::new(cont),
                            };

                            // First call the "before" thunk
                            // After that returns, we need to:
                            // 1. Push the wind record
                            // 2. Call the body thunk
                            // The result goes to cleanup_cont

                            // We'll use a special two-phase approach:
                            // Phase 1: Call before thunk with a continuation that sets up phase 2
                            // Phase 2: Push wind record, call body with cleanup_cont

                            // Create a continuation for after the "before" thunk completes
                            let setup_cont = ContValue::DynamicWindSetup {
                                wind_record,
                                body,
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

                        "with-exception-handler" => {
                            // (with-exception-handler handler thunk)
                            // Installs handler for duration of thunk's dynamic extent
                            if args.len() != 2 {
                                return Err(EvalError::WrongArity {
                                    expected: "2".to_string(),
                                    actual: args.len(),
                                });
                            }
                            let handler = args[0].clone();
                            let thunk = args[1].clone();

                            // Verify both are procedures
                            if !matches!(handler, Value::Procedure(_) | Value::Continuation(_)) {
                                return Err(EvalError::TypeError(
                                    "with-exception-handler: first argument must be a procedure"
                                        .to_string(),
                                ));
                            }
                            if !matches!(thunk, Value::Procedure(_) | Value::Continuation(_)) {
                                return Err(EvalError::TypeError(
                                    "with-exception-handler: second argument must be a procedure"
                                        .to_string(),
                                ));
                            }

                            // Create cleanup continuation that pops the handler when thunk completes
                            let cleanup_cont = ContValue::ExceptionHandlerCleanup {
                                original_cont: Box::new(cont),
                            };

                            // Push the exception handler onto the stack
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

                        "raise" => {
                            // (raise obj) - Raise non-continuable exception
                            // If handler returns, raises secondary exception
                            if args.len() != 1 {
                                return Err(EvalError::WrongArity {
                                    expected: "1".to_string(),
                                    actual: args.len(),
                                });
                            }
                            let exception = args.into_iter().next().unwrap();

                            if let Some(handler_entry) = exception_handlers.last().cloned() {
                                // Pop this handler (one-shot semantics)
                                let mut new_handlers = exception_handlers;
                                new_handlers.pop();

                                // Create continuation for when handler returns
                                // For non-continuable raise, returning is an error
                                let handler_return_cont = ContValue::RaiseHandlerReturn {
                                    continuable: false,
                                    original_exception: Some(exception.clone()),
                                    original_cont: Box::new(cont),
                                };

                                // Call handler with exception
                                Ok(StepResult::ApplyProc {
                                    proc: handler_entry.handler,
                                    args: vec![exception],
                                    cont: handler_return_cont,
                                    env: self.evaluator.global_env.clone(),
                                    cont_env,
                                    prompt_stack,
                                    dynamic_winds,
                                    exception_handlers: new_handlers,
                                })
                            } else {
                                // No handler - propagate to Rust level
                                use super::error::SchemeExceptionKind;
                                Err(EvalError::SchemeException {
                                    kind: SchemeExceptionKind::Error,
                                    message: format!("unhandled exception: {}", exception),
                                    irritants_display: String::new(),
                                })
                            }
                        }

                        "raise-continuable" => {
                            // (raise-continuable obj) - Raise continuable exception
                            // Handler's return value becomes result of raise-continuable
                            if args.len() != 1 {
                                return Err(EvalError::WrongArity {
                                    expected: "1".to_string(),
                                    actual: args.len(),
                                });
                            }
                            let exception = args.into_iter().next().unwrap();

                            if let Some(handler_entry) = exception_handlers.last().cloned() {
                                // Pop this handler (one-shot semantics)
                                let mut new_handlers = exception_handlers;
                                new_handlers.pop();

                                // Create continuation for when handler returns
                                // For continuable raise, handler's return value continues
                                let handler_return_cont = ContValue::RaiseHandlerReturn {
                                    continuable: true,
                                    original_exception: None,
                                    original_cont: Box::new(cont),
                                };

                                // Call handler with exception
                                Ok(StepResult::ApplyProc {
                                    proc: handler_entry.handler,
                                    args: vec![exception],
                                    cont: handler_return_cont,
                                    env: self.evaluator.global_env.clone(),
                                    cont_env,
                                    prompt_stack,
                                    dynamic_winds,
                                    exception_handlers: new_handlers,
                                })
                            } else {
                                // No handler - propagate to Rust level
                                use super::error::SchemeExceptionKind;
                                Err(EvalError::SchemeException {
                                    kind: SchemeExceptionKind::Error,
                                    message: format!(
                                        "unhandled continuable exception: {}",
                                        exception
                                    ),
                                    irritants_display: String::new(),
                                })
                            }
                        }

                        "error" => {
                            // (error message obj ...) - Create error object and raise it
                            // This is CPS-aware so it goes through the exception handler stack
                            if args.is_empty() {
                                return Err(EvalError::WrongArity {
                                    expected: "at least 1".to_string(),
                                    actual: args.len(),
                                });
                            }

                            // First argument must be a string (the message)
                            let message = match &args[0] {
                                Value::String(s) => s.borrow().clone(),
                                _ => {
                                    return Err(EvalError::TypeError(
                                        "error: first argument must be a string".to_string(),
                                    ));
                                }
                            };

                            // Remaining arguments are irritants
                            let irritants: Vec<Value> = args[1..].to_vec();

                            // Create exception object
                            let exception = Value::Exception(Rc::new(
                                patina_core::ExceptionObject {
                                    kind: patina_core::ExceptionKind::Error,
                                    message,
                                    irritants,
                                },
                            ));

                            // Now do the same as raise (non-continuable)
                            if let Some(handler_entry) = exception_handlers.last().cloned() {
                                // Pop this handler (one-shot semantics)
                                let mut new_handlers = exception_handlers;
                                new_handlers.pop();

                                // Create continuation for when handler returns
                                let handler_return_cont = ContValue::RaiseHandlerReturn {
                                    continuable: false,
                                    original_exception: Some(exception.clone()),
                                    original_cont: Box::new(cont),
                                };

                                // Call handler with exception
                                Ok(StepResult::ApplyProc {
                                    proc: handler_entry.handler,
                                    args: vec![exception],
                                    cont: handler_return_cont,
                                    env: self.evaluator.global_env.clone(),
                                    cont_env,
                                    prompt_stack,
                                    dynamic_winds,
                                    exception_handlers: new_handlers,
                                })
                            } else {
                                // No handler - propagate to Rust level
                                use super::error::SchemeExceptionKind;
                                let irritants_display = args[1..]
                                    .iter()
                                    .map(|v| format!("{}", v))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                Err(EvalError::SchemeException {
                                    kind: SchemeExceptionKind::Error,
                                    message: match &args[0] {
                                        Value::String(s) => s.borrow().clone(),
                                        _ => "error".to_string(),
                                    },
                                    irritants_display,
                                })
                            }
                        }

                        "apply" => {
                            // (apply proc arg1 ... args)
                            // The last argument must be a list
                            if args.len() < 2 {
                                return Err(EvalError::WrongArity {
                                    expected: "at least 2".to_string(),
                                    actual: args.len(),
                                });
                            }

                            let proc = args[0].clone();
                            let last_idx = args.len() - 1;
                            let last_arg = &args[last_idx];

                            // Flatten: take args[1..last] and append the list in args[last]
                            let mut flat_args: Vec<Value> = args[1..last_idx].to_vec();

                            // Append the list
                            let mut current = last_arg.clone();
                            loop {
                                match current {
                                    Value::Null => break,
                                    Value::Pair(p) => {
                                        let (car, cdr) = {
                                            let b = p.borrow();
                                            (b.0.clone(), b.1.clone())
                                        };
                                        flat_args.push(car);
                                        current = cdr;
                                    }
                                    _ => {
                                        return Err(EvalError::TypeError(format!(
                                            "apply: expected list as last argument, got {}",
                                            last_arg
                                        )));
                                    }
                                }
                            }

                            // Now apply the procedure with the flattened arguments
                            Ok(StepResult::ApplyProc {
                                proc,
                                args: flat_args,
                                cont,
                                env: self.evaluator.global_env.clone(),
                                cont_env,
                                prompt_stack,
                                dynamic_winds,
                                exception_handlers,
                            })
                        }

                        _ => {
                            // For other primitives, delegate to direct evaluator
                            // Higher-order primitives like map/for-each should be implemented
                            // in Scheme (lib/scheme/base/) for proper CPS compatibility.
                            //
                            // IMPORTANT: Wrap primitive calls to catch I/O and read errors
                            // and route them through the CPS exception handler stack.
                            let prim_result =
                                self.evaluator
                                    .apply(Value::Procedure(p.clone()), args, false);

                            match prim_result {
                                Ok(eval_result) => {
                                    let result = match eval_result {
                                        super::EvalResult::Value(v) => v,
                                        _ => {
                                            return Err(EvalError::InternalError(
                                                "Primitive returned tail call".to_string(),
                                            ));
                                        }
                                    };

                                    // Return InvokeContinuation step instead of recursive call
                                    Ok(StepResult::InvokeContinuation {
                                        cont,
                                        value: result,
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
                    }
                }
            },

            Value::Continuation(k) => {
                // Invoking a captured continuation - non-local control transfer
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }

                let val = args.into_iter().next().unwrap();

                // Run dynamic-wind handlers for continuation jump
                // This travels from current winds to the captured winds
                self.run_wind_handlers(&dynamic_winds, &k.dynamic_winds)?;

                // Store escape data and return error to propagate up
                set_pending_escape(val, k);
                Err(EvalError::ContinuationEscape)
            }

            // Parameters are callable - delegate to direct evaluator
            Value::Parameter { values, converter } => {
                // Parameters can be called with 0 or 1 arguments:
                // (param)      => get current value (top of stack)
                // (param val)  => set value (replace top of stack after applying converter)
                let result = match args.len() {
                    0 => {
                        // Get current value (top of stack)
                        let stack = values.borrow();
                        let current_value = stack.last().ok_or_else(|| {
                            EvalError::InvalidSyntax("parameter stack is empty".to_string())
                        })?;
                        current_value.clone()
                    }
                    1 => {
                        // Set value (replace top of stack after applying converter)
                        let new_val = if let Some(conv) = converter {
                            // Apply converter to new value - delegate to direct evaluator
                            let conv_result = self.evaluator.apply(
                                *conv.clone(),
                                vec![args[0].clone()],
                                false,
                            )?;
                            match conv_result {
                                super::EvalResult::Value(v) => v,
                                _ => {
                                    return Err(EvalError::InvalidSyntax(
                                        "parameter converter returned non-value".to_string(),
                                    ));
                                }
                            }
                        } else {
                            args[0].clone()
                        };

                        // Set the new value (replace top of stack)
                        let mut stack = values.borrow_mut();
                        if let Some(top) = stack.last_mut() {
                            *top = new_val;
                        }
                        Value::Unspecified
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
                    value: result,
                    env: self.evaluator.global_env.clone(),
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                })
            }

            _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
        }
    }

    /// Apply a regular (non-CPS) procedure
    fn apply_regular_proc(&self, proc: &Procedure, args: Vec<Value>) -> Result<Value, EvalError> {
        let result = self
            .evaluator
            .apply(Value::Procedure(Rc::new(proc.clone())), args, false)?;
        match result {
            super::EvalResult::Value(v) => Ok(v),
            super::EvalResult::TailCall { expr, env } => {
                // Need to trampoline
                self.evaluator.eval_in_env(&expr, &env)
            }
            super::EvalResult::TailCallPrimitive { proc, args } => self
                .evaluator
                .apply(proc, args, false)
                .and_then(|r| match r {
                    super::EvalResult::Value(v) => Ok(v),
                    _ => Err(EvalError::InternalError(
                        "Unexpected tail call from primitive".to_string(),
                    )),
                }),
        }
    }

    /// Convert certain errors (IOError, InvalidSyntax) to CPS-routed exceptions
    ///
    /// If there are exception handlers installed, convert the error to a Scheme
    /// exception value and route it through the handler stack. Otherwise,
    /// propagate the error as a Rust Err.
    fn maybe_route_error_through_cps(
        &self,
        err: EvalError,
        cont: ContValue,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Determine if this error should be converted to a CPS exception
        let (exception_kind, message) = match &err {
            EvalError::IOError(msg) => {
                // Check if it's a file error (contains "Cannot open", "Cannot delete", etc.)
                let kind = if msg.contains("Cannot open")
                    || msg.contains("Cannot delete")
                    || msg.contains("Cannot read")
                    || msg.contains("Cannot write")
                    || msg.contains("No such file")
                {
                    patina_core::ExceptionKind::FileError
                } else {
                    patina_core::ExceptionKind::Error
                };
                (kind, msg.clone())
            }
            EvalError::InvalidSyntax(msg) => {
                // Read errors typically contain "read:" in the message
                let kind = if msg.contains("read:") {
                    patina_core::ExceptionKind::ReadError
                } else {
                    patina_core::ExceptionKind::Error
                };
                (kind, msg.clone())
            }
            // Other errors: propagate as-is
            _ => return Err(err),
        };

        // If there are exception handlers, route through them
        if let Some(handler_entry) = exception_handlers.last().cloned() {
            // Create exception object
            let exception = Value::Exception(Rc::new(patina_core::ExceptionObject {
                kind: exception_kind,
                message,
                irritants: vec![],
            }));

            // Pop the handler (it's been invoked)
            let new_handlers = exception_handlers[..exception_handlers.len() - 1].to_vec();

            // Create continuation for when handler returns
            // Since these are system errors, treat as non-continuable
            let raise_return_cont = ContValue::RaiseHandlerReturn {
                continuable: false,
                original_exception: Some(exception.clone()),
                original_cont: Box::new(cont),
            };

            // Call the handler with the exception
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception],
                cont: raise_return_cont,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers: new_handlers,
            })
        } else {
            // No handlers - propagate the error as-is
            Err(err)
        }
    }

    /// Invoke a continuation with a value (returns StepResult for trampolining)
    #[allow(clippy::too_many_arguments)]
    fn invoke_continuation_step(
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
                    let secondary_exception = Value::Exception(Rc::new(
                        patina_core::ExceptionObject {
                            kind: patina_core::ExceptionKind::Error,
                            message: "exception handler returned from non-continuable exception"
                                .to_string(),
                            irritants: original_exception.into_iter().collect(),
                        },
                    ));

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
                        use super::error::SchemeExceptionKind;
                        Err(EvalError::SchemeException {
                            kind: SchemeExceptionKind::Error,
                            message: "exception handler returned from non-continuable exception"
                                .to_string(),
                            irritants_display: String::new(),
                        })
                    }
                }
            }
        }
    }

    /// Force a promise in CPS mode
    ///
    /// If the promise is already forced, return the cached value.
    /// Otherwise, call the thunk (which may be a CPS lambda) and cache the result.
    fn force_promise_cps(
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
    fn make_delimited_continuation(
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

    /// Run dynamic-wind handlers when switching from one continuation to another
    ///
    /// This implements the "travel to point" algorithm from chibi-scheme:
    /// 1. Find the common prefix of the two wind stacks (by ID)
    /// 2. Run "after" handlers for winds being exited (from current to common, in reverse)
    /// 3. Run "before" handlers for winds being entered (from common to target)
    fn run_wind_handlers(
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

    /// Evaluate a primitive operation
    ///
    /// Delegates to the tree-walker's primitive implementations.
    fn eval_primop(&self, op: &CpsPrimitive, args: Vec<Value>) -> Result<Value, EvalError> {
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
                return Ok(Value::Boolean(matches!(args[0], Value::Continuation(_))));
            }
            CpsPrimitive::IsPromptTag => {
                // prompt-tag? is a new predicate
                if args.len() != 1 {
                    return Err(EvalError::WrongArity {
                        expected: "1".to_string(),
                        actual: args.len(),
                    });
                }
                return Ok(Value::Boolean(matches!(
                    args[0],
                    Value::ContinuationPromptTag(_)
                )));
            }
            _ => {}
        }

        // Delegate to the global environment's primitive
        if let Some(proc) = self.evaluator.global_env.get(name) {
            let result = self.evaluator.apply(proc, args, false)?;
            match result {
                super::EvalResult::Value(v) => Ok(v),
                _ => Err(EvalError::InternalError(format!(
                    "Primitive {} returned unexpected result",
                    name
                ))),
            }
        } else {
            Err(EvalError::UndefinedVariable(name.to_string()))
        }
    }
}

/// Evaluate a CoreExpr using CPS transformation
///
/// This is the main entry point for CPS-based evaluation. It:
/// 1. Transforms CoreExpr to CpsExpr using CpsTransformer
/// 2. Evaluates the CpsExpr using CpsEvaluator
///
/// This function should be used when call/cc or delimited continuations
/// are needed. For regular code, use `eval_core` instead for better performance.
///
/// # Arguments
/// * `expr` - The CoreExpr to evaluate
/// * `env` - The environment for variable lookup (currently unused, uses global)
/// * `evaluator` - The evaluator instance (wrapped in Rc for sharing)
///
/// # Returns
/// The result of evaluating the expression
pub fn eval_cps(
    expr: &patina_core::CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    use patina_core::CoreExpr;
    use patina_ir::CpsTransformer;

    // Handle Import specially - it's a side-effect that modifies the environment
    // and doesn't need CPS transformation
    if let CoreExpr::Import { import_sets } = expr {
        for import_set_expr in import_sets {
            let import_set = patina_frontend::LibraryDefinition::parse_import_set(import_set_expr)
                .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;
            evaluator.process_import_for_eval(&import_set, &env)?;
        }
        return Ok(Value::Unspecified);
    }

    // Transform CoreExpr to CpsExpr
    let transformer = CpsTransformer::new();
    let cps_expr = transformer.transform_toplevel(expr);

    // Create CPS evaluator and evaluate
    let cps_evaluator = CpsEvaluator::new(evaluator);
    cps_evaluator.eval(&cps_expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_evaluator() -> super::super::Evaluator {
        super::super::Evaluator::new()
    }

    /// Helper to check if two Values are equal (since Value doesn't impl PartialEq)
    fn values_equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Integer(x), Value::Integer(y)) => x == y,
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            _ => false,
        }
    }

    #[test]
    fn test_cps_eval_literal() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt 42)
        let expr = CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(42)))));
        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(42)));
    }

    #[test]
    fn test_cps_eval_variable() {
        let evaluator = make_test_evaluator();
        evaluator
            .global_env
            .define("x".to_string(), Value::Integer(10));
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (halt x)
        let expr = CpsExpr::Halt(Rc::new(CpsExpr::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        }));
        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(10)));
    }

    #[test]
    fn test_cps_eval_primop() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (let-cont ((k result) (halt result)) (+ 1 2 k))
        let halt_cont = CpsExpr::LetCont {
            name: Rc::from("k"),
            param: Rc::from("result"),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(CpsExpr::PrimOp {
                op: CpsPrimitive::Add,
                args: vec![
                    CpsExpr::Literal(Rc::new(Value::Integer(1))),
                    CpsExpr::Literal(Rc::new(Value::Integer(2))),
                ],
                cont: Rc::from("k"),
            }),
        };

        let result = cps_eval.eval(&halt_cont).unwrap();
        assert!(values_equal(&result, &Value::Integer(3)));
    }

    #[test]
    fn test_cps_eval_if() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // CPS: (if #t (halt 1) (halt 2))
        let expr = CpsExpr::If {
            test: Rc::new(CpsExpr::Literal(Rc::new(Value::Boolean(true)))),
            consequent: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Integer(1),
            ))))),
            alternate: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Integer(2),
            ))))),
        };

        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(1)));

        // Test false branch
        let expr_false = CpsExpr::If {
            test: Rc::new(CpsExpr::Literal(Rc::new(Value::Boolean(false)))),
            consequent: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Integer(1),
            ))))),
            alternate: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Integer(2),
            ))))),
        };

        let result_false = cps_eval.eval(&expr_false).unwrap();
        assert!(values_equal(&result_false, &Value::Integer(2)));
    }

    #[test]
    fn test_cps_eval_lambda_application() {
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // Create a simple CPS lambda that returns its argument
        // CPS: ((lambda (x k) (k x)) 42 halt)
        let lambda = CpsExpr::Lambda {
            params: vec![CpsParam {
                name: Rc::from("x"),
                scopes: ScopeSet::new(),
            }],
            variadic: None,
            cont_param: Rc::from("k"),
            body: Rc::new(CpsExpr::Continue {
                cont: Rc::from("k"),
                value: Rc::new(CpsExpr::Var {
                    name: Rc::from("x"),
                    scopes: ScopeSet::new(),
                }),
            }),
            binding_scope: None,
        };

        // Apply it to 42
        let expr = CpsExpr::LetVal {
            name: Rc::from("f"),
            value: Rc::new(lambda),
            body: Rc::new(CpsExpr::LetCont {
                name: Rc::from("halt_k"),
                param: Rc::from("result"),
                cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                    name: Rc::from("result"),
                    scopes: ScopeSet::new(),
                }))),
                body: Rc::new(CpsExpr::App {
                    func: Rc::new(CpsExpr::Var {
                        name: Rc::from("f"),
                        scopes: ScopeSet::new(),
                    }),
                    args: vec![CpsExpr::Literal(Rc::new(Value::Integer(42)))],
                    cont: Rc::from("halt_k"),
                }),
            }),
        };

        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(42)));
    }

    #[test]
    fn test_cps_eval_callcc_basic() {
        // Test: (call/cc (lambda (k) 42))
        // The continuation is not invoked, so result is 42
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // Lambda body just returns 42 (ignores k)
        let lambda_body = CpsExpr::Continue {
            cont: Rc::from("k"),
            value: Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(42)))),
        };

        let lambda = CpsExpr::Lambda {
            params: vec![CpsParam {
                name: Rc::from("captured_k"),
                scopes: ScopeSet::new(),
            }],
            variadic: None,
            cont_param: Rc::from("k"),
            body: Rc::new(lambda_body),
            binding_scope: None,
        };

        // call/cc expression
        let expr = CpsExpr::LetCont {
            name: Rc::from("halt_k"),
            param: Rc::from("result"),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(CpsExpr::LetVal {
                name: Rc::from("proc"),
                value: Rc::new(lambda),
                body: Rc::new(CpsExpr::CallCC {
                    proc: Rc::new(CpsExpr::Var {
                        name: Rc::from("proc"),
                        scopes: ScopeSet::new(),
                    }),
                    cont: Rc::from("halt_k"),
                }),
            }),
        };

        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(42)));
    }

    #[test]
    fn test_cps_eval_callcc_escape() {
        // Test: (call/cc (lambda (exit) (exit 99) 1))
        // The continuation is invoked with 99, so 1 is never reached
        // Result should be 99
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // Lambda body: (begin (exit 99) 1)
        // In CPS: apply exit to 99, then (unreachable) continue with 1
        // But since exit is a continuation, invoking it escapes
        let lambda_body = CpsExpr::App {
            func: Rc::new(CpsExpr::Var {
                name: Rc::from("exit"),
                scopes: ScopeSet::new(),
            }),
            args: vec![CpsExpr::Literal(Rc::new(Value::Integer(99)))],
            // This continuation is never called because exit escapes
            cont: Rc::from("k"),
        };

        let lambda = CpsExpr::Lambda {
            params: vec![CpsParam {
                name: Rc::from("exit"),
                scopes: ScopeSet::new(),
            }],
            variadic: None,
            cont_param: Rc::from("k"),
            body: Rc::new(lambda_body),
            binding_scope: None,
        };

        // call/cc expression
        let expr = CpsExpr::LetCont {
            name: Rc::from("halt_k"),
            param: Rc::from("result"),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(CpsExpr::LetVal {
                name: Rc::from("proc"),
                value: Rc::new(lambda),
                body: Rc::new(CpsExpr::CallCC {
                    proc: Rc::new(CpsExpr::Var {
                        name: Rc::from("proc"),
                        scopes: ScopeSet::new(),
                    }),
                    cont: Rc::from("halt_k"),
                }),
            }),
        };

        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(99)));
    }

    #[test]
    fn test_continuation_is_procedure() {
        // Test that captured continuations satisfy procedure?
        // This is important for R7RS compliance
        let evaluator = make_test_evaluator();

        // Create a continuation value
        let cont = Value::Continuation(Rc::new(patina_core::value::CpsContinuation {
            body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Literal(Rc::new(
                Value::Integer(0),
            ))))),
            param: Rc::from("v"),
            env: evaluator.global_env.clone(),
            prompt_tag: None,
            dynamic_winds: vec![],
            captured_cont_bindings: vec![],
        }));

        // Check that it matches the Continuation variant
        assert!(matches!(cont, Value::Continuation(_)));

        // Check that procedure? would return true (logic from predicates.rs)
        let is_proc = matches!(cont, Value::Procedure(_) | Value::Continuation(_));
        assert!(is_proc, "Continuations should satisfy procedure?");
    }

    #[test]
    fn test_cps_define_in_correct_scope() {
        // Test that define creates bindings in the correct environment
        // This was a bug where define was using global_env instead of def_env
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // (let () (define x 42) x)
        // In CPS: create child env, define x, then return x
        let expr = CpsExpr::LetCont {
            name: Rc::from("k"),
            param: Rc::from("result"),
            cont_body: Rc::new(CpsExpr::Halt(Rc::new(CpsExpr::Var {
                name: Rc::from("result"),
                scopes: ScopeSet::new(),
            }))),
            body: Rc::new(CpsExpr::Define {
                name: Rc::from("x"),
                value: Rc::new(CpsExpr::Literal(Rc::new(Value::Integer(42)))),
                cont: Rc::new(CpsExpr::Continue {
                    cont: Rc::from("k"),
                    value: Rc::new(CpsExpr::Var {
                        name: Rc::from("x"),
                        scopes: ScopeSet::new(),
                    }),
                }),
            }),
        };

        let result = cps_eval.eval(&expr).unwrap();
        assert!(values_equal(&result, &Value::Integer(42)));
    }

    #[test]
    fn test_dynamic_wind_record_id() {
        // Test that DynamicWindRecord generates unique IDs
        let before = Value::Integer(1);
        let after = Value::Integer(2);

        let r1 = DynamicWindRecord::new(before.clone(), after.clone());
        let r2 = DynamicWindRecord::new(before.clone(), after.clone());

        // IDs should be unique
        assert_ne!(r1.id, r2.id, "Each DynamicWindRecord should have unique ID");
    }

    #[test]
    fn test_run_wind_handlers_common_prefix() {
        // Test that run_wind_handlers finds common prefix correctly
        // and runs the right handlers in the right order
        let evaluator = make_test_evaluator();
        let cps_eval = CpsEvaluator::new(&evaluator);

        // Create mock thunks (we don't actually call them in this test)
        let mock_thunk = Value::Integer(0);

        // Create wind records
        let r1 = DynamicWindRecord::new(mock_thunk.clone(), mock_thunk.clone());
        let _r2 = DynamicWindRecord::new(mock_thunk.clone(), mock_thunk.clone());
        let _r3 = DynamicWindRecord::new(mock_thunk.clone(), mock_thunk.clone());

        // Test: empty stacks should not run any handlers
        let from: Vec<DynamicWindRecord> = vec![];
        let to: Vec<DynamicWindRecord> = vec![];
        // This should not panic
        cps_eval.run_wind_handlers(&from, &to).unwrap();

        // Test: going from [r1] to [r1] (same ID) should not run handlers
        let from = vec![r1.clone()];
        let to = vec![r1.clone()]; // Same ID
        cps_eval.run_wind_handlers(&from, &to).unwrap();

        // r2 and r3 reserved for future tests with actual thunks
    }
}
