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

    /// Parse simple bindings of the form ((var val) ...)
    /// Used by let, let*, letrec, and letrec*
    fn parse_simple_bindings(
        &self,
        bindings: &Value,
        form_name: &str,
    ) -> Result<(Vec<Rc<str>>, Vec<Value>), EvalError> {
        let mut names = Vec::new();
        let mut value_exprs = Vec::new();
        let mut current = bindings.clone();

        while let Value::Pair(binding_pair) = current {
            // Each binding should be (name value)
            let binding = &binding_pair.0;
            if let Value::Pair(binding_contents) = binding {
                if let Value::Symbol(name) = &binding_contents.0 {
                    names.push(name.clone());

                    // Extract value expression
                    if let Value::Pair(value_pair) = &binding_contents.1 {
                        if !matches!(value_pair.1, Value::Null) {
                            return Err(EvalError::InvalidSyntax(format!(
                                "{} binding must have exactly 2 elements",
                                form_name
                            )));
                        }
                        value_exprs.push(value_pair.0.clone());
                    } else {
                        return Err(EvalError::InvalidSyntax(format!(
                            "{} binding must have a value",
                            form_name
                        )));
                    }
                } else {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} binding name must be a symbol",
                        form_name
                    )));
                }
            } else {
                return Err(EvalError::InvalidSyntax(format!(
                    "{} binding must be a list",
                    form_name
                )));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(format!(
                "{} bindings must be a proper list",
                form_name
            )));
        }

        Ok((names, value_exprs))
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
    pub(super) fn eval_if(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
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

        let test_result = self.eval_in_env(&condition, env)?;

        if test_result.is_truthy() {
            self.eval_in_env(&then_branch, env)
        } else {
            self.eval_in_env(&else_branch, env)
        }
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
    pub(super) fn eval_cond(
        &self,
        clauses: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
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
                        // Regular else clause - evaluate all expressions
                        return self.eval_begin(exprs, env);
                    }
                }

                // Evaluate the test
                let test_result = self.eval_in_env(test, env)?;

                if test_result.is_truthy() {
                    // Test succeeded - check for => syntax
                    if let Some(result) = self.eval_arrow_syntax(exprs, test_result, env, "cond")? {
                        return Ok(result);
                    }

                    // Regular clause - evaluate expressions
                    return self.eval_begin(exprs, env);
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "cond clause must be a list".to_string(),
                ));
            }

            current = clause_pair.1.clone();
        }

        // No clause matched and no else - return unspecified
        Ok(Value::Unspecified)
    }

    /// Evaluate case special form: (case key ((datum...) expr...)...)
    pub(super) fn eval_case(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
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
                            return Ok(result);
                        }
                        // Regular else clause
                        return self.eval_begin(exprs, env);
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
                        return Ok(result);
                    }

                    // Regular clause - evaluate expressions
                    return self.eval_begin(exprs, env);
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "case clause must be a list".to_string(),
                ));
            }

            current = clause_pair.1.clone();
        }

        // No clause matched and no else - return unspecified
        Ok(Value::Unspecified)
    }

    /// Evaluate begin special form: (begin expr...)
    pub(super) fn eval_begin(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let mut current = args.clone();
        let mut result = Value::Unspecified;

        while let Value::Pair(pair) = current {
            result = self.eval_in_env(&pair.0, env)?;
            current = pair.1.clone();
        }

        Ok(result)
    }

    /// Evaluate let special form: (let ((var val)...) body...)
    pub(super) fn eval_let(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings using shared helper
        let (names, value_exprs) = self.parse_simple_bindings(&bindings, "let")?;

        // Evaluate all value expressions in the current environment
        let mut values = Vec::new();
        for value_expr in value_exprs {
            values.push(self.eval_in_env(&value_expr, env)?);
        }

        // Create new environment and bind all variables
        let new_env = Rc::new(Environment::with_parent(env.clone()));
        for (name, value) in names.iter().zip(values.iter()) {
            new_env.define(name.to_string(), value.clone());
        }

        // Evaluate body in the new environment
        self.eval_begin(&rest, &new_env)
    }

    /// Evaluate let* special form: (let* ((var val)...) body...)
    pub(super) fn eval_let_star(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings using shared helper
        let (names, value_exprs) = self.parse_simple_bindings(&bindings, "let*")?;

        // Build environment incrementally
        let mut current_env = env.clone();

        for (name, value_expr) in names.iter().zip(value_exprs.iter()) {
            // Evaluate value in current environment
            let value = self.eval_in_env(value_expr, &current_env)?;

            // Create new environment extending current one
            let new_env = Rc::new(Environment::with_parent(current_env));
            new_env.define(name.to_string(), value);
            current_env = new_env;
        }

        // Evaluate body in the final environment
        self.eval_begin(&rest, &current_env)
    }

    /// Evaluate letrec special form: (letrec ((var val)...) body...)
    pub(super) fn eval_letrec(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings using shared helper
        let (names, value_exprs) = self.parse_simple_bindings(&bindings, "letrec")?;

        // Create new environment and bind all variables to Unspecified initially
        let new_env = Rc::new(Environment::with_parent(env.clone()));
        for name in &names {
            new_env.define(name.to_string(), Value::Unspecified);
        }

        // Now evaluate all value expressions in the new environment and update bindings
        for (name, value_expr) in names.iter().zip(value_exprs.iter()) {
            let value = self.eval_in_env(value_expr, &new_env)?;
            new_env
                .set(name, value)
                .map_err(EvalError::UndefinedVariable)?;
        }

        // Evaluate body in the new environment
        self.eval_begin(&rest, &new_env)
    }

    /// Evaluate letrec* special form: (letrec* ((var val)...) body...)
    pub(super) fn eval_letrec_star(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings using shared helper
        let (names, value_exprs) = self.parse_simple_bindings(&bindings, "letrec*")?;

        // Create new environment and bind all variables to Unspecified initially
        let new_env = Rc::new(Environment::with_parent(env.clone()));
        for name in &names {
            new_env.define(name.to_string(), Value::Unspecified);
        }

        // Evaluate and assign values sequentially (this is the difference from letrec)
        for (name, value_expr) in names.iter().zip(value_exprs.iter()) {
            let value = self.eval_in_env(value_expr, &new_env)?;
            new_env
                .set(name, value)
                .map_err(EvalError::UndefinedVariable)?;
        }

        // Evaluate body in the new environment
        self.eval_begin(&rest, &new_env)
    }

    /// Evaluate let-values special form: (let-values (((formals) expr)...) body...)
    pub(super) fn eval_let_values(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Create new environment
        let new_env = Rc::new(Environment::with_parent(env.clone()));

        // Process each binding
        let mut current = bindings.clone();
        while let Value::Pair(binding_pair) = current {
            let binding = &binding_pair.0;

            // Each binding should be ((formals) expr)
            if let Value::Pair(binding_contents) = binding {
                let formals = &binding_contents.0;

                if let Value::Pair(expr_pair) = &binding_contents.1 {
                    if !matches!(expr_pair.1, Value::Null) {
                        return Err(EvalError::InvalidSyntax(
                            "let-values binding must have exactly 2 elements".to_string(),
                        ));
                    }

                    let expr = &expr_pair.0;

                    // Evaluate the expression to get values
                    let values = self.eval_in_env(expr, env)?;

                    // Bind the values to the formals
                    self.bind_values_to_formals(formals, &values, &new_env)?;
                } else {
                    return Err(EvalError::InvalidSyntax(
                        "let-values binding must have an expression".to_string(),
                    ));
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "let-values binding must be a list".to_string(),
                ));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "let-values bindings must be a proper list".to_string(),
            ));
        }

        // Evaluate body in new environment
        self.eval_begin(&rest, &new_env)
    }

    /// Evaluate let*-values special form: (let*-values (((formals) expr)...) body...)
    pub(super) fn eval_let_star_values(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (bindings, rest) = self.extract_pair(args)?;

        // Empty bindings - just evaluate body
        if matches!(bindings, Value::Null) {
            return self.eval_begin(&rest, env);
        }

        // Process first binding
        if let Value::Pair(first_pair) = bindings {
            let first_binding = &first_pair.0;
            let remaining_bindings = &first_pair.1;

            // Parse first binding: ((formals) expr)
            if let Value::Pair(binding_contents) = first_binding {
                let formals = &binding_contents.0;

                if let Value::Pair(expr_pair) = &binding_contents.1 {
                    if !matches!(expr_pair.1, Value::Null) {
                        return Err(EvalError::InvalidSyntax(
                            "let*-values binding must have exactly 2 elements".to_string(),
                        ));
                    }

                    let expr = &expr_pair.0;

                    // Evaluate the expression
                    let values = self.eval_in_env(expr, env)?;

                    // Create new environment with these bindings
                    let new_env = Rc::new(Environment::with_parent(env.clone()));
                    self.bind_values_to_formals(formals, &values, &new_env)?;

                    // Recursively process remaining bindings in new environment
                    if matches!(remaining_bindings, Value::Null) {
                        // No more bindings, evaluate body
                        self.eval_begin(&rest, &new_env)
                    } else {
                        // More bindings - recurse with let*-values
                        // Build: (remaining_bindings . body)
                        let new_args =
                            Value::Pair(Rc::new((remaining_bindings.clone(), rest.clone())));
                        self.eval_let_star_values(&new_args, &new_env)
                    }
                } else {
                    Err(EvalError::InvalidSyntax(
                        "let*-values binding must have an expression".to_string(),
                    ))
                }
            } else {
                Err(EvalError::InvalidSyntax(
                    "let*-values binding must be a list".to_string(),
                ))
            }
        } else {
            Err(EvalError::InvalidSyntax(
                "let*-values bindings must be a proper list".to_string(),
            ))
        }
    }

    /// Helper to bind values to formals
    pub(super) fn bind_values_to_formals(
        &self,
        formals: &Value,
        values: &Value,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        match formals {
            // Single identifier - bind all values (or single value)
            Value::Symbol(name) => {
                env.define(name.to_string(), values.clone());
                Ok(())
            }

            // List of identifiers - extract values and bind each
            Value::Pair(_) => {
                // Extract value list
                let val_list = match values {
                    Value::Values(vals) => vals.clone(),
                    other => vec![other.clone()],
                };

                // Extract formals list
                let mut formal_names = Vec::new();
                let mut current = formals.clone();

                while let Value::Pair(pair) = current {
                    if let Value::Symbol(name) = &pair.0 {
                        formal_names.push(name.clone());
                        current = pair.1.clone();
                    } else {
                        return Err(EvalError::InvalidSyntax(
                            "Formals must be symbols".to_string(),
                        ));
                    }
                }

                // Handle dotted list (rest parameter)
                let has_rest = if let Value::Symbol(rest_name) = current {
                    formal_names.push(rest_name);
                    true
                } else if !matches!(current, Value::Null) {
                    return Err(EvalError::InvalidSyntax("Invalid formals list".to_string()));
                } else {
                    false
                };

                // Bind values to formals
                if has_rest {
                    // Last formal gets remaining values as a list
                    let fixed_count = formal_names.len() - 1;
                    if val_list.len() < fixed_count {
                        return Err(EvalError::WrongArity {
                            expected: format!("at least {}", fixed_count),
                            actual: val_list.len(),
                        });
                    }

                    for (i, name) in formal_names.iter().take(fixed_count).enumerate() {
                        env.define(name.to_string(), val_list[i].clone());
                    }

                    let rest_values = self.list_from_vec(val_list[fixed_count..].to_vec());
                    env.define(formal_names[fixed_count].to_string(), rest_values);
                } else {
                    // Exact match required
                    if val_list.len() != formal_names.len() {
                        return Err(EvalError::WrongArity {
                            expected: formal_names.len().to_string(),
                            actual: val_list.len(),
                        });
                    }

                    for (name, value) in formal_names.iter().zip(val_list.iter()) {
                        env.define(name.to_string(), value.clone());
                    }
                }

                Ok(())
            }

            // Empty list
            Value::Null => {
                // No formals, values should be empty too
                match values {
                    Value::Values(vals) if vals.is_empty() => Ok(()),
                    Value::Values(vals) => Err(EvalError::WrongArity {
                        expected: "0".to_string(),
                        actual: vals.len(),
                    }),
                    _ => Err(EvalError::WrongArity {
                        expected: "0".to_string(),
                        actual: 1,
                    }),
                }
            }

            _ => Err(EvalError::InvalidSyntax(
                "Invalid formals in let-values".to_string(),
            )),
        }
    }

    /// Evaluate and special form: (and test...)
    pub(super) fn eval_and(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let mut current = args.clone();

        // Empty (and) returns #t
        if matches!(current, Value::Null) {
            return Ok(Value::Boolean(true));
        }

        // Evaluate tests sequentially until we find a false one or reach the end
        loop {
            match current {
                Value::Pair(ref pair) => {
                    let test_result = self.eval_in_env(&pair.0, env)?;

                    // If this is the last test, return its value (not just #t/#f)
                    if matches!(pair.1, Value::Null) {
                        return Ok(test_result);
                    }

                    // If test is false, short-circuit and return #f
                    if !test_result.is_truthy() {
                        return Ok(Value::Boolean(false));
                    }

                    // Continue with rest
                    current = pair.1.clone();
                }
                Value::Null => {
                    // Should not reach here due to earlier check
                    return Ok(Value::Boolean(true));
                }
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "and expects a list of expressions".to_string(),
                    ));
                }
            }
        }
    }

    /// Evaluate or special form: (or test...)
    pub(super) fn eval_or(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let mut current = args.clone();

        // Empty (or) returns #f
        if matches!(current, Value::Null) {
            return Ok(Value::Boolean(false));
        }

        // Evaluate tests sequentially until we find a true one or reach the end
        loop {
            match current {
                Value::Pair(ref pair) => {
                    let test_result = self.eval_in_env(&pair.0, env)?;

                    // If this is the last test, return its value (not just #t/#f)
                    if matches!(pair.1, Value::Null) {
                        return Ok(test_result);
                    }

                    // If test is true, short-circuit and return its value
                    if test_result.is_truthy() {
                        return Ok(test_result);
                    }

                    // Continue with rest
                    current = pair.1.clone();
                }
                Value::Null => {
                    // Should not reach here due to earlier check
                    return Ok(Value::Boolean(false));
                }
                _ => {
                    return Err(EvalError::InvalidSyntax(
                        "or expects a list of expressions".to_string(),
                    ));
                }
            }
        }
    }

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
    /// R7RS Section 4.2.4: Iteration
    ///
    /// Semantics:
    /// 1. Evaluate all init expressions and bind variables
    /// 2. Loop:
    ///    - Evaluate test
    ///    - If true: evaluate result expressions and return last value
    ///    - If false: evaluate commands, then evaluate steps and update bindings
    /// 3. If step is omitted, variable doesn't change between iterations
    pub(super) fn eval_do(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
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
                let mut last_result = Value::Unspecified;
                for result_expr in &result_exprs {
                    last_result = self.eval_in_env(result_expr, &loop_env)?;
                }
                return Ok(last_result);
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
                    loop_env.get(var_name).ok_or_else(|| {
                        EvalError::UndefinedVariable(var_name.to_string())
                    })?
                };
                new_values.push((var_name.clone(), new_val));
            }

            // Update all bindings atomically
            for (var_name, new_val) in new_values {
                loop_env.set(&var_name, new_val)
                    .map_err(EvalError::UndefinedVariable)?;
            }
        }
    }

    /// Parse do bindings: ((var init step?) ...)
    /// Returns: Vec<(var_name, init_expr, step_expr_opt)>
    fn parse_do_bindings(
        &self,
        bindings_expr: &Value,
    ) -> Result<Vec<DoBinding>, EvalError> {
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
    fn parse_do_test_clause(&self, test_clause_expr: &Value) -> Result<(Value, Vec<Value>), EvalError> {
        let (test_expr, rest) = self.extract_pair(test_clause_expr)?;
        let result_exprs = self.collect_list_items(&rest)?;
        Ok((test_expr.clone(), result_exprs))
    }
}
