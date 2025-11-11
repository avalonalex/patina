//! Special forms evaluation
//!
//! This module implements evaluation for Scheme special forms including:
//! - `quote` - Quote expressions
//! - `if` - Conditional evaluation
//! - `define` - Variable and function definition
//! - `set!` - Assignment
//! - `lambda` - Procedure creation
//! - `let` variants - Binding forms (let, let*, letrec, letrec*, let-values, let*-values)
//! - `cond` and `case` - Multi-way conditionals
//! - `and` and `or` - Boolean operators with short-circuit evaluation
//! - `begin` - Sequential evaluation
//! - `apply` - Apply procedure to list of arguments

use crate::env::Environment;
use crate::value::{Procedure, Value};
use std::rc::Rc;

use super::error::EvalError;
use super::primitives::equality::values_eqv;
use super::Evaluator;

/// Type alias for do loop bindings: (variable_name, init_expr, optional_step_expr)
type DoBinding = (Rc<str>, Value, Option<Value>);

impl Evaluator {
    /// Helper to extract a pair, returning an error if the value is not a pair
    pub(super) fn extract_pair(&self, expr: &Value) -> Result<(Value, Value), EvalError> {
        match expr {
            Value::Pair(pair) => Ok((pair.0.clone(), pair.1.clone())),
            _ => Err(EvalError::InvalidSyntax("Expected a pair".to_string())),
        }
    }

    /// Check for and evaluate arrow syntax (test => proc)
    /// Returns Some(result) if arrow syntax was found and evaluated
    /// Returns None if no arrow syntax (caller should handle as regular body)
    fn eval_arrow_syntax(
        &self,
        exprs: &Value,
        test_value: Value,
        env: &Rc<Environment>,
        form_name: &str,
    ) -> Result<Option<Value>, EvalError> {
        if let Value::Pair(exprs_pair) = exprs {
            if let Value::Symbol(arrow) = &exprs_pair.0 {
                if arrow.as_ref() == "=>" {
                    // Found arrow syntax - apply proc to test value
                    if let Value::Pair(proc_pair) = &exprs_pair.1 {
                        if !matches!(proc_pair.1, Value::Null) {
                            return Err(EvalError::InvalidSyntax(format!(
                                "{}: => requires exactly one expression",
                                form_name
                            )));
                        }
                        let proc = self.eval_in_env(&proc_pair.0, env)?;
                        return Ok(Some(self.apply(proc, vec![test_value])?));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Ensure a value is a proper list (terminated by Null)
    #[allow(dead_code)]
    fn ensure_proper_list(&self, value: &Value, context: &str) -> Result<(), EvalError> {
        if !matches!(value, Value::Null) {
            return Err(EvalError::InvalidSyntax(format!(
                "{} must be a proper list",
                context
            )));
        }
        Ok(())
    }

    /// Extract a symbol from a value, with context for error messages
    #[allow(dead_code)]
    fn expect_symbol(&self, value: &Value, context: &str) -> Result<Rc<str>, EvalError> {
        match value {
            Value::Symbol(s) => Ok(s.clone()),
            _ => Err(EvalError::InvalidSyntax(format!(
                "{} expects a symbol, got {}",
                context,
                value.type_name()
            ))),
        }
    }

    /// Evaluate quote special form: (quote expr)
    pub(super) fn eval_quote(&self, args: &Value) -> Result<Value, EvalError> {
        let (quoted, rest) = self.extract_pair(args)?;
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quote expects exactly one argument".to_string(),
            ));
        }
        Ok(quoted)
    }

    /// Evaluate if special form: (if test then [else])
    /// Evaluate if special form (tail position aware version)
    pub(super) fn eval_if_impl(
        &self,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        let (condition, rest) = self.extract_pair(args)?;
        let (then_branch, rest) = self.extract_pair(&rest)?;

        let else_branch = match rest {
            Value::Null => Value::Unspecified,
            Value::Pair(pair) => {
                if !matches!(pair.1, Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "if expects 2 or 3 arguments".to_string(),
                    ));
                }
                pair.0.clone()
            }
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Malformed if expression".to_string(),
                ))
            }
        };

        // Evaluate condition (NOT in tail position)
        let test_result = self.eval_in_env(&condition, env)?;

        // Both branches ARE in tail position if the if itself is
        let branch = if test_result.is_truthy() {
            then_branch
        } else {
            else_branch
        };

        if in_tail_position {
            Ok(super::EvalResult::TailCall {
                expr: branch,
                env: env.clone(),
            })
        } else {
            self.eval_in_env(&branch, env).map(super::EvalResult::Value)
        }
    }

    /// Legacy wrapper - calls eval_if_impl with in_tail_position=false
    pub(super) fn eval_if(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        self.eval_if_impl(args, env, false)
            .and_then(|result| match result {
                super::EvalResult::Value(v) => Ok(v),
                super::EvalResult::TailCall { .. } => Err(EvalError::InternalError(
                    "Unexpected tail call in non-tail context".to_string(),
                )),
            })
    }

    /// Evaluate define special form: (define var expr) or (define (name params...) body...)
    pub(super) fn eval_define(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (first, rest) = self.extract_pair(args)?;

        match first {
            Value::Symbol(name) => {
                // (define var value)
                let (value_expr, rest) = self.extract_pair(&rest)?;
                if !matches!(rest, Value::Null) {
                    return Err(EvalError::InvalidSyntax(
                        "define expects 2 arguments".to_string(),
                    ));
                }
                let value = self.eval_in_env(&value_expr, env)?;
                env.define(name.to_string(), value);
                Ok(Value::Unspecified)
            }
            Value::Pair(_) => {
                // (define (name params...) body...)
                // Shorthand for (define name (lambda (params...) body...))

                // Extract name from (name params...)
                let (name_val, params_rest) = self.extract_pair(&first)?;
                let name = match name_val {
                    Value::Symbol(s) => s,
                    _ => {
                        return Err(EvalError::InvalidSyntax(
                            "define: function name must be a symbol".to_string(),
                        ))
                    }
                };

                // Parse the parameters
                let (params, variadic) = self.parse_lambda_params(&params_rest)?;

                // rest contains the body expressions
                let body = self.collect_list_items(&rest)?;

                if body.is_empty() {
                    return Err(EvalError::InvalidSyntax(
                        "define: function body cannot be empty".to_string(),
                    ));
                }

                // Create the lambda
                let lambda = Value::Procedure(crate::value::Procedure::Lambda {
                    params,
                    variadic,
                    body,
                    env: env.clone(),
                });

                // Define it
                env.define(name.to_string(), lambda);
                Ok(Value::Unspecified)
            }
            _ => Err(EvalError::InvalidSyntax(
                "define expects a symbol or list".to_string(),
            )),
        }
    }

    /// Evaluate set! special form: (set! var expr)
    pub(super) fn eval_set(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let (var, rest) = self.extract_pair(args)?;
        let (value_expr, rest) = self.extract_pair(&rest)?;

        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "set! expects 2 arguments".to_string(),
            ));
        }

        if let Value::Symbol(name) = var {
            let value = self.eval_in_env(&value_expr, env)?;
            env.set(&name, value)
                .map_err(EvalError::UndefinedVariable)?;
            Ok(Value::Unspecified)
        } else {
            Err(EvalError::InvalidSyntax(
                "set! expects a symbol".to_string(),
            ))
        }
    }

    /// Evaluate lambda special form: (lambda params body...)
    pub(super) fn eval_lambda(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (params_expr, rest) = self.extract_pair(args)?;

        // Parse parameters
        let (params, variadic) = self.parse_lambda_params(&params_expr)?;

        // Collect body expressions
        let body = self.collect_list_items(&rest)?;

        if body.is_empty() {
            return Err(EvalError::InvalidSyntax(
                "lambda requires at least one body expression".to_string(),
            ));
        }

        Ok(Value::Procedure(Procedure::Lambda {
            params,
            variadic,
            body,
            env: env.clone(),
        }))
    }

    /// Parse lambda parameter list
    fn parse_lambda_params(
        &self,
        params_expr: &Value,
    ) -> Result<(Vec<String>, Option<String>), EvalError> {
        match params_expr {
            // (lambda args body...) - single symbol, all args go to it
            Value::Symbol(s) => Ok((vec![], Some(s.to_string()))),

            // (lambda () body...) - no parameters
            Value::Null => Ok((vec![], None)),

            // (lambda (x y z) body...) or (lambda (x y . rest) body...)
            Value::Pair(_) => {
                let mut params = Vec::new();
                let mut current = params_expr.clone();

                loop {
                    match &current {
                        Value::Null => return Ok((params, None)),
                        Value::Symbol(s) => {
                            // Rest parameter: (x y . rest)
                            return Ok((params, Some(s.to_string())));
                        }
                        Value::Pair(pair) => {
                            if let Value::Symbol(param) = &pair.0 {
                                params.push(param.to_string());
                                current = pair.1.clone();
                            } else {
                                return Err(EvalError::InvalidSyntax(
                                    "lambda parameters must be symbols".to_string(),
                                ));
                            }
                        }
                        _ => {
                            return Err(EvalError::InvalidSyntax(
                                "invalid lambda parameter list".to_string(),
                            ))
                        }
                    }
                }
            }

            _ => Err(EvalError::InvalidSyntax(
                "lambda parameters must be a list or symbol".to_string(),
            )),
        }
    }

    /// Helper to collect list items into a vector
    fn collect_list_items(&self, list: &Value) -> Result<Vec<Value>, EvalError> {
        let mut items = Vec::new();
        let mut current = list.clone();

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "improper list in lambda body".to_string(),
            ));
        }

        Ok(items)
    }

    /// Evaluate cond special form: (cond (test expr...) ...)
    /// Evaluate cond special form (tail position aware version)
    pub(super) fn eval_cond_impl(
        &self,
        clauses: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        let mut current = clauses.clone();

        while let Value::Pair(clause_pair) = current {
            let clause = &clause_pair.0;

            // Extract test and expressions from clause
            if let Value::Pair(clause_contents) = clause {
                let test = &clause_contents.0;
                let exprs = &clause_contents.1;

                // Check for 'else' clause
                if let Value::Symbol(sym) = test {
                    if sym.as_ref() == "else" {
                        // else clause - check for => syntax and reject it
                        if let Value::Pair(exprs_pair) = exprs {
                            if let Value::Symbol(arrow) = &exprs_pair.0 {
                                if arrow.as_ref() == "=>" {
                                    return Err(EvalError::InvalidSyntax(
                                        "cond: else clause cannot use =>".to_string(),
                                    ));
                                }
                            }
                        }
                        // Regular else clause - body is in tail position
                        return self.eval_begin_impl(exprs, env, in_tail_position);
                    }
                }

                // Evaluate the test (NOT in tail position)
                let test_result = self.eval_in_env(test, env)?;

                if test_result.is_truthy() {
                    // Test succeeded - check for => syntax
                    // TODO: => application should also be in tail position per R7RS
                    if let Some(result) = self.eval_arrow_syntax(exprs, test_result, env, "cond")? {
                        return Ok(super::EvalResult::Value(result));
                    }

                    // Regular clause - body is in tail position
                    return self.eval_begin_impl(exprs, env, in_tail_position);
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "cond clause must be a list".to_string(),
                ));
            }

            current = clause_pair.1.clone();
        }

        // No clause matched and no else - return unspecified
        Ok(super::EvalResult::Value(Value::Unspecified))
    }

    /// Legacy wrapper - calls eval_cond_impl with in_tail_position=false
    pub(super) fn eval_cond(
        &self,
        clauses: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        self.eval_cond_impl(clauses, env, false)
            .and_then(|result| match result {
                super::EvalResult::Value(v) => Ok(v),
                super::EvalResult::TailCall { .. } => Err(EvalError::InternalError(
                    "Unexpected tail call in non-tail context".to_string(),
                )),
            })
    }

    /// Evaluate case special form: (case key ((datum...) expr...)...)
    /// Evaluate case special form (tail position aware version)
    pub(super) fn eval_case_impl(
        &self,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Extract key expression
        let (key_expr, clauses) = self.extract_pair(args)?;

        // Evaluate the key
        let key = self.eval_in_env(&key_expr, env)?;

        // Iterate through clauses
        let mut current = clauses;

        while let Value::Pair(clause_pair) = current {
            let clause = &clause_pair.0;

            // Each clause should be ((<datum> ...) <expr> ...)
            if let Value::Pair(clause_contents) = clause {
                let datums = &clause_contents.0;
                let exprs = &clause_contents.1;

                // Check for 'else' clause
                if let Value::Symbol(sym) = datums {
                    if sym.as_ref() == "else" {
                        // else clause - check for => syntax
                        if let Some(result) =
                            self.eval_arrow_syntax(exprs, key.clone(), env, "case")?
                        {
                            // Arrow syntax is not in tail position (requires apply)
                            return Ok(super::EvalResult::Value(result));
                        }
                        // Regular else clause (in tail position)
                        return self.eval_begin_impl(exprs, env, in_tail_position);
                    }
                }

                // Check if key matches any datum in this clause
                let mut matched = false;
                let mut datum_list = datums.clone();

                while let Value::Pair(datum_pair) = datum_list {
                    let datum = &datum_pair.0;

                    // Use eqv? semantics for comparison
                    if values_eqv(&key, datum) {
                        matched = true;
                        break;
                    }

                    datum_list = datum_pair.1.clone();
                }

                if matched {
                    // Check for => syntax
                    if let Some(result) = self.eval_arrow_syntax(exprs, key.clone(), env, "case")? {
                        // Arrow syntax is not in tail position (requires apply)
                        return Ok(super::EvalResult::Value(result));
                    }

                    // Regular clause - evaluate expressions (in tail position)
                    return self.eval_begin_impl(exprs, env, in_tail_position);
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "case clause must be a list".to_string(),
                ));
            }

            current = clause_pair.1.clone();
        }

        // No clause matched and no else - return unspecified
        Ok(super::EvalResult::Value(Value::Unspecified))
    }

    /// Legacy wrapper - calls eval_case_impl with in_tail_position=false
    pub(super) fn eval_case(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        self.eval_case_impl(args, env, false)
            .and_then(|result| match result {
                super::EvalResult::Value(v) => Ok(v),
                super::EvalResult::TailCall { .. } => Err(EvalError::InternalError(
                    "Unexpected tail call in non-tail context".to_string(),
                )),
            })
    }

    /// Evaluate begin special form (tail position aware version)
    pub(super) fn eval_begin_impl(
        &self,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        let mut current = args.clone();
        let mut result = Value::Unspecified;

        // Evaluate all expressions except the last
        while let Value::Pair(pair) = &current {
            if matches!(pair.1, Value::Null) {
                // Last expression - in tail position if begin is
                if in_tail_position {
                    return Ok(super::EvalResult::TailCall {
                        expr: pair.0.clone(),
                        env: env.clone(),
                    });
                } else {
                    result = self.eval_in_env(&pair.0, env)?;
                    break;
                }
            } else {
                // Not last expression - NOT in tail position
                result = self.eval_in_env(&pair.0, env)?;
                current = pair.1.clone();
            }
        }

        Ok(super::EvalResult::Value(result))
    }

    /// Legacy wrapper - calls eval_begin_impl with in_tail_position=false
    pub(super) fn eval_begin(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        self.eval_begin_impl(args, env, false)
            .and_then(|result| match result {
                super::EvalResult::Value(v) => Ok(v),
                super::EvalResult::TailCall { .. } => Err(EvalError::InternalError(
                    "Unexpected tail call in non-tail context".to_string(),
                )),
            })
    }

    // NOTE: 'let' and 'let*' are now implemented as macros in lib/bootstrap.scm
    // This reduces code duplication and aligns with R7RS reference implementations.
    // Previous Rust implementations (eval_let_impl, eval_let, eval_let_star_impl,
    // eval_let_star) were removed in favor of the macro approach.

    // NOTE: 'letrec' and 'letrec*' are now implemented as macros in lib/bootstrap.scm
    // This reduces code duplication and aligns with R7RS reference implementations.
    // The macro implementations correctly handle recursive bindings by initializing
    // variables to #f first, then assigning values sequentially. Previous Rust
    // implementations (parse_simple_bindings, eval_letrec_impl, eval_letrec,
    // eval_letrec_star_impl, eval_letrec_star) were removed in favor of the macro
    // approach.
    //
    // NOTE: let-values and let*-values have been migrated to macros in lib/bootstrap.scm.
    // The native implementations below were removed as they are no longer needed.
    // See commit history for the original implementations if needed for reference.

    /// Evaluate call-with-values special form (tail position aware version)
    /// (call-with-values producer consumer)
    /// Per R7RS Section 3.5: "the second argument passed to call-with-values
    /// must be called via a tail call"
    pub(super) fn eval_call_with_values_impl(
        &self,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Extract producer and consumer expressions
        let (producer_expr, rest) = self.extract_pair(args)?;
        let (consumer_expr, rest2) = self.extract_pair(&rest)?;

        if !matches!(rest2, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "call-with-values expects exactly 2 arguments".to_string(),
            ));
        }

        // Evaluate producer and consumer to get procedures
        let producer = self.eval_in_env(&producer_expr, env)?;
        let consumer = self.eval_in_env(&consumer_expr, env)?;

        // Call producer with no arguments to get values
        let produced = self.apply(producer, vec![])?;

        // Unpack multiple values if present
        let consumer_args = match produced {
            Value::Values(vals) => vals,
            other => vec![other],
        };

        // Apply consumer to the produced values
        // This MUST be a tail call per R7RS when in tail position
        if in_tail_position {
            // Construct the application as an S-expression: (consumer arg1 arg2 ...)
            // This allows the trampoline to handle it as a proper tail call
            let mut app_list = vec![consumer];
            app_list.extend(consumer_args);
            let application = self.list_from_vec(app_list);

            Ok(super::EvalResult::TailCall {
                expr: application,
                env: env.clone(),
            })
        } else {
            Ok(super::EvalResult::Value(
                self.apply(consumer, consumer_args)?,
            ))
        }
    }

    // NOTE: bind_values_to_formals was removed as it was only used by the now-deleted
    // let-values and let*-values implementations. The macro versions in bootstrap.scm
    // handle value binding through lambda parameters instead.

    /// Evaluate and special form (tail position aware version)
    // NOTE: 'and' and 'or' are now implemented as macros in lib/bootstrap.scm
    // This reduces code duplication and aligns with R7RS reference implementations.
    // The macro implementations provide the same semantics including short-circuit
    // evaluation. Previous Rust implementations (eval_and_impl, eval_and,
    // eval_or_impl, eval_or) were removed in favor of the macro approach.
    /// Evaluate apply special form: (apply proc arg... args)
    pub(super) fn eval_apply(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        // Collect all arguments
        let mut arg_exprs = Vec::new();
        let mut current = args.clone();

        while let Value::Pair(pair) = current {
            arg_exprs.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "apply expects a proper list of arguments".to_string(),
            ));
        }

        if arg_exprs.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: arg_exprs.len(),
            });
        }

        // First argument is the procedure
        let proc = self.eval_in_env(&arg_exprs[0], env)?;

        // Middle arguments (if any) are regular arguments
        let mut final_args = Vec::new();
        for arg in arg_exprs.iter().take(arg_exprs.len() - 1).skip(1) {
            final_args.push(self.eval_in_env(arg, env)?);
        }

        // Last argument must be a list
        let last_arg = self.eval_in_env(&arg_exprs[arg_exprs.len() - 1], env)?;

        // Convert the last argument (list) to a vector and append to final_args
        let mut current = last_arg.clone();
        while let Value::Pair(pair) = current {
            final_args.push(pair.0.clone());
            current = pair.1.clone();
        }

        // Check that the last argument was a proper list
        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(
                "apply: last argument must be a proper list".to_string(),
            ));
        }

        // Now apply the procedure to the combined arguments
        self.apply(proc, final_args)
    }

    /// Evaluate define-syntax special form: (define-syntax name (syntax-rules (literals) rules...))
    pub(super) fn eval_define_syntax(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        // Parse: (define-syntax name transformer)
        let (name_expr, rest) = self.extract_pair(args)?;
        let (transformer_expr, rest2) = self.extract_pair(&rest)?;

        // Check that there are no extra arguments
        if !matches!(rest2, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "define-syntax expects exactly 2 arguments".to_string(),
            ));
        }

        // Name must be a symbol
        let name = match name_expr {
            Value::Symbol(s) => s,
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "define-syntax name must be a symbol".to_string(),
                ))
            }
        };

        // Transformer must be (syntax-rules (literals) rules...)
        let macro_def = self.parse_syntax_rules(&transformer_expr, env)?;

        // Create a Macro value with this name
        let macro_value = Value::Macro(Rc::new(crate::macro_system::Macro {
            name: name.clone(),
            rules: macro_def.rules,
            literals: macro_def.literals,
            env: env.clone(),
        }));

        // Bind the macro in the environment
        env.define(name.to_string(), macro_value);

        Ok(Value::Unspecified)
    }

    /// Parse a syntax-rules form: (syntax-rules (literals) (pattern template)...)
    fn parse_syntax_rules(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
    ) -> Result<crate::macro_system::Macro, EvalError> {
        // Must be a list starting with 'syntax-rules
        let (keyword, rest) = self.extract_pair(expr)?;

        match keyword {
            Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Expected syntax-rules".to_string(),
                ))
            }
        }

        // Parse literals list
        let (literals_expr, rules_expr) = self.extract_pair(&rest)?;

        let literals = self.parse_literals_list(&literals_expr)?;

        // Parse rules
        let rules = self.parse_macro_rules(&rules_expr)?;

        Ok(crate::macro_system::Macro {
            name: Rc::from("anonymous"),
            rules,
            literals,
            env: env.clone(),
        })
    }

    /// Parse the literals list: (lit1 lit2 ...)
    fn parse_literals_list(&self, expr: &Value) -> Result<Vec<Rc<str>>, EvalError> {
        let mut literals = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(pair) = current {
            match &pair.0 {
                Value::Symbol(s) => literals.push(s.clone()),
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "syntax-rules literals must be symbols".to_string(),
                    ))
                }
            }
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules literals must be a proper list".to_string(),
            ));
        }

        Ok(literals)
    }

    /// Parse macro rules: ((pattern template) ...)
    fn parse_macro_rules(
        &self,
        expr: &Value,
    ) -> Result<Vec<crate::macro_system::MacroRule>, EvalError> {
        let mut rules = Vec::new();
        let mut current = expr.clone();

        while let Value::Pair(pair) = current {
            // Each rule is (pattern template)
            let (pattern_expr, template_rest) = self.extract_pair(&pair.0)?;
            let (template_expr, template_end) = self.extract_pair(&template_rest)?;

            if !matches!(template_end, Value::Null) {
                return Err(EvalError::InvalidSyntax(
                    "syntax-rules rule must have exactly 2 elements (pattern template)".to_string(),
                ));
            }

            // Parse pattern and template using macro_system functions
            let pattern = crate::macro_system::parse_pattern(&pattern_expr)?;
            let template = crate::macro_system::parse_template(&template_expr)?;

            rules.push(crate::macro_system::MacroRule { pattern, template });

            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules rules must be a proper list".to_string(),
            ));
        }

        if rules.is_empty() {
            return Err(EvalError::InvalidSyntax(
                "syntax-rules must have at least one rule".to_string(),
            ));
        }

        Ok(rules)
    }

    /// Expand a macro call
    pub(super) fn expand_macro(
        &self,
        macro_val: &crate::macro_system::Macro,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        crate::macro_system::expand_macro(macro_val, args, env)
    }

    /// Evaluate do special form: (do ((var init step) ...) (test result ...) command ...)
    ///
    /// Evaluate do special form (tail position aware version)
    ///
    /// R7RS Section 4.2.4: Iteration
    ///
    /// Semantics:
    /// 1. Evaluate all init expressions and bind variables
    /// 2. Loop:
    ///    - Evaluate test
    ///    - If true: evaluate result expressions and return last value
    ///    - If false: evaluate commands, then evaluate steps and update bindings
    /// 3. If step is omitted, variable doesn't change between iterations
    pub(super) fn eval_do_impl(
        &self,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        // Parse: (do ((var init step?) ...) (test result ...) command ...)
        let (bindings_expr, rest) = self.extract_pair(args)?;
        let (test_clause_expr, commands_expr) = self.extract_pair(&rest)?;

        // Parse bindings: ((var init step?) ...)
        let bindings = self.parse_do_bindings(&bindings_expr)?;

        // Parse test clause: (test result ...)
        let (test_expr, result_exprs) = self.parse_do_test_clause(&test_clause_expr)?;

        // Parse commands: (command ...)
        let commands = self.collect_list_items(&commands_expr)?;

        // 1. Create loop environment and evaluate init expressions
        let loop_env = Rc::new(Environment::with_parent(env.clone()));

        for (var_name, init_expr, _step_expr) in &bindings {
            let init_val = self.eval_in_env(init_expr, env)?;
            loop_env.define(var_name.to_string(), init_val);
        }

        // 2. Execute loop
        loop {
            // Evaluate test expression
            let test_result = self.eval_in_env(&test_expr, &loop_env)?;

            // If test is true, evaluate result expressions and return
            if test_result.is_truthy() {
                // Result expressions are in tail position
                if result_exprs.is_empty() {
                    return Ok(super::EvalResult::Value(Value::Unspecified));
                }

                // Evaluate all but last result expression
                for result_expr in &result_exprs[..result_exprs.len() - 1] {
                    self.eval_in_env(result_expr, &loop_env)?;
                }

                // Last result expression is in tail position
                let last_result_expr = &result_exprs[result_exprs.len() - 1];
                if in_tail_position {
                    return Ok(super::EvalResult::TailCall {
                        expr: last_result_expr.clone(),
                        env: loop_env,
                    });
                } else {
                    let last_result = self.eval_in_env(last_result_expr, &loop_env)?;
                    return Ok(super::EvalResult::Value(last_result));
                }
            }

            // Test is false: execute commands (for side effects)
            for command in &commands {
                self.eval_in_env(command, &loop_env)?;
            }

            // Evaluate step expressions and update bindings
            // Important: evaluate all steps BEFORE updating any bindings
            let mut new_values = Vec::new();
            for (var_name, _init_expr, step_expr_opt) in &bindings {
                let new_val = if let Some(step_expr) = step_expr_opt {
                    self.eval_in_env(step_expr, &loop_env)?
                } else {
                    // No step: variable keeps its current value
                    loop_env
                        .get(var_name)
                        .ok_or_else(|| EvalError::UndefinedVariable(var_name.to_string()))?
                };
                new_values.push((var_name.clone(), new_val));
            }

            // Update all bindings atomically
            for (var_name, new_val) in new_values {
                loop_env
                    .set(&var_name, new_val)
                    .map_err(EvalError::UndefinedVariable)?;
            }
        }
    }

    /// Legacy wrapper - calls eval_do_impl with in_tail_position=false
    pub(super) fn eval_do(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        self.eval_do_impl(args, env, false)
            .and_then(|result| match result {
                super::EvalResult::Value(v) => Ok(v),
                super::EvalResult::TailCall { .. } => Err(EvalError::InternalError(
                    "Unexpected tail call in non-tail context".to_string(),
                )),
            })
    }

    /// Parse do bindings: ((var init step?) ...)
    /// Returns: Vec<(var_name, init_expr, step_expr_opt)>
    fn parse_do_bindings(&self, bindings_expr: &Value) -> Result<Vec<DoBinding>, EvalError> {
        let mut bindings = Vec::new();
        let mut current = bindings_expr.clone();

        while let Value::Pair(pair) = current {
            // Each binding is (var init) or (var init step)
            let binding = &pair.0;

            let (var_expr, rest) = self.extract_pair(binding)?;
            let var_name = match var_expr {
                Value::Symbol(s) => s.clone(),
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "do binding variable must be a symbol".to_string(),
                    ))
                }
            };

            let (init_expr, rest) = self.extract_pair(&rest)?;

            // Step is optional
            let step_expr_opt = match rest {
                Value::Null => None,
                Value::Pair(step_pair) => {
                    // Ensure there's nothing after step
                    if !matches!(step_pair.1, Value::Null) {
                        return Err(EvalError::InvalidSyntax(
                            "do binding must be (var init) or (var init step)".to_string(),
                        ));
                    }
                    Some(step_pair.0.clone())
                }
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "do binding must be a proper list".to_string(),
                    ))
                }
            };

            bindings.push((var_name, init_expr.clone(), step_expr_opt));
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "do bindings must be a proper list".to_string(),
            ));
        }

        Ok(bindings)
    }

    /// Parse do test clause: (test result ...)
    /// Returns: (test_expr, Vec<result_expr>)
    fn parse_do_test_clause(
        &self,
        test_clause_expr: &Value,
    ) -> Result<(Value, Vec<Value>), EvalError> {
        let (test_expr, rest) = self.extract_pair(test_clause_expr)?;
        let result_exprs = self.collect_list_items(&rest)?;
        Ok((test_expr.clone(), result_exprs))
    }
}
