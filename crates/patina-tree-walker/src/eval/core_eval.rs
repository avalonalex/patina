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
pub(crate) enum CoreEvalResult {
    /// Final value - evaluation complete
    Value(Value),
    /// Tail call - continue with this expression and environment
    /// Uses Rc<CoreExpr> to share expressions without cloning (~80 bytes → 8 bytes)
    TailCall {
        expr: Rc<CoreExpr>,
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
        CoreEvalResult::TailCall { expr, env } => eval_core_rc(expr, env, evaluator),
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
    // Start with the expression by reference, trampoline if needed
    match eval_core_step(expr, env.clone(), evaluator)? {
        CoreEvalResult::Value(v) => Ok(v),
        CoreEvalResult::TailCall { expr, env } => eval_core_rc(expr, env, evaluator),
    }
}

/// Evaluate a CoreExpr from an Rc (used by trampoline)
///
/// This version takes ownership of an Rc<CoreExpr>, avoiding unnecessary cloning.
/// The trampoline loop uses Rc to share expressions efficiently.
fn eval_core_rc(
    expr: Rc<CoreExpr>,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<Value, EvalError> {
    let mut current_expr = expr;
    let mut current_env = env;

    // Trampoline loop for TCO - uses Rc for efficient sharing (8 bytes per iteration)
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
pub(crate) fn eval_core_step(
    expr: &CoreExpr,
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
) -> Result<CoreEvalResult, EvalError> {
    match expr {
        // Literals evaluate to themselves (dereference the Rc)
        // For compound data (pairs, vectors) that may contain Identifiers from macro expansion
        // (pattern variable substitution marks symbols with scopes), strip them to plain Symbols
        // for proper comparison with eq?, eqv?, equal?, etc.
        CoreExpr::Literal(v) => {
            let value = v.as_ref().clone();
            // Strip Identifiers from compound data structures (lists and vectors)
            let result = match &value {
                Value::Pair(_) | Value::Vector(_) => strip_identifiers_to_symbols(&value),
                _ => value,
            };
            Ok(CoreEvalResult::Value(result))
        }

        // Variables: look up in environment (with optional hygiene scopes)
        CoreExpr::Var { name, scopes } => {
            if scopes.is_empty() {
                // Simple lookup for non-macro code
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[EVAL] Looking up Var '{}' (simple lookup)", name);
                }
                env.get(name)
                    .map(CoreEvalResult::Value)
                    .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
            } else {
                // Scope-based lookup for hygienic macros
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[EVAL] Looking up Var '{}' with scopes {}", name, scopes);
                }
                env.get_with_scopes(name, scopes)
                    .map(CoreEvalResult::Value)
                    .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))
            }
        }

        // Quote: return the quoted value, converting Identifiers to Symbols
        // After macro expansion, quoted data may contain Identifiers with scopes.
        // These should be converted to plain Symbols for proper comparison (eq?, eqv?, memq, etc.)
        CoreExpr::Quote(v) => {
            let stripped = strip_identifiers_to_symbols(v.as_ref());
            Ok(CoreEvalResult::Value(stripped))
        }

        // Quasiquote: template with selective evaluation
        CoreExpr::Quasiquote(template) => {
            let result = eval_quasiquote_impl(evaluator, template, &env, 0)?;
            Ok(CoreEvalResult::Value(result))
        }

        // Lambda: create a closure
        // NOTE: We store BOTH Value and CoreExpr body
        // - Value body for backward compatibility with legacy code paths
        // - CoreExpr body for direct evaluation (preserves scope IDs)
        CoreExpr::Lambda {
            params,
            body,
            binding_scope,
        } => {
            // Convert Formals to (params, variadic) format, preserving scopes!
            let (scoped_params, variadic) = formals_to_scoped_params(params)?;

            // Store CoreExpr body directly - this preserves scope IDs for hygiene
            Ok(CoreEvalResult::Value(Value::Procedure(Rc::new(
                Procedure::Lambda {
                    params: scoped_params,
                    variadic,
                    body: body.clone(),
                    env: env.clone(),
                    binding_scope: *binding_scope,
                },
            ))))
        }

        // If: evaluate test, then branch based on result
        // The selected branch is in TAIL POSITION
        CoreExpr::If { test, then, else_ } => {
            // Test is NOT in tail position - must evaluate fully
            let test_val = match eval_core_step(test, env.clone(), evaluator)? {
                CoreEvalResult::Value(v) => v,
                CoreEvalResult::TailCall { expr, env } => {
                    // Test resulted in tail call - trampoline it
                    eval_core_rc(expr, env, evaluator)?
                }
            };

            // In Scheme, only #f is false, everything else is true
            let branch = if matches!(test_val, Value::Boolean(false)) {
                else_
            } else {
                then
            };

            // Branch is in tail position - return TailCall (just clone Rc, 8 bytes)
            Ok(CoreEvalResult::TailCall {
                expr: branch.clone(),
                env,
            })
        }

        // Set!: mutate existing binding (with optional hygiene scopes)
        // Value is NOT in tail position
        CoreExpr::Set { var, scopes, value } => {
            let val = eval_non_tail(value, env.clone(), evaluator)?;
            if scopes.is_empty() {
                // Simple lookup for non-macro code
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[EVAL] Set! '{}' (simple lookup)", var);
                }
                env.set(var.as_ref(), val)
                    .map_err(|_| EvalError::UndefinedVariable(var.to_string()))?;
            } else {
                // Scope-based lookup for hygienic macros
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[EVAL] Set! '{}' with scopes {}", var, scopes);
                }
                env.set_with_scopes(var.as_ref(), scopes, val)
                    .map_err(|_| EvalError::UndefinedVariable(var.to_string()))?;
            }
            Ok(CoreEvalResult::Value(Value::Unspecified))
        }

        // Define: create or update top-level binding
        // Value is NOT in tail position
        CoreExpr::Define { name, value } => {
            let val = eval_non_tail(value, env.clone(), evaluator)?;
            env.define(name.to_string(), val);
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
            // Note: wrapping in Rc since Begin stores Vec<CoreExpr> not Vec<Rc<CoreExpr>>
            Ok(CoreEvalResult::TailCall {
                expr: Rc::new(exprs[exprs.len() - 1].clone()),
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

        // Expand: show macro expansion without evaluating
        // This is a Patina debugging extension
        CoreExpr::Expand { expr } => {
            // Evaluate the argument to unwrap quotes
            // (expand '(do ...)) evaluates '(do ...) to get (do ...)
            let expr_val = eval_non_tail(expr, env.clone(), evaluator)?;

            // Check if expr is a macro call
            if let Value::Pair(p) = &expr_val {
                let p_ref = p.borrow();
                let car = &p_ref.0;
                if let Value::Symbol(sym) = car {
                    // Check if this symbol is bound to a macro
                    if let Some(Value::Macro(compiled_macro)) = env.get(sym) {
                        // Expand the macro
                        let expanded = patina_macros::expand_macro(
                            &compiled_macro,
                            &expr_val,
                            &env,
                        )
                        .map_err(|e| {
                            EvalError::InvalidSyntax(format!("Macro expansion failed: {}", e))
                        })?;
                        return Ok(CoreEvalResult::Value(expanded));
                    }
                }
            }

            // If not a macro, return the expression as-is
            Ok(CoreEvalResult::Value(expr_val))
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
        Value::Procedure(proc_box) => match proc_box.as_ref() {
            Procedure::Lambda {
                params,
                variadic,
                body,
                env: closure_env,
                binding_scope,
            } => {
                // Create new environment extending closure environment
                let call_env = Rc::new(Environment::with_parent(closure_env.clone()));

                // Bind parameters with proper hygiene (use parameter scopes if available)
                bind_scoped_params(params, variadic.as_ref(), args, &call_env, *binding_scope)?;

                // Evaluate CoreExpr body (preserves scope IDs for hygiene)
                if patina_runtime::macro_debug::is_enabled() {
                    println!("[APPLY] Lambda with CoreExpr body ({} exprs)", body.len());
                }
                if body.is_empty() {
                    return Err(EvalError::InvalidSyntax("Empty lambda body".to_string()));
                }

                // Evaluate all but last for side effects
                for expr in &body[..body.len() - 1] {
                    eval_non_tail(expr, call_env.clone(), evaluator)?;
                }

                // Last expression is in tail position
                Ok(CoreEvalResult::TailCall {
                    expr: Rc::new(body[body.len() - 1].clone()),
                    env: call_env,
                })
            }

            Procedure::Primitive { .. } => {
                // Delegate to the evaluator's primitive registry
                // We pass in_tail_position=true to enable TCO for primitives like call-with-values
                match evaluator.apply_primitive(proc_box.as_ref(), args.to_vec(), true)? {
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
                            expr: Rc::new(core_expr),
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

            Procedure::CpsLambda { .. } => {
                // CPS lambdas invoked from direct mode: use the CPS evaluator
                // to apply the procedure with a halt continuation.
                use crate::eval::cps_eval::CpsEvaluator;

                let cps_eval = CpsEvaluator::new(evaluator);
                let result = cps_eval.apply_from_direct(proc.clone(), args.to_vec())?;

                Ok(CoreEvalResult::Value(result))
            }
        },

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

/// Bind parameters to arguments with proper hygiene support
///
/// For each parameter:
/// - If the parameter has scopes (from macro expansion), use those scopes for binding
/// - If binding_scope is provided and parameter has no scopes, use that scope
/// - Always also create an unscoped binding for simple lookup compatibility
fn bind_scoped_params(
    params: &[patina_runtime::ScopedParam],
    variadic: Option<&patina_runtime::ScopedParam>,
    args: &[Value],
    env: &Environment,
    binding_scope: Option<patina_runtime::ScopeId>,
) -> Result<(), EvalError> {
    use patina_runtime::ScopeSet;

    let required = params.len();
    let debug = patina_runtime::macro_debug::is_enabled();

    // Helper to define bindings with proper scopes
    let define = |param: &patina_runtime::ScopedParam, value: Value| {
        let name = param.name.to_string();

        if debug {
            println!(
                "[BIND] Binding param '{}' with scopes {} (binding_scope: {:?})",
                name, param.scopes, binding_scope
            );
        }

        // Determine the scopes for this binding
        // Priority: use parameter's own scopes (from macro), else use binding_scope
        let scopes = if !param.scopes.is_empty() {
            // Parameter has scopes from macro expansion - use them!
            Some(param.scopes.clone())
        } else {
            // No macro scopes, use lambda's binding scope if available
            binding_scope.map(ScopeSet::singleton)
        };

        // IMPORTANT: For hygiene to work correctly:
        // - If parameter has scopes (macro-introduced), ONLY create scoped binding
        //   This ensures use-site references (no scopes) don't see it
        // - If parameter has no scopes, create simple binding for backward compatibility
        //   AND scoped binding if binding_scope is set
        if !param.scopes.is_empty() {
            // Macro-introduced parameter: ONLY scoped binding
            // This prevents it from shadowing use-site variables in simple lookup
            if debug {
                println!(
                    "[BIND]   -> Scoped-only binding for '{}' with scopes {}",
                    name,
                    scopes.as_ref().unwrap()
                );
            }
            env.define_with_scopes(name, scopes.unwrap(), value);
        } else {
            // Non-macro parameter: simple binding + optional scoped binding
            if debug {
                println!(
                    "[BIND]   -> Simple binding for '{}' + optional scoped",
                    name
                );
            }
            env.define(name.clone(), value.clone());
            if let Some(scope_set) = scopes {
                env.define_with_scopes(name, scope_set, value);
            }
        }
    };

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
                define(param, arg.clone());
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
                define(param, arg.clone());
            }
            // Bind rest parameters as list
            let rest_args = &args[required..];
            let rest_list = args_to_list(rest_args);
            define(rest_param, rest_list);
        }
    }

    Ok(())
}

/// Convert Formals to (params, variadic) format for runtime, preserving scopes
fn formals_to_scoped_params(
    formals: &Formals,
) -> Result<
    (
        Vec<patina_runtime::ScopedParam>,
        Option<patina_runtime::ScopedParam>,
    ),
    EvalError,
> {
    match formals {
        Formals::Fixed(params) => Ok((params.clone(), None)),
        Formals::Variadic(param) => Ok((vec![], Some(param.clone()))),
        Formals::Mixed { fixed, rest } => Ok((fixed.clone(), Some(rest.clone()))),
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
pub(crate) fn core_expr_to_value(expr: &CoreExpr) -> Result<Value, EvalError> {
    match expr {
        CoreExpr::Literal(v) => Ok(v.as_ref().clone()),
        CoreExpr::Var { name, scopes } => {
            if scopes.is_empty() {
                Ok(Value::Symbol(name.clone()))
            } else {
                Ok(Value::Identifier(Box::new(
                    patina_runtime::IdentifierData {
                        name: name.clone(),
                        scopes: scopes.clone(),
                    },
                )))
            }
        }
        CoreExpr::Quote(v) => {
            // (quote datum)
            Ok(cons(
                Value::symbol("quote"),
                cons(v.as_ref().clone(), Value::Null),
            ))
        }
        CoreExpr::Quasiquote(template) => {
            // (quasiquote template)
            Ok(cons(
                Value::symbol("quasiquote"),
                cons(template.as_ref().clone(), Value::Null),
            ))
        }
        CoreExpr::Lambda { params, body, .. } => {
            // (lambda params body...)
            // Note: binding_scope is not serialized to Value - it's only used at runtime
            let params_value = formals_to_value_list(params);
            let body_values: Result<Vec<Value>, _> = body.iter().map(core_expr_to_value).collect();
            let body_list = vec_to_list(&body_values?);

            Ok(cons(Value::symbol("lambda"), cons(params_value, body_list)))
        }
        CoreExpr::If { test, then, else_ } => {
            // (if test then else)
            let test_val = core_expr_to_value(test)?;
            let then_val = core_expr_to_value(then)?;
            let else_val = core_expr_to_value(else_)?;

            Ok(cons(
                Value::symbol("if"),
                cons(test_val, cons(then_val, cons(else_val, Value::Null))),
            ))
        }
        CoreExpr::Set { var, scopes, value } => {
            // (set! var value) or (set! var{scopes} value)
            let value_val = core_expr_to_value(value)?;
            let var_val = if scopes.is_empty() {
                Value::Symbol(var.clone())
            } else {
                Value::Identifier(Box::new(patina_runtime::IdentifierData {
                    name: var.clone(),
                    scopes: scopes.clone(),
                }))
            };
            Ok(cons(
                Value::symbol("set!"),
                cons(var_val, cons(value_val, Value::Null)),
            ))
        }
        CoreExpr::Define { name, value } => {
            // (define name value)
            let value_val = core_expr_to_value(value)?;
            Ok(cons(
                Value::symbol("define"),
                cons(Value::Symbol(name.clone()), cons(value_val, Value::Null)),
            ))
        }
        CoreExpr::Import { import_sets } => {
            // (import import-set ...)
            let import_list = vec_to_list(import_sets);
            Ok(cons(Value::symbol("import"), import_list))
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
                Value::symbol("parameterize"),
                cons(bindings_list, body_list),
            ))
        }
        CoreExpr::Begin(exprs) => {
            // (begin expr...)
            let expr_values: Result<Vec<Value>, _> = exprs.iter().map(core_expr_to_value).collect();
            let expr_list = vec_to_list(&expr_values?);

            Ok(cons(Value::symbol("begin"), expr_list))
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
            result = cons(Value::symbol("apply"), result);
            Ok(result)
        }
        CoreExpr::Expand { expr } => {
            // (expand expr)
            let expr_val = core_expr_to_value(expr)?;
            Ok(cons(Value::symbol("expand"), cons(expr_val, Value::Null)))
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
            let symbols: Vec<Value> = params
                .iter()
                .map(|p| Value::Symbol(p.name.clone()))
                .collect();
            vec_to_list(&symbols)
        }
        Formals::Variadic(param) => Value::Symbol(param.name.clone()),
        Formals::Mixed { fixed, rest } => {
            // Create dotted list: (x y . rest)
            let mut result = Value::Symbol(rest.name.clone());
            for param in fixed.iter().rev() {
                result = cons(Value::Symbol(param.name.clone()), result);
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

/// Strip Identifiers to Symbols in quoted data.
///
/// After macro expansion, quoted data may contain Identifier values (which have
/// hygiene scopes). For proper comparison with eq?, eqv?, memq, etc., these
/// should be converted back to plain Symbols.
///
/// This is necessary because:
/// 1. User writes '(a b c) expecting plain symbols
/// 2. Macro expansion may wrap these in Identifier with scopes
/// 3. When comparing with other symbols, Identifier != Symbol
fn strip_identifiers_to_symbols(value: &Value) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    match value {
        // Convert Identifier to Symbol
        Value::Identifier(id) => Value::Symbol(id.name.clone()),

        // Recursively process pairs (lists)
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let new_car = strip_identifiers_to_symbols(&borrowed.0);
            let new_cdr = strip_identifiers_to_symbols(&borrowed.1);
            Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
        }

        // Recursively process vectors
        Value::Vector(vec) => {
            let new_elements: Vec<_> = vec
                .borrow()
                .iter()
                .map(strip_identifiers_to_symbols)
                .collect();
            Value::Vector(Rc::new(RefCell::new(new_elements)))
        }

        // All other values pass through unchanged
        _ => value.clone(),
    }
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
        | Value::Complex(_)
        | Value::Character(_)
        | Value::String(_)
        | Value::Bytevector(_)
        | Value::Unspecified => Ok(expr.clone()),

        // Symbols and null: quote them (return as-is)
        // Identifiers (from macro expansion) are converted to Symbols for consistency
        Value::Symbol(_) | Value::Null => Ok(expr.clone()),
        Value::Identifier(id) => Ok(Value::Symbol(id.name.clone())),

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

            // Extract symbol name from either Symbol or Identifier
            // After macro expansion, special forms may be Identifiers with scopes
            let sym_name = match car {
                Value::Symbol(s) => Some(s.as_ref()),
                Value::Identifier(id) => Some(id.name.as_ref()),
                _ => None,
            };

            // Check if car is a symbol/identifier that requires special handling
            if let Some(name) = sym_name {
                match name {
                    // Nested quasiquote: increment depth
                    "quasiquote" => {
                        let (inner, rest) = extract_pair(cdr)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "quasiquote expects exactly one argument".to_string(),
                            ));
                        }
                        let processed = eval_quasiquote_impl(evaluator, &inner, env, depth + 1)?;
                        Ok(list_from_vec(vec![Value::symbol("quasiquote"), processed]))
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
                            Ok(list_from_vec(vec![Value::symbol("unquote"), processed]))
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
                                Value::symbol("unquote-splicing"),
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
                    if is_named(&inner_car, "unquote-splicing") {
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
                            if is_named(&cdr_car, "unquote") {
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
                    if is_named(&cdr_car, "unquote") {
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

/// Helper: check if a value is a symbol/identifier with a given name
/// After macro expansion, symbols may become Identifiers with scopes
fn is_named(value: &Value, name: &str) -> bool {
    match value {
        Value::Symbol(s) => s.as_ref() == name,
        Value::Identifier(id) => id.name.as_ref() == name,
        _ => false,
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

// =============================================================================
// Public API for CPS Evaluator
// =============================================================================

/// Evaluate a quasiquote template in the given environment
///
/// This is a public wrapper around `eval_quasiquote_impl` for use by the CPS evaluator.
/// Quasiquote evaluation doesn't involve continuations, so we can delegate to the
/// direct evaluator.
pub fn eval_quasiquote_in_env(
    evaluator: &super::Evaluator,
    template: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    eval_quasiquote_impl(evaluator, template, env, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_ir::ScopedParam;
    use patina_runtime::ScopeSet;

    #[test]
    fn test_eval_literal() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Literal(Rc::new(Value::Integer(42)));

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_quote() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Quote(Rc::new(Value::symbol("x")));

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn test_eval_variable() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        env.define("x".to_string(), Value::Integer(42));

        let expr = CoreExpr::Var {
            name: Rc::from("x"),
            scopes: ScopeSet::new(),
        };
        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_eval_variable_unbound() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::Var {
            name: Rc::from("undefined"),
            scopes: ScopeSet::new(),
        };

        let result = eval_core(&expr, env, &evaluator);
        assert!(result.is_err());
    }

    #[test]
    fn test_eval_if_true() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Rc::new(CoreExpr::Literal(Rc::new(Value::Boolean(true)))),
            then: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(1)))),
            else_: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(2)))),
        };

        let result = eval_core(&expr, env, &evaluator).unwrap();
        assert!(matches!(result, Value::Integer(1)));
    }

    #[test]
    fn test_eval_if_false() {
        let evaluator = super::super::Evaluator::new();
        let env = Rc::new(Environment::new());
        let expr = CoreExpr::If {
            test: Rc::new(CoreExpr::Literal(Rc::new(Value::Boolean(false)))),
            then: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(1)))),
            else_: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(2)))),
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
            value: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(42)))),
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
            scopes: ScopeSet::new(),
            value: Rc::new(CoreExpr::Literal(Rc::new(Value::Integer(42)))),
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
            CoreExpr::Literal(Rc::new(Value::Integer(1))),
            CoreExpr::Literal(Rc::new(Value::Integer(2))),
            CoreExpr::Literal(Rc::new(Value::Integer(3))),
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
            params: Formals::Fixed(vec![ScopedParam::simple(Rc::from("x"))]),
            body: vec![CoreExpr::Var {
                name: Rc::from("x"),
                scopes: ScopeSet::new(),
            }],
            binding_scope: None,
        };

        let result = eval_core(&expr, env, &evaluator).unwrap();
        // Should return a procedure
        assert!(matches!(result, Value::Procedure(_)));
    }
}
