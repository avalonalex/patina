//! CoreExpr Evaluator - Evaluates Core IR expressions
//!
//! This module implements evaluation of CoreExpr (the minimal IR) rather than
//! the full Value-based AST. This is part of the IR migration strategy (Phase 2).
//!
//! # Architecture
//!
//! ```text
//! Parser → Value → Macro Expander → Value → Desugarer → CoreExpr → [THIS MODULE] → Value
//! ```
//!
//! The CoreExpr evaluator handles only the 9 core forms defined in the IR:
//! - `Literal` - Self-evaluating values (numbers, booleans, strings, etc.)
//! - `Var` - Variable references
//! - `Quote` - Return quoted data
//! - `Lambda` - Create closures
//! - `If` - Conditional evaluation (ternary if-then-else)
//! - `Set` - Mutation of existing bindings
//! - `Define` - Create new bindings
//! - `Begin` - Sequence expressions, return last value
//! - `App` - Function application
//!
//! All derived forms (let, cond, and, or, case, do, etc.) have already been desugared
//! by the frontend, so this evaluator is much simpler than the Value evaluator.
//!
//! # Status: Phase 2 (Parallel Path)
//!
//! This evaluator currently runs **in parallel** with the legacy Value-based evaluator:
//! - **Primary path**: `Evaluator::eval()` - Evaluates Value AST directly
//! - **New path**: `eval_core()` - Evaluates CoreExpr IR (this module)
//!
//! Both paths are tested for parity and produce identical results. The CoreExpr path
//! will eventually become the default in Phase 3.
//!
//! # Design Note: Hybrid Approach
//!
//! This evaluator takes CoreExpr as input and produces Value as output.
//! Lambdas are stored as Value::Procedure with Vec<Value> bodies (not CoreExpr).
//! This is a pragmatic choice to work with the existing runtime without
//! modifying the Value enum or breaking existing code.
//!
//! **Why this works:**
//! - CoreExpr bodies are converted to Value via `core_expr_to_value()`
//! - When lambdas execute, Value bodies are converted back via `value_to_core_simple()`
//! - This roundtrip is isolated to lambda body evaluation
//!
//! **Future work (Phase 3):**
//! - Introduce CoreClosure type that stores Vec<CoreExpr> bodies directly
//! - Eliminate Value ↔ CoreExpr conversion overhead
//! - Make CoreExpr the primary representation throughout
//!
//! # Performance
//!
//! Current benchmarks show CoreExpr evaluation is comparable to Value evaluation:
//! - Overhead from Value ↔ CoreExpr conversion in lambdas: ~minimal
//! - Benefit from simpler pattern matching: ~small gain
//! - Net result: Roughly equivalent performance
//!
//! The main benefit is architectural cleanliness, not performance (yet).

use super::error::EvalError;
use patina_ir::{CoreExpr, Formals};
use patina_runtime::environment::Environment;
use patina_runtime::value::{Procedure, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Result of a CoreExpr evaluation step
///
/// Used for tail call optimization via trampoline pattern.
/// Instead of recursing directly, tail positions return TailCall
/// which tells the trampoline to continue with the next expression.
enum CoreEvalResult {
    /// Final value - evaluation complete
    Value(Value),
    /// Tail call - continue with this expression and environment
    TailCall {
        expr: CoreExpr,
        env: Rc<Environment>,
    },
}

/// Helper to construct a pair
fn cons(car: Value, cdr: Value) -> Value {
    Value::Pair(Rc::new(RefCell::new((car, cdr))))
}

/// Helper to fully evaluate a non-tail expression
///
/// When we need a value (not tail position), we must handle potential tail calls
/// by trampolining them through eval_core.
fn eval_non_tail(
    expr: &CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    match eval_core_step(expr, env.clone(), evaluator)? {
        CoreEvalResult::Value(v) => Ok(v),
        CoreEvalResult::TailCall { expr, env } => eval_core(&expr, env, evaluator),
    }
}

/// Evaluate a CoreExpr in the given environment
///
/// This is the main entry point for CoreExpr evaluation.
/// Returns a Value result.
///
/// Uses a trampoline pattern for tail call optimization (TCO).
/// Tail positions return CoreEvalResult::TailCall instead of recursing,
/// preventing stack growth for recursive calls in tail position.
///
/// # Parameters
/// - `expr`: The CoreExpr to evaluate
/// - `env`: The environment for variable lookup
/// - `evaluator`: The evaluator instance (needed for primitives and Value evaluation)
pub fn eval_core(
    expr: &CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    let mut current_expr = expr.clone();
    let mut current_env = env;

    // Trampoline loop for TCO
    loop {
        match eval_core_step(&current_expr, current_env.clone(), evaluator)? {
            CoreEvalResult::Value(v) => return Ok(v),
            CoreEvalResult::TailCall { expr, env } => {
                current_expr = expr;
                current_env = env;
            }
        }
    }
}

/// Internal implementation of CoreExpr evaluation (one step)
///
/// Returns CoreEvalResult to enable tail call optimization.
/// Tail positions return TailCall, non-tail positions return Value.
fn eval_core_step(
    expr: &CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<CoreEvalResult, EvalError> {
    match expr {
        // Literals evaluate to themselves
        CoreExpr::Literal(v) => Ok(CoreEvalResult::Value(v.clone())),

        // Variables: look up in environment
        CoreExpr::Var(name) => env
            .get(name)
            .map(CoreEvalResult::Value)
            .ok_or_else(|| EvalError::UndefinedVariable(name.to_string())),

        // Quote: return the quoted value as-is
        CoreExpr::Quote(v) => Ok(CoreEvalResult::Value(v.clone())),

        // Quasiquote: template with selective evaluation
        CoreExpr::Quasiquote(template) => {
            let result = eval_quasiquote_impl(evaluator, template, &env, 0)?;
            Ok(CoreEvalResult::Value(result))
        }

        // Lambda: create a closure
        // NOTE: We convert CoreExpr body to Value body for compatibility with existing runtime
        CoreExpr::Lambda { params, body } => {
            // Convert Formals to (params, variadic) format
            let (param_names, variadic) = formals_to_params(params)?;

            // Convert CoreExpr body to Value body
            // This is a temporary bridge - ideally we'd store CoreExpr in closures
            let body_values: Result<Vec<Value>, EvalError> =
                body.iter().map(core_expr_to_value).collect();
            let body_values = body_values?;

            Ok(CoreEvalResult::Value(Value::Procedure(Procedure::Lambda {
                params: param_names,
                variadic,
                body: body_values,
                env: env.clone(),
            })))
        }

        // If: evaluate test, then branch based on result
        // The selected branch is in TAIL POSITION
        CoreExpr::If { test, then, else_ } => {
            // Test is NOT in tail position - must evaluate fully
            let test_val = match eval_core_step(test, env.clone(), evaluator)? {
                CoreEvalResult::Value(v) => v,
                CoreEvalResult::TailCall { expr, env } => {
                    // Test resulted in tail call - trampoline it
                    eval_core(&expr, env, evaluator)?
                }
            };

            // In Scheme, only #f is false, everything else is true
            let branch = if matches!(test_val, Value::Boolean(false)) {
                else_
            } else {
                then
            };

            // Branch is in tail position - return TailCall
            Ok(CoreEvalResult::TailCall {
                expr: (**branch).clone(),
                env,
            })
        }

        // Set!: mutate existing binding
        // Value is NOT in tail position
        CoreExpr::Set { var, value } => {
            let val = eval_non_tail(value, env.clone(), evaluator)?;
            env.set(var.as_ref(), val)
                .map_err(|_| EvalError::UndefinedVariable(var.to_string()))?;
            Ok(CoreEvalResult::Value(Value::Unspecified))
        }

        // Define: create or update top-level binding
        // Value is NOT in tail position
        CoreExpr::Define { name, value } => {
            let val = eval_non_tail(value, env.clone(), evaluator)?;
            env.define(name.to_string(), val);
            Ok(CoreEvalResult::Value(Value::Unspecified))
        }

        // DefineSyntax: compile transformer and install macro
        // The transformer is typically (syntax-rules ...) which needs special handling
        CoreExpr::DefineSyntax { name, transformer } => {
            // Transformer is already a Value (stored as data, not desugared code)
            // Compile the syntax-rules transformer
            let compiled_macro = evaluator.compile_syntax_rules(transformer, name.clone(), &env)?;

            // Store the compiled macro
            let macro_value = Value::Macro {
                name: name.clone(),
                data: Rc::new(compiled_macro),
            };

            // Bind the macro in the environment
            env.define(name.to_string(), macro_value);
            Ok(CoreEvalResult::Value(Value::Unspecified))
        }

        // Import: load library bindings into environment
        CoreExpr::Import { import_sets } => {
            // Process each import set
            for import_set_expr in import_sets {
                // Parse the import set
                let import_set = patina_frontend::LibraryDefinition::parse_import_set(
                    import_set_expr,
                )
                .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;

                // Process the import set: this will import the identifiers into env
                evaluator.process_import_for_eval(&import_set, &env)?;
            }

            Ok(CoreEvalResult::Value(Value::Unspecified))
        }

        // Parameterize: dynamically rebind parameters
        // Body is NOT in tail position (TCO disabled for proper stack cleanup)
        CoreExpr::Parameterize { bindings, body } => {
            // Evaluate bindings and push new values onto parameter stacks
            let mut params = Vec::new();

            for (param_expr, value_expr) in bindings {
                // Evaluate param expression
                let param = eval_non_tail(param_expr, env.clone(), evaluator)?;

                // Verify it's a parameter
                match &param {
                    Value::Parameter { values, converter } => {
                        // Evaluate value expression
                        let new_val = eval_non_tail(value_expr, env.clone(), evaluator)?;

                        // Apply converter if present
                        let converted_val = if let Some(conv) = converter {
                            match evaluator.apply(*conv.clone(), vec![new_val.clone()], false)? {
                                super::EvalResult::Value(v) => v,
                                _ => {
                                    return Err(EvalError::InvalidSyntax(
                                        "parameter converter returned non-value".to_string(),
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

            // Evaluate body expressions in sequence (NOT in tail position)
            // We can't use TCO here because we need to pop the parameter stack
            // AFTER the body completes, not before.
            let mut result = Value::Unspecified;
            for expr in body {
                result = eval_non_tail(expr, env.clone(), evaluator)?;
            }

            // Pop parameter values from stack (restore to previous state)
            for param in &params {
                if let Value::Parameter { values, .. } = param {
                    values.borrow_mut().pop();
                }
            }

            Ok(CoreEvalResult::Value(result))
        }

        // Begin: evaluate expressions in sequence, return last
        // Last expression is in TAIL POSITION
        CoreExpr::Begin(exprs) => {
            if exprs.is_empty() {
                return Err(EvalError::InvalidSyntax("Empty begin".to_string()));
            }

            // Evaluate all but last expression for side effects (non-tail)
            for expr in &exprs[..exprs.len() - 1] {
                eval_non_tail(expr, env.clone(), evaluator)?;
            }

            // Last expression is in tail position - return TailCall
            Ok(CoreEvalResult::TailCall {
                expr: exprs[exprs.len() - 1].clone(),
                env,
            })
        }

        // Application: evaluate function and arguments, then apply
        // The application itself is in TAIL POSITION (if it's a lambda call)
        CoreExpr::App { func, args } => {
            // Function and arguments are NOT in tail position
            let func_val = eval_non_tail(func, env.clone(), evaluator)?;

            // Evaluate all arguments
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(eval_non_tail(arg, env.clone(), evaluator)?);
            }

            // Apply procedure - this is in tail position
            // apply_procedure will handle TCO for lambda calls
            apply_procedure(&func_val, &arg_vals, env, evaluator)
        }

        // Apply special form: (apply proc arg1 ... argN list)
        CoreExpr::Apply { func, args } => {
            // Evaluate function and all arguments
            let func_val = eval_non_tail(func, env.clone(), evaluator)?;

            let mut evaluated_args = Vec::new();
            for arg in args {
                evaluated_args.push(eval_non_tail(arg, env.clone(), evaluator)?);
            }

            // Last argument must be a list - convert to Vec
            if evaluated_args.is_empty() {
                return Err(EvalError::WrongArity {
                    expected: "at least 1 (the list)".to_string(),
                    actual: 0,
                });
            }

            let list_arg = evaluated_args.pop().unwrap();
            let mut final_args = evaluated_args; // Middle args

            // Convert the list to individual arguments
            let mut current = list_arg;
            loop {
                match current {
                    Value::Null => break,
                    Value::Pair(pair) => {
                        let (car, cdr) = pair.borrow().clone();
                        final_args.push(car);
                        current = cdr;
                    }
                    _ => {
                        return Err(EvalError::InvalidSyntax(
                            "apply: last argument must be a proper list".to_string(),
                        ));
                    }
                }
            }

            // Apply procedure - this is in tail position
            apply_procedure(&func_val, &final_args, env, evaluator)
        }

        // Optimized forms (not yet generated by desugarer)
        CoreExpr::PrimCall { prim, .. } => Err(EvalError::InternalError(format!(
            "PrimCall optimization not yet implemented: {:?}",
            prim
        ))),

        CoreExpr::Let { .. } => Err(EvalError::InternalError(
            "Let optimization not yet implemented".to_string(),
        )),
    }
}

/// Apply a procedure to arguments
///
/// This delegates to the existing application logic in the tree-walker.
/// For now, we just handle the basic case and will integrate with the
/// full application module later.
fn apply_procedure(
    proc: &Value,
    args: &[Value],
    _env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<CoreEvalResult, EvalError> {
    match proc {
        Value::Procedure(Procedure::Lambda {
            params,
            variadic,
            body,
            env: closure_env,
        }) => {
            // Create new environment extending closure environment
            let call_env = Rc::new(Environment::with_parent(closure_env.clone()));

            // Bind parameters
            bind_params(params, variadic.as_ref(), args, &call_env)?;

            // Evaluate body expressions in sequence
            if body.is_empty() {
                return Err(EvalError::InvalidSyntax("Empty lambda body".to_string()));
            }

            // Evaluate all but last for side effects
            for expr in &body[..body.len() - 1] {
                // Since body is Vec<Value>, we need to evaluate it using the Value evaluator
                // TODO: This is a temporary bridge - we should use CoreExpr bodies
                // For now, we use a minimal evaluation strategy
                eval_value_simple(expr, call_env.clone(), evaluator)?;
            }

            // Last expression is in tail position - convert to CoreExpr and return TailCall
            let last_expr_value = &body[body.len() - 1];
            // Use desugarer with environment to handle macros in lambda bodies
            let desugarer = patina_frontend::Desugarer::with_env(call_env.clone());
            let last_expr_core = desugarer
                .desugar(last_expr_value)
                .map_err(|e| EvalError::InvalidSyntax(format!("Desugaring failed: {}", e)))?;

            Ok(CoreEvalResult::TailCall {
                expr: last_expr_core,
                env: call_env,
            })
        }

        Value::Procedure(procedure @ Procedure::Primitive { .. }) => {
            // Delegate to the evaluator's primitive registry
            // We pass in_tail_position=true to enable TCO for primitives like call-with-values
            match evaluator.apply_primitive(procedure, args.to_vec(), true)? {
                super::EvalResult::Value(v) => Ok(CoreEvalResult::Value(v)),
                super::EvalResult::TailCall { expr, env } => {
                    // Primitive returned a tail call (e.g., call-with-values)
                    // Convert Value to CoreExpr and return TailCall
                    // Use desugarer with environment to handle macros
                    let desugarer = patina_frontend::Desugarer::with_env(env.clone());
                    let core_expr = desugarer.desugar(&expr).map_err(|e| {
                        EvalError::InvalidSyntax(format!("Desugaring failed: {}", e))
                    })?;
                    Ok(CoreEvalResult::TailCall {
                        expr: core_expr,
                        env,
                    })
                }
                super::EvalResult::TailCallPrimitive { proc, args } => {
                    // Primitive wants to tail-call another procedure
                    // Apply it and return the result
                    apply_procedure(&proc, &args, _env, evaluator)
                }
            }
        }

        Value::Procedure(Procedure::CaseLambda {
            clauses,
            env: case_env,
        }) => {
            // case-lambda: try each clause in order to find one that matches the argument count
            for (params, variadic, body) in clauses {
                let matches = if variadic.is_some() {
                    // Variadic clause: need at least as many args as fixed params
                    args.len() >= params.len()
                } else {
                    // Fixed arity clause: need exact number of args
                    args.len() == params.len()
                };

                if matches {
                    // Found a matching clause - create new environment and evaluate
                    let call_env = Rc::new(Environment::with_parent(case_env.clone()));
                    bind_params(params, variadic.as_ref(), args, &call_env)?;

                    // Evaluate body (Vec<Value>) using eval_value_simple
                    if body.is_empty() {
                        return Err(EvalError::InvalidSyntax(
                            "Empty case-lambda body".to_string(),
                        ));
                    }

                    // Evaluate all but last for side effects
                    for expr in &body[..body.len() - 1] {
                        eval_value_simple(expr, call_env.clone(), evaluator)?;
                    }

                    // Last expression is in tail position
                    let last_expr_value = &body[body.len() - 1];
                    // Use desugarer with environment to handle macros in lambda bodies
                    let desugarer = patina_frontend::Desugarer::with_env(call_env.clone());
                    let last_expr_core = desugarer.desugar(last_expr_value).map_err(|e| {
                        EvalError::InvalidSyntax(format!("Desugaring failed: {}", e))
                    })?;

                    return Ok(CoreEvalResult::TailCall {
                        expr: last_expr_core,
                        env: call_env,
                    });
                }
            }

            // No matching clause found
            Err(EvalError::WrongArity {
                expected: format!("case-lambda: no clause matches {} arguments", args.len()),
                actual: args.len(),
            })
        }

        Value::Parameter { .. } => {
            // Parameters are handled by the Value evaluator's apply function
            // Delegate to it (not in tail position since parameters just get/set values)
            match evaluator.apply(proc.clone(), args.to_vec(), false)? {
                super::EvalResult::Value(v) => Ok(CoreEvalResult::Value(v)),
                _ => Err(EvalError::InvalidSyntax(
                    "parameter application returned unexpected result".to_string(),
                )),
            }
        }

        _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
    }
}

/// Bind parameters to arguments
fn bind_params(
    params: &[String],
    variadic: Option<&String>,
    args: &[Value],
    env: &Environment,
) -> Result<(), EvalError> {
    let required = params.len();

    match variadic {
        None => {
            // Fixed arity
            if args.len() != required {
                return Err(EvalError::WrongArity {
                    expected: required.to_string(),
                    actual: args.len(),
                });
            }
            for (param, arg) in params.iter().zip(args.iter()) {
                env.define(param.clone(), arg.clone());
            }
        }
        Some(rest_param) => {
            // Variadic
            if args.len() < required {
                return Err(EvalError::WrongArity {
                    expected: format!("at least {}", required),
                    actual: args.len(),
                });
            }
            // Bind required parameters
            for (param, arg) in params.iter().zip(args.iter()) {
                env.define(param.clone(), arg.clone());
            }
            // Bind rest parameters as list
            let rest_args = &args[required..];
            let rest_list = args_to_list(rest_args);
            env.define(rest_param.clone(), rest_list);
        }
    }

    Ok(())
}

/// Convert Formals to (params, variadic) format for runtime
fn formals_to_params(formals: &Formals) -> Result<(Vec<String>, Option<String>), EvalError> {
    match formals {
        Formals::Fixed(params) => {
            let names = params.iter().map(|p| p.to_string()).collect();
            Ok((names, None))
        }
        Formals::Variadic(param) => Ok((vec![], Some(param.to_string()))),
        Formals::Mixed { fixed, rest } => {
            let names = fixed.iter().map(|p| p.to_string()).collect();
            Ok((names, Some(rest.to_string())))
        }
    }
}

/// Convert argument vector to Scheme list
fn args_to_list(args: &[Value]) -> Value {
    let mut result = Value::Null;
    for arg in args.iter().rev() {
        result = cons(arg.clone(), result);
    }
    result
}

/// Convert CoreExpr to Value (temporary bridge)
///
/// This is a hack to work with the existing runtime that stores lambda bodies as Vec<Value>.
/// Ideally we'd have a CoreClosure type that stores CoreExpr bodies directly.
///
/// For now, we convert the CoreExpr back to an equivalent Value representation.
fn core_expr_to_value(expr: &CoreExpr) -> Result<Value, EvalError> {
    match expr {
        CoreExpr::Literal(v) => Ok(v.clone()),
        CoreExpr::Var(name) => Ok(Value::Symbol(name.clone())),
        CoreExpr::Quote(v) => {
            // (quote datum)
            Ok(cons(
                Value::Symbol(Rc::from("quote")),
                cons(v.clone(), Value::Null),
            ))
        }
        CoreExpr::Quasiquote(template) => {
            // (quasiquote template)
            Ok(cons(
                Value::Symbol(Rc::from("quasiquote")),
                cons(template.clone(), Value::Null),
            ))
        }
        CoreExpr::Lambda { params, body } => {
            // (lambda params body...)
            let params_value = formals_to_value_list(params);
            let body_values: Result<Vec<Value>, _> = body.iter().map(core_expr_to_value).collect();
            let body_list = vec_to_list(&body_values?);

            Ok(cons(
                Value::Symbol(Rc::from("lambda")),
                cons(params_value, body_list),
            ))
        }
        CoreExpr::If { test, then, else_ } => {
            // (if test then else)
            let test_val = core_expr_to_value(test)?;
            let then_val = core_expr_to_value(then)?;
            let else_val = core_expr_to_value(else_)?;

            Ok(cons(
                Value::Symbol(Rc::from("if")),
                cons(test_val, cons(then_val, cons(else_val, Value::Null))),
            ))
        }
        CoreExpr::Set { var, value } => {
            // (set! var value)
            let value_val = core_expr_to_value(value)?;
            Ok(cons(
                Value::Symbol(Rc::from("set!")),
                cons(Value::Symbol(var.clone()), cons(value_val, Value::Null)),
            ))
        }
        CoreExpr::Define { name, value } => {
            // (define name value)
            let value_val = core_expr_to_value(value)?;
            Ok(cons(
                Value::Symbol(Rc::from("define")),
                cons(Value::Symbol(name.clone()), cons(value_val, Value::Null)),
            ))
        }
        CoreExpr::DefineSyntax { name, transformer } => {
            // (define-syntax name transformer)
            // Transformer is already a Value (template data)
            Ok(cons(
                Value::Symbol(Rc::from("define-syntax")),
                cons(
                    Value::Symbol(name.clone()),
                    cons(transformer.clone(), Value::Null),
                ),
            ))
        }
        CoreExpr::Import { import_sets } => {
            // (import import-set ...)
            let import_list = vec_to_list(import_sets);
            Ok(cons(Value::Symbol(Rc::from("import")), import_list))
        }
        CoreExpr::Parameterize { bindings, body } => {
            // (parameterize ((param val) ...) body ...)
            let bindings_values: Result<Vec<Value>, EvalError> = bindings
                .iter()
                .map(|(param, val)| {
                    let param_val = core_expr_to_value(param)?;
                    let val_val = core_expr_to_value(val)?;
                    Ok(cons(param_val, cons(val_val, Value::Null)))
                })
                .collect();
            let bindings_list = vec_to_list(&bindings_values?);

            let body_values: Result<Vec<Value>, EvalError> =
                body.iter().map(core_expr_to_value).collect();
            let body_list = vec_to_list(&body_values?);

            Ok(cons(
                Value::Symbol(Rc::from("parameterize")),
                cons(bindings_list, body_list),
            ))
        }
        CoreExpr::Begin(exprs) => {
            // (begin expr...)
            let expr_values: Result<Vec<Value>, _> = exprs.iter().map(core_expr_to_value).collect();
            let expr_list = vec_to_list(&expr_values?);

            Ok(cons(Value::Symbol(Rc::from("begin")), expr_list))
        }
        CoreExpr::App { func, args } => {
            // (func arg...)
            let func_val = core_expr_to_value(func)?;
            let arg_values: Result<Vec<Value>, _> = args.iter().map(core_expr_to_value).collect();

            let mut result = vec_to_list(&arg_values?);
            result = cons(func_val, result);
            Ok(result)
        }
        CoreExpr::Apply { func, args } => {
            let func_val = core_expr_to_value(func)?;
            let arg_values: Result<Vec<Value>, _> = args.iter().map(core_expr_to_value).collect();

            let mut result = vec_to_list(&arg_values?);
            result = cons(func_val, result);
            result = cons(Value::Symbol(Rc::from("apply")), result);
            Ok(result)
        }
        CoreExpr::PrimCall { .. } | CoreExpr::Let { .. } => Err(EvalError::InternalError(
            "Cannot convert optimized CoreExpr to Value".to_string(),
        )),
    }
}

/// Convert Formals to Value parameter list
fn formals_to_value_list(formals: &Formals) -> Value {
    match formals {
        Formals::Fixed(params) => {
            let symbols: Vec<Value> = params.iter().map(|p| Value::Symbol(p.clone())).collect();
            vec_to_list(&symbols)
        }
        Formals::Variadic(param) => Value::Symbol(param.clone()),
        Formals::Mixed { fixed, rest } => {
            // Create dotted list: (x y . rest)
            let mut result = Value::Symbol(rest.clone());
            for param in fixed.iter().rev() {
                result = cons(Value::Symbol(param.clone()), result);
            }
            result
        }
    }
}

/// Convert vector to Scheme list
fn vec_to_list(items: &[Value]) -> Value {
    let mut result = Value::Null;
    for item in items.iter().rev() {
        result = cons(item.clone(), result);
    }
    result
}

/// Minimal Value evaluator (temporary bridge)
///
/// This is a simplified evaluator for Value expressions that appear in lambda bodies.
/// It's a temporary bridge until we migrate to storing CoreExpr bodies in closures.
///
/// For now, this just converts Value → CoreExpr → evaluate via eval_core.
/// This is inefficient but correct, and isolated to this one place.
fn eval_value_simple(
    value: &Value,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    // Use the Value evaluator directly instead of converting to CoreExpr
    // This is important because lambda bodies may contain special forms
    // like quasiquote that aren't in CoreExpr
    evaluator.eval_in_env(value, &env)
}

/// Minimal Value → CoreExpr converter (temporary bridge)
/// Implementation of quasiquote with depth tracking
///
/// The depth parameter tracks nesting level:
/// - depth 0: at current quasiquote level (unquotes are active)
/// - depth > 0: inside nested quasiquote (unquotes become quoted)
fn eval_quasiquote_impl(
    evaluator: &super::Evaluator,
    expr: &Value,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<Value, EvalError> {
    match expr {
        // Self-evaluating values: return as-is
        Value::Boolean(_)
        | Value::Integer(_)
        | Value::BigInteger(_)
        | Value::Rational(_)
        | Value::Real(_)
        | Value::Complex(_, _)
        | Value::Character(_)
        | Value::String(_)
        | Value::Bytevector(_)
        | Value::Unspecified => Ok(expr.clone()),

        // Symbols and null: quote them (return as-is)
        Value::Symbol(_) | Value::Null => Ok(expr.clone()),

        // Vectors: convert to list, process, convert back
        Value::Vector(vec) => {
            let list = vector_to_list(&vec.borrow());
            let processed = eval_quasiquote_impl(evaluator, &list, env, depth)?;
            list_to_vector(&processed)
        }

        // Pairs: the interesting case
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let car = &borrowed.0;
            let cdr = &borrowed.1;

            // Check if car is a symbol that requires special handling
            if let Value::Symbol(sym) = car {
                match sym.as_ref() {
                    // Nested quasiquote: increment depth
                    "quasiquote" => {
                        let (inner, rest) = extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "quasiquote expects exactly one argument".to_string(),
                            ));
                        }
                        let processed = eval_quasiquote_impl(evaluator, &inner, env, depth + 1)?;
                        Ok(list_from_vec(vec![
                            Value::Symbol(Rc::from("quasiquote")),
                            processed,
                        ]))
                    }

                    // Unquote: evaluate if at depth 0, otherwise decrement depth
                    "unquote" => {
                        let (inner, rest) = extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }

                        if depth == 0 {
                            // At quasiquote level: evaluate the unquoted expression
                            evaluator.eval_in_env(&inner, env)
                        } else {
                            // Inside nested quasiquote: preserve unquote, decrement depth
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![
                                Value::Symbol(Rc::from("unquote")),
                                processed,
                            ]))
                        }
                    }

                    // Unquote-splicing: can't appear at top level of quasiquote
                    "unquote-splicing" => {
                        if depth == 0 {
                            Err(EvalError::InvalidSyntax(
                                "unquote-splicing not in list context".to_string(),
                            ))
                        } else {
                            // Inside nested quasiquote: preserve, decrement depth
                            let (inner, rest) = extract_pair(cdr)?;
                            if !matches!(rest, Value::Null) {
                                return Err(EvalError::InvalidSyntax(
                                    "unquote-splicing expects exactly one argument".to_string(),
                                ));
                            }
                            let processed =
                                eval_quasiquote_impl(evaluator, &inner, env, depth - 1)?;
                            Ok(list_from_vec(vec![
                                Value::Symbol(Rc::from("unquote-splicing")),
                                processed,
                            ]))
                        }
                    }

                    _ => {
                        // Regular symbol: process as normal pair
                        process_quasiquote_pair(evaluator, expr, env, depth)
                    }
                }
            } else {
                // Non-symbol car: process as normal pair
                process_quasiquote_pair(evaluator, expr, env, depth)
            }
        }

        // Other types: return as-is
        _ => Ok(expr.clone()),
    }
}

/// Process a regular pair in quasiquote context
///
/// This handles the case where we have a list that might contain unquote-splicing
fn process_quasiquote_pair(
    evaluator: &super::Evaluator,
    expr: &Value,
    env: &Rc<Environment>,
    depth: i32,
) -> Result<Value, EvalError> {
    // Convert to vector for easier manipulation
    let mut elements = Vec::new();
    let mut current = expr.clone();
    let mut tail = Value::Null; // For improper lists

    // Walk the list
    loop {
        match current {
            Value::Null => break,
            Value::Pair(ref pair) => {
                let (car, cdr) = {
                    let borrowed = pair.borrow();
                    (borrowed.0.clone(), borrowed.1.clone())
                };

                // Check if this element is (unquote-splicing ...)
                if depth == 0
                    && let Value::Pair(ref inner_pair) = car
                {
                    let (inner_car, inner_cdr) = {
                        let b = inner_pair.borrow();
                        (b.0.clone(), b.1.clone())
                    };
                    if let Value::Symbol(ref sym) = inner_car
                        && sym.as_ref() == "unquote-splicing"
                    {
                        // Evaluate the splicing expression
                        let (splice_expr, rest) = extract_pair(&inner_cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing expects exactly one argument".to_string(),
                            ));
                        }

                        let splice_result = evaluator.eval_in_env(&splice_expr, env)?;

                        // Must be a list
                        if !is_list(&splice_result) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing result must be a list".to_string(),
                            ));
                        }

                        // Append all elements from the spliced list
                        let mut splice_current = splice_result;
                        while let Value::Pair(ref sp) = splice_current {
                            let (sc, sn) = {
                                let b = sp.borrow();
                                (b.0.clone(), b.1.clone())
                            };
                            elements.push(sc);
                            splice_current = sn;
                        }

                        // After splicing, check if CDR is an unquote form for improper list tail
                        if let Value::Pair(ref cdr_pair) = cdr {
                            let (cdr_car, cdr_cdr) = {
                                let b = cdr_pair.borrow();
                                (b.0.clone(), b.1.clone())
                            };
                            if let Value::Symbol(ref sym) = cdr_car
                                && sym.as_ref() == "unquote"
                            {
                                // Evaluate the unquote expression as the tail
                                let (unquote_expr, rest) = extract_pair(&cdr_cdr)?;
                                if !matches!(rest, Value::Null) {
                                    return Err(EvalError::InvalidSyntax(
                                        "unquote expects exactly one argument".to_string(),
                                    ));
                                }
                                tail = evaluator.eval_in_env(&unquote_expr, env)?;
                                break;
                            }
                        }

                        current = cdr;
                        continue;
                    }
                }

                // Check if CDR is an unquote form (for improper lists like (a . ,x))
                if depth == 0
                    && let Value::Pair(ref cdr_pair) = cdr
                {
                    let (cdr_car, cdr_cdr) = {
                        let b = cdr_pair.borrow();
                        (b.0.clone(), b.1.clone())
                    };
                    if let Value::Symbol(ref sym) = cdr_car
                        && sym.as_ref() == "unquote"
                    {
                        // This is an improper list: (... car . ,expr)
                        // Process car normally, then evaluate the unquote as tail
                        let processed_car = eval_quasiquote_impl(evaluator, &car, env, depth)?;
                        elements.push(processed_car);

                        // Evaluate the unquote expression
                        let (unquote_expr, rest) = extract_pair(&cdr_cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }
                        tail = evaluator.eval_in_env(&unquote_expr, env)?;
                        break;
                    }
                }

                // Regular element: process recursively
                let processed = eval_quasiquote_impl(evaluator, &car, env, depth)?;
                elements.push(processed);
                current = cdr;
            }
            _ => {
                // Improper list (dotted pair with non-list tail)
                tail = eval_quasiquote_impl(evaluator, &current, env, depth)?;
                break;
            }
        }
    }

    // Reconstruct the list
    if matches!(tail, Value::Null) {
        Ok(list_from_vec(elements))
    } else {
        // Improper list
        let mut result = tail;
        for elem in elements.iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((elem.clone(), result))));
        }
        Ok(result)
    }
}

/// Helper: extract pair into (car, cdr)
fn extract_pair(value: &Value) -> Result<(Value, Value), EvalError> {
    match value {
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            Ok((borrowed.0.clone(), borrowed.1.clone()))
        }
        _ => Err(EvalError::InvalidSyntax("Expected pair".to_string())),
    }
}

/// Helper: check if a value is a proper list
fn is_list(val: &Value) -> bool {
    let mut current = val.clone();
    loop {
        match current {
            Value::Null => return true,
            Value::Pair(ref pair) => {
                let next = pair.borrow().1.clone();
                current = next;
            }
            _ => return false,
        }
    }
}

/// Helper: convert vector to list
fn vector_to_list(vec: &[Value]) -> Value {
    list_from_vec(vec.to_vec())
}

/// Helper: convert list to vector
fn list_to_vector(list: &Value) -> Result<Value, EvalError> {
    let mut vec = Vec::new();
    let mut current = list.clone();

    loop {
        match current {
            Value::Null => break,
            Value::Pair(ref pair) => {
                let (car, cdr) = {
                    let borrowed = pair.borrow();
                    (borrowed.0.clone(), borrowed.1.clone())
                };
                vec.push(car);
                current = cdr;
            }
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Cannot convert improper list to vector".to_string(),
                ));
            }
        }
    }

    Ok(Value::Vector(Rc::new(RefCell::new(vec))))
}

/// Helper: construct list from vector
fn list_from_vec(vec: Vec<Value>) -> Value {
    let mut result = Value::Null;
    for item in vec.into_iter().rev() {
        result = Value::Pair(Rc::new(RefCell::new((item, result))));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_literal() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Literal(Value::Integer(42));

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_quote() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Quote(Value::Symbol(Rc::from("x")));

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn test_eval_variable() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), Value::Integer(42));

        let expr = CoreExpr::Var(Rc::from("x"));
        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_variable_unbound() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Var(Rc::from("undefined"));

        let result = eval_core(&expr, env, &evaluator);
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_if_true() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Box::new(CoreExpr::Literal(Value::Boolean(true))),
            then: Box::new(CoreExpr::Literal(Value::Integer(1))),
            else_: Box::new(CoreExpr::Literal(Value::Integer(2))),
        };

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(1)));
    }

    #[test]
    fn test_eval_if_false() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Box::new(CoreExpr::Literal(Value::Boolean(false))),
            then: Box::new(CoreExpr::Literal(Value::Integer(1))),
            else_: Box::new(CoreExpr::Literal(Value::Integer(2))),
        };

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(2)));
    }

    #[test]
    fn test_eval_define() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Define {
            name: Rc::from("x"),
            value: Box::new(CoreExpr::Literal(Value::Integer(42))),
        };

        let result = eval_core(&expr, env.clone(), &evaluator).unwrap();
        assert!(matches!(result, Value::Unspecified));

        // Check that variable was defined
        let x_val = env.get(&Rc::from("x")).unwrap();
        assert!(matches!(x_val, Value::Integer(42)));
    }

    #[test]
    fn test_eval_set() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), Value::Integer(1));

        let expr = CoreExpr::Set {
            var: Rc::from("x"),
            value: Box::new(CoreExpr::Literal(Value::Integer(42))),
        };

        let result = eval_core(&expr, env.clone(), &evaluator).unwrap();
        assert!(matches!(result, Value::Unspecified));

        // Check that variable was updated
        let x_val = env.get(&Rc::from("x")).unwrap();
        assert!(matches!(x_val, Value::Integer(42)));
    }

    #[test]
    fn test_eval_begin() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Begin(vec![
            CoreExpr::Literal(Value::Integer(1)),
            CoreExpr::Literal(Value::Integer(2)),
            CoreExpr::Literal(Value::Integer(3)),
        ]);

        let result = eval_core(&expr, env, &evaluator).unwrap();
        // Should return last value
        assert!(matches!(result, Value::Integer(3)));
    }

    #[test]
    fn test_eval_lambda() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Lambda {
            params: Formals::Fixed(vec![Rc::from("x")]),
            body: vec![CoreExpr::Var(Rc::from("x"))],
        };

        let result = eval_core(&expr, env, &evaluator).unwrap();
        // Should return a procedure
        assert!(matches!(result, Value::Procedure(_)));
    }
}
