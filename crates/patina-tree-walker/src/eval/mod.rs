// Module declarations
mod application;
mod debug;
mod error;
mod primitives;
mod special_forms;

// Re-export error type for public API
pub use error::EvalError;

use debug::DebugConfig;
use patina_runtime::environment::Environment;
use patina_runtime::value::{Procedure, Value};
use std::rc::Rc;

/// Result of evaluation step in the trampoline
///
/// The trampoline pattern enables tail call optimization by converting
/// recursive calls into an iterative loop. Instead of making recursive
/// calls that grow the stack, tail positions return `TailCall` which
/// tells the trampoline to continue with the next computation.
#[derive(Debug)]
pub(crate) enum EvalResult {
    /// Final value - evaluation complete
    Value(Value),
    /// Tail call - continue trampolining with this expression and environment
    TailCall { expr: Value, env: Rc<Environment> },
    /// Tail call to a primitive procedure with already-evaluated arguments
    /// This enables primitives like call-with-values to participate in tail call optimization
    /// The procedure and arguments are already evaluated, so we just need to apply them
    TailCallPrimitive { proc: Value, args: Vec<Value> },
}

pub struct Evaluator {
    pub(in crate::eval) global_env: Rc<Environment>,
    pub(crate) debug: Rc<DebugConfig>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        Self::install_primitives(&global_env);
        let evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
        };

        // Load bootstrap library
        evaluator.load_bootstrap();

        evaluator
    }

    fn load_bootstrap(&self) {
        // Embed bootstrap.scm at compile time
        const BOOTSTRAP: &str = include_str!("../../../../lib/bootstrap.scm");

        // Parse and evaluate all expressions in bootstrap
        // Silently ignore any errors (shouldn't happen in bootstrap)
        let mut parser = match patina_frontend::Parser::new(BOOTSTRAP) {
            Ok(p) => p,
            Err(_) => return, // Bootstrap failed to parse
        };

        // Parse and eval all expressions
        loop {
            match parser.parse() {
                Ok(expr) => {
                    // Evaluate, ignore result and errors
                    let _ = self.eval(&expr);
                }
                Err(patina_frontend::ParseError::UnexpectedEof) => break,
                Err(_) => break, // Stop on other errors
            }
        }
    }

    /// Main evaluation entry point with trampoline for tail call optimization
    ///
    /// This implements the trampoline pattern: instead of recursing directly for tail calls,
    /// we loop and process `TailCall` results iteratively. This enables proper tail recursion
    /// as required by R7RS - tail calls execute in constant stack space.
    pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
        let mut current_expr = expr.clone();
        let mut current_env = self.global_env.clone();

        // Trampoline loop: keep evaluating until we get a final value
        loop {
            match self.eval_step(&current_expr, &current_env)? {
                EvalResult::Value(v) => return Ok(v),
                EvalResult::TailCall { expr, env } => {
                    // Tail call - continue loop with new expr and env
                    // This reuses the stack frame instead of growing the stack
                    current_expr = expr;
                    current_env = env;
                }
                EvalResult::TailCallPrimitive { proc, args } => {
                    // Primitive tail call - re-apply directly in the trampoline
                    // This allows primitives like call-with-values to participate in TCO
                    // Note: We pass in_tail_position=true since we're continuing the trampoline
                    match self.apply(proc, args, true)? {
                        EvalResult::Value(v) => return Ok(v),
                        EvalResult::TailCall { expr, env } => {
                            current_expr = expr;
                            current_env = env;
                        }
                        EvalResult::TailCallPrimitive { proc, args } => {
                            // Another primitive tail call - continue loop
                            // We'll handle this in the next iteration by reconstructing
                            // the application expression
                            let mut app_list = vec![proc];
                            app_list.extend(args);
                            current_expr = self.list_from_vec(app_list);
                            // Keep current_env unchanged
                        }
                    }
                }
            }
        }
    }

    /// Single evaluation step for the trampoline
    ///
    /// Returns EvalResult which can be either:
    /// - Value: evaluation complete, return this value
    /// - TailCall: tail position, bounce to expr in env
    ///
    /// The `in_tail_position` parameter indicates whether this expression is in tail context.
    /// If true, the final result can be returned as a TailCall for the trampoline to process.
    fn eval_step(&self, expr: &Value, env: &Rc<Environment>) -> Result<EvalResult, EvalError> {
        // Top-level trampoline evaluations are in tail position!
        // This allows the trampoline to bounce tail calls.
        self.eval_step_impl(expr, env, true)
    }

    /// Implementation of eval_step with tail position tracking
    fn eval_step_impl(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            eprintln!(
                "[EVAL]{} Evaluating: {} (tail={})",
                self.debug.current_indent(),
                expr,
                in_tail_position
            );
            self.debug.indent();
        }

        let result = match expr {
            // Self-evaluating
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_, _)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(EvalResult::Value(expr.clone())),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    eprintln!("[ENV]{} Lookup: '{}'", self.debug.current_indent(), name);
                }

                // First try looking up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(EvalResult::Value(value));
                }

                // If it's a gensym and not found, try looking it up in the global environment
                if patina_frontend::macro_expander::hygiene::is_gensym(name.as_ref()) {
                    if let Some(original_name) = extract_original_from_gensym(name.as_ref()) {
                        if let Some(value) = self.global_env.get(&Rc::from(original_name)) {
                            return Ok(EvalResult::Value(value));
                        }
                    }
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
            }

            // Empty list
            Value::Null => Ok(EvalResult::Value(Value::Null)),

            // Lists (procedure calls or special forms)
            Value::Pair(_) => self.eval_list_impl(expr, env, in_tail_position),

            _ => Ok(EvalResult::Value(expr.clone())),
        };

        // Debug trace exit
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            self.debug.dedent();
            match &result {
                Ok(EvalResult::Value(val)) => {
                    eprintln!("[EVAL]{} => {}", self.debug.current_indent(), val)
                }
                Ok(EvalResult::TailCall { expr, .. }) => eprintln!(
                    "[EVAL]{} => TAIL CALL: {}",
                    self.debug.current_indent(),
                    expr
                ),
                Ok(EvalResult::TailCallPrimitive { proc, args }) => {
                    let args_str = args
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!(
                        "[EVAL]{} => TAIL CALL PRIMITIVE: {} ({})",
                        self.debug.current_indent(),
                        proc,
                        args_str
                    )
                }
                Err(e) => eprintln!("[EVAL]{} => ERROR: {}", self.debug.current_indent(), e),
            }
        }

        result
    }

    /// Evaluate a list (procedure call or special form) with tail position tracking
    fn eval_list_impl(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Check for special forms - these will handle tail positions internally
        if let Value::Symbol(ref sym) = car {
            match sym.as_ref() {
                "quote" => return self.eval_quote(&cdr).map(EvalResult::Value),
                "if" => return self.eval_if_impl(&cdr, env, in_tail_position),
                "define" => return self.eval_define(&cdr, env).map(EvalResult::Value),
                "define-syntax" => {
                    return self.eval_define_syntax(&cdr, env).map(EvalResult::Value)
                }
                "set!" => return self.eval_set(&cdr, env).map(EvalResult::Value),
                "lambda" => return self.eval_lambda(&cdr, env).map(EvalResult::Value),
                "begin" => return self.eval_begin_impl(&cdr, env, in_tail_position),
                // NOTE: 'cond' and 'case' are now implemented as macros in lib/bootstrap.scm
                // NOTE: 'do' has a macro implementation in bootstrap.scm, but we use the special
                // form for proper tail call optimization. The macro version uses 'apply' which
                // doesn't yet support TCO, causing stack overflows in tail-recursive exit clauses.
                // TODO(TCO): 'apply' needs tail call support - see PRD/future/GENERAL_TAIL_CALL_OPTIMIZATION.md
                "apply" => return self.eval_apply(&cdr, env).map(EvalResult::Value),
                "do" => return self.eval_do_impl(&cdr, env, in_tail_position),
                // Note: call-with-values was previously a special form, but is now fully handled
                // as a primitive that participates in tail call optimization via TailCallPrimitive
                _ => {}
            }

            // Check if this symbol is bound to a macro
            if let Some(Value::Macro { data, .. }) = env.get(sym) {
                let macro_def = data
                    .downcast_ref::<patina_frontend::macro_expander::Macro>()
                    .ok_or_else(|| EvalError::InternalError("Invalid macro data".to_string()))?;

                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanding macro '{}': {}",
                        self.debug.current_indent(),
                        sym,
                        expr
                    );
                    self.debug.indent();
                }

                let expanded = self.expand_macro(macro_def, expr, env)?;

                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanded to: {}",
                        self.debug.current_indent(),
                        expanded
                    );
                    self.debug.dedent();
                }

                // Evaluate the expanded form, preserving tail position
                return self.eval_step_impl(&expanded, env, in_tail_position);
            }
        }

        // Regular procedure call - this can be a tail call if in tail position
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;

        // Check if this is a lambda in tail position
        if in_tail_position {
            if let Value::Procedure(Procedure::Lambda {
                params,
                variadic,
                body,
                env: lambda_env,
            }) = proc
            {
                // Tail call to lambda - set up environment and evaluate body
                // This is the key to tail recursion!

                // Check arity
                if variadic.is_some() {
                    if args.len() < params.len() {
                        return Err(EvalError::WrongArity {
                            expected: format!("at least {}", params.len()),
                            actual: args.len(),
                        });
                    }
                } else if args.len() != params.len() {
                    return Err(EvalError::WrongArity {
                        expected: params.len().to_string(),
                        actual: args.len(),
                    });
                }

                // Create new environment for the lambda
                let new_env = Rc::new(Environment::with_parent(lambda_env));

                // Bind parameters
                for (param, arg) in params.iter().zip(args.iter()) {
                    new_env.define(param.clone(), arg.clone());
                }

                // Bind rest parameter if variadic
                if let Some(rest_param) = variadic {
                    let rest_args: Vec<Value> = args.into_iter().skip(params.len()).collect();
                    let rest_list = self.list_from_vec(rest_args);
                    new_env.define(rest_param, rest_list);
                }

                // Return tail call to evaluate the body in the new environment
                // The body expressions are evaluated sequentially, with the last in tail position
                if body.is_empty() {
                    return Ok(EvalResult::Value(Value::Unspecified));
                }

                // Evaluate all but the last expression
                for expr in &body[..body.len() - 1] {
                    self.eval_in_env(expr, &new_env)?;
                }

                // Last expression is in tail position - return it for the trampoline
                return Ok(EvalResult::TailCall {
                    expr: body.last().unwrap().clone(),
                    env: new_env,
                });
            }
        }

        // Not in tail position, or not a lambda - just apply normally
        self.apply(proc, args, in_tail_position)
    }

    /// Internal eval_in_env - evaluates expression in given environment
    /// Used by special forms and primitives for recursive evaluation.
    fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            eprintln!("[EVAL]{} Evaluating: {}", self.debug.current_indent(), expr);
            self.debug.indent();
        }

        let result = match expr {
            // Self-evaluating
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_, _)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(expr.clone()),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    eprintln!("[ENV]{} Lookup: '{}'", self.debug.current_indent(), name);
                }

                // First try looking up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(value);
                }

                // If it's a gensym and not found, try looking it up in the global environment
                // This handles hygienic macro expansion: gensyms reference bindings from
                // where the macro was defined, not where it's used
                if patina_frontend::macro_expander::hygiene::is_gensym(name.as_ref()) {
                    // Extract original name from gensym (format: ##name#counter)
                    if let Some(original_name) = extract_original_from_gensym(name.as_ref()) {
                        if let Some(value) = self.global_env.get(&Rc::from(original_name)) {
                            return Ok(value);
                        }
                    }
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
            }

            // Empty list
            Value::Null => Ok(Value::Null),

            // Lists (procedure calls or special forms)
            Value::Pair(_) => self.eval_list(expr, env),

            _ => Ok(expr.clone()),
        };

        // Debug trace exit
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            self.debug.dedent();
            match &result {
                Ok(val) => eprintln!("[EVAL]{} => {}", self.debug.current_indent(), val),
                Err(e) => eprintln!("[EVAL]{} => ERROR: {}", self.debug.current_indent(), e),
            }
        }

        result
    }

    fn eval_list(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Check for special forms
        if let Value::Symbol(ref sym) = car {
            match sym.as_ref() {
                "quote" => return self.eval_quote(&cdr),
                "if" => return self.eval_if(&cdr, env),
                "define" => return self.eval_define(&cdr, env),
                "define-syntax" => return self.eval_define_syntax(&cdr, env),
                "set!" => return self.eval_set(&cdr, env),
                "lambda" => return self.eval_lambda(&cdr, env),
                "begin" => return self.eval_begin(&cdr, env),
                // NOTE: 'cond' and 'case' are now implemented as macros in lib/bootstrap.scm
                // NOTE: 'do' macro is defined in bootstrap.scm but special form is used for TCO
                "apply" => return self.eval_apply(&cdr, env),
                "do" => return self.eval_do(&cdr, env),
                _ => {}
            }

            // Check if this symbol is bound to a macro
            if let Some(Value::Macro { data, .. }) = env.get(sym) {
                let macro_def = data
                    .downcast_ref::<patina_frontend::macro_expander::Macro>()
                    .ok_or_else(|| EvalError::InternalError("Invalid macro data".to_string()))?;

                // Debug trace: macro expansion entry
                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanding macro '{}': {}",
                        self.debug.current_indent(),
                        sym,
                        expr
                    );
                    self.debug.indent();
                }

                // Expand the macro with the WHOLE form (unevaluated!)
                // The pattern includes the keyword, so we pass the whole expr
                let expanded = self.expand_macro(macro_def, expr, env)?;

                // Debug trace: show expanded form
                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanded to: {}",
                        self.debug.current_indent(),
                        expanded
                    );
                    self.debug.dedent();
                }

                // Evaluate the expanded form
                return self.eval_in_env(&expanded, env);
            }
        }

        // Regular procedure call
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;
        // Legacy eval_list is not tail-aware, so always pass false
        match self.apply(proc, args, false)? {
            EvalResult::Value(v) => Ok(v),
            EvalResult::TailCall { .. } | EvalResult::TailCallPrimitive { .. } => Err(
                EvalError::InternalError("Unexpected tail call in non-tail context".to_string()),
            ),
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the original identifier name from a gensym
///
/// Gensyms have format: ##name#counter
/// This extracts "name" from that format.
fn extract_original_from_gensym(gensym: &str) -> Option<String> {
    if !gensym.starts_with("##") {
        return None;
    }

    // Skip "##" prefix
    let without_prefix = &gensym[2..];

    // Find the last '#' which separates name from counter
    without_prefix
        .rfind('#')
        .map(|last_hash| without_prefix[..last_hash].to_string())
}
