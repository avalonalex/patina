use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("Undefined variable: {0}")]
    UndefinedVariable(String),

    #[error("Not a procedure: {0}")]
    NotAProcedure(String),

    #[error("Wrong number of arguments: expected {expected}, got {actual}")]
    WrongArity { expected: String, actual: usize },

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Division by zero")]
    DivisionByZero,
}

pub struct Evaluator {
    global_env: Rc<Environment>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        Self::install_primitives(&global_env);
        Evaluator { global_env }
    }

    pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
        self.eval_in_env(expr, &self.global_env)
    }

    fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        match expr {
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
            Value::Symbol(name) => env
                .get(name)
                .ok_or_else(|| EvalError::UndefinedVariable(name.to_string())),

            // Empty list
            Value::Null => Ok(Value::Null),

            // Lists (procedure calls or special forms)
            Value::Pair(_) => self.eval_list(expr, env),

            _ => Ok(expr.clone()),
        }
    }

    fn eval_list(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Check for special forms
        if let Value::Symbol(ref sym) = car {
            match sym.as_ref() {
                "quote" => return self.eval_quote(&cdr),
                "if" => return self.eval_if(&cdr, env),
                "define" => return self.eval_define(&cdr, env),
                "set!" => return self.eval_set(&cdr, env),
                "lambda" => return self.eval_lambda(&cdr, env),
                "begin" => return self.eval_begin(&cdr, env),
                "cond" => return self.eval_cond(&cdr, env),
                "let" => return self.eval_let(&cdr, env),
                "let*" => return self.eval_let_star(&cdr, env),
                "letrec" => return self.eval_letrec(&cdr, env),
                "letrec*" => return self.eval_letrec_star(&cdr, env),
                "and" => return self.eval_and(&cdr, env),
                "or" => return self.eval_or(&cdr, env),
                "apply" => return self.eval_apply(&cdr, env),
                _ => {}
            }
        }

        // Regular procedure call
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;
        self.apply(proc, args)
    }

    fn extract_pair(&self, expr: &Value) -> Result<(Value, Value), EvalError> {
        match expr {
            Value::Pair(pair) => Ok((pair.0.clone(), pair.1.clone())),
            _ => Err(EvalError::InvalidSyntax("Expected a pair".to_string())),
        }
    }

    fn eval_quote(&self, args: &Value) -> Result<Value, EvalError> {
        let (quoted, rest) = self.extract_pair(args)?;
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quote expects exactly one argument".to_string(),
            ));
        }
        Ok(quoted)
    }

    fn eval_if(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
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

    fn eval_define(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
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
                Err(EvalError::InvalidSyntax(
                    "Function define shorthand not yet implemented".to_string(),
                ))
            }
            _ => Err(EvalError::InvalidSyntax(
                "define expects a symbol or list".to_string(),
            )),
        }
    }

    fn eval_set(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
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

    fn eval_lambda(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (lambda params body...)
        // params can be:
        // - (x y z) - fixed arity
        // - args - variadic (all args go to single parameter)
        // - (x y . rest) - mixed (first n are fixed, rest go to rest parameter)

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

    fn eval_cond(&self, clauses: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (cond (test1 expr1 ...) (test2 expr2 ...) ... [(else exprN ...)])
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
                        // else clause - evaluate all expressions
                        return self.eval_begin(exprs, env);
                    }
                }

                // Evaluate the test
                let test_result = self.eval_in_env(test, env)?;

                if test_result.is_truthy() {
                    // Test succeeded - evaluate expressions in this clause
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

    fn eval_begin(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let mut current = args.clone();
        let mut result = Value::Unspecified;

        while let Value::Pair(pair) = current {
            result = self.eval_in_env(&pair.0, env)?;
            current = pair.1.clone();
        }

        Ok(result)
    }

    fn eval_let(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (let ((var1 val1) (var2 val2) ...) body ...)
        // Desugars to: ((lambda (var1 var2 ...) body ...) val1 val2 ...)

        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings into (names, values)
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
                            return Err(EvalError::InvalidSyntax(
                                "let binding must have exactly 2 elements".to_string(),
                            ));
                        }
                        value_exprs.push(value_pair.0.clone());
                    } else {
                        return Err(EvalError::InvalidSyntax(
                            "let binding must have a value".to_string(),
                        ));
                    }
                } else {
                    return Err(EvalError::InvalidSyntax(
                        "let binding name must be a symbol".to_string(),
                    ));
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "let binding must be a list".to_string(),
                ));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "let bindings must be a proper list".to_string(),
            ));
        }

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

    fn eval_let_star(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (let* ((var1 val1) (var2 val2) ...) body ...)
        // Sequential binding: each binding can see previous ones

        let (bindings, rest) = self.extract_pair(args)?;

        // Build environment incrementally
        let mut current_env = env.clone();
        let mut current = bindings.clone();

        while let Value::Pair(binding_pair) = current {
            // Each binding should be (name value)
            let binding = &binding_pair.0;
            if let Value::Pair(binding_contents) = binding {
                if let Value::Symbol(name) = &binding_contents.0 {
                    // Extract value expression
                    if let Value::Pair(value_pair) = &binding_contents.1 {
                        if !matches!(value_pair.1, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "let* binding must have exactly 2 elements".to_string(),
                            ));
                        }

                        // Evaluate value in current environment
                        let value = self.eval_in_env(&value_pair.0, &current_env)?;

                        // Create new environment extending current one
                        let new_env = Rc::new(Environment::with_parent(current_env));
                        new_env.define(name.to_string(), value);
                        current_env = new_env;
                    } else {
                        return Err(EvalError::InvalidSyntax(
                            "let* binding must have a value".to_string(),
                        ));
                    }
                } else {
                    return Err(EvalError::InvalidSyntax(
                        "let* binding name must be a symbol".to_string(),
                    ));
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "let* binding must be a list".to_string(),
                ));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "let* bindings must be a proper list".to_string(),
            ));
        }

        // Evaluate body in the final environment
        self.eval_begin(&rest, &current_env)
    }

    fn eval_letrec(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (letrec ((var1 val1) (var2 val2) ...) body ...)
        // All variables are bound first (to undefined), then values are assigned
        // This allows mutual recursion

        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings into (names, value_exprs)
        let mut names = Vec::new();
        let mut value_exprs = Vec::new();
        let mut current = bindings.clone();

        while let Value::Pair(binding_pair) = current {
            let binding = &binding_pair.0;
            if let Value::Pair(binding_contents) = binding {
                if let Value::Symbol(name) = &binding_contents.0 {
                    names.push(name.clone());

                    if let Value::Pair(value_pair) = &binding_contents.1 {
                        if !matches!(value_pair.1, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "letrec binding must have exactly 2 elements".to_string(),
                            ));
                        }
                        value_exprs.push(value_pair.0.clone());
                    } else {
                        return Err(EvalError::InvalidSyntax(
                            "letrec binding must have a value".to_string(),
                        ));
                    }
                } else {
                    return Err(EvalError::InvalidSyntax(
                        "letrec binding name must be a symbol".to_string(),
                    ));
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "letrec binding must be a list".to_string(),
                ));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "letrec bindings must be a proper list".to_string(),
            ));
        }

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

    fn eval_letrec_star(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (letrec* ((var1 val1) (var2 val2) ...) body ...)
        // Like letrec but values are evaluated and assigned sequentially

        let (bindings, rest) = self.extract_pair(args)?;

        // Parse bindings into (names, value_exprs)
        let mut names = Vec::new();
        let mut value_exprs = Vec::new();
        let mut current = bindings.clone();

        while let Value::Pair(binding_pair) = current {
            let binding = &binding_pair.0;
            if let Value::Pair(binding_contents) = binding {
                if let Value::Symbol(name) = &binding_contents.0 {
                    names.push(name.clone());

                    if let Value::Pair(value_pair) = &binding_contents.1 {
                        if !matches!(value_pair.1, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "letrec* binding must have exactly 2 elements".to_string(),
                            ));
                        }
                        value_exprs.push(value_pair.0.clone());
                    } else {
                        return Err(EvalError::InvalidSyntax(
                            "letrec* binding must have a value".to_string(),
                        ));
                    }
                } else {
                    return Err(EvalError::InvalidSyntax(
                        "letrec* binding name must be a symbol".to_string(),
                    ));
                }
            } else {
                return Err(EvalError::InvalidSyntax(
                    "letrec* binding must be a list".to_string(),
                ));
            }

            current = binding_pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "letrec* bindings must be a proper list".to_string(),
            ));
        }

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

    fn eval_and(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (and) => #t
        // (and test) => test
        // (and test1 test2 ...) => if test1 is false, return #f; else continue
        // Short-circuits on first false value

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

    fn eval_or(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (or) => #f
        // (or test) => test
        // (or test1 test2 ...) => if test1 is true, return it; else continue
        // Short-circuits on first true value

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

    fn eval_apply(&self, args: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // (apply proc arg1 ... args)
        // where args is a list, and all arg1... are prepended to that list
        // Minimum 2 arguments: proc and final list

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

    fn eval_arguments(&self, args: &Value, env: &Rc<Environment>) -> Result<Vec<Value>, EvalError> {
        let mut result = Vec::new();
        let mut current = args.clone();

        while let Value::Pair(pair) = current {
            result.push(self.eval_in_env(&pair.0, env)?);
            current = pair.1.clone();
        }

        Ok(result)
    }

    fn apply(&self, proc: Value, args: Vec<Value>) -> Result<Value, EvalError> {
        match proc {
            Value::Procedure(Procedure::Primitive { name, arity }) => {
                self.check_arity(&arity, args.len())?;
                self.apply_primitive(name, args)
            }
            Value::Procedure(Procedure::Lambda {
                params,
                variadic,
                body,
                env,
            }) => {
                // Check arity
                if variadic.is_some() {
                    // Variadic: need at least as many args as fixed params
                    if args.len() < params.len() {
                        return Err(EvalError::WrongArity {
                            expected: format!("at least {}", params.len()),
                            actual: args.len(),
                        });
                    }
                } else {
                    // Fixed arity: need exact number of args
                    if args.len() != params.len() {
                        return Err(EvalError::WrongArity {
                            expected: params.len().to_string(),
                            actual: args.len(),
                        });
                    }
                }

                // Create new environment with lambda's captured environment as parent
                let new_env = Rc::new(Environment::with_parent(env));

                // Bind fixed parameters
                for (param, arg) in params.iter().zip(args.iter()) {
                    new_env.define(param.clone(), arg.clone());
                }

                // Bind rest parameter if variadic
                if let Some(rest_param) = variadic {
                    // Collect remaining args into a list
                    let rest_args: Vec<Value> = args.into_iter().skip(params.len()).collect();
                    let rest_list = self.list_from_vec(rest_args);
                    new_env.define(rest_param, rest_list);
                }

                // Evaluate body expressions in sequence
                let mut result = Value::Unspecified;
                for expr in body {
                    result = self.eval_in_env(&expr, &new_env)?;
                }

                Ok(result)
            }
            _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
        }
    }

    fn check_arity(&self, arity: &Arity, actual: usize) -> Result<(), EvalError> {
        match arity {
            Arity::Exact(n) if *n != actual => Err(EvalError::WrongArity {
                expected: n.to_string(),
                actual,
            }),
            Arity::Min(n) if actual < *n => Err(EvalError::WrongArity {
                expected: format!("at least {}", n),
                actual,
            }),
            Arity::Range(min, max) if actual < *min || actual > *max => {
                Err(EvalError::WrongArity {
                    expected: format!("{}-{}", min, max),
                    actual,
                })
            }
            _ => Ok(()),
        }
    }

    fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            "+" => self.primitive_add(args),
            "-" => self.primitive_subtract(args),
            "*" => self.primitive_multiply(args),
            "/" => self.primitive_divide(args),
            "=" => self.primitive_numeric_equal(args),
            "<" => self.primitive_less_than(args),
            ">" => self.primitive_greater_than(args),
            "<=" => self.primitive_less_equal(args),
            ">=" => self.primitive_greater_equal(args),
            "cons" => self.primitive_cons(args),
            "car" => self.primitive_car(args),
            "cdr" => self.primitive_cdr(args),
            "null?" => self.primitive_null_p(args),
            "pair?" => self.primitive_pair_p(args),
            "list" => Ok(self.list_from_vec(args)),
            "map" => self.primitive_map(args),
            "for-each" => self.primitive_for_each(args),
            "number?" => self.primitive_number_p(args),
            "integer?" => self.primitive_integer_p(args),
            "boolean?" => self.primitive_boolean_p(args),
            "string?" => self.primitive_string_p(args),
            "symbol?" => self.primitive_symbol_p(args),
            "eq?" => self.primitive_eq(args),
            "eqv?" => self.primitive_eqv(args),
            "equal?" => self.primitive_equal(args),
            _ => Err(EvalError::InvalidSyntax(format!(
                "Unknown primitive: {}",
                name
            ))),
        }
    }

    fn list_from_vec(&self, items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, item| Value::Pair(Rc::new((item, acc))))
    }

    // Primitive implementations
    fn primitive_add(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result = 0i64;
        for arg in args {
            if let Value::Integer(n) = arg {
                result += n;
            } else {
                return Err(EvalError::TypeError(format!(
                    "+ expects numbers, got {}",
                    arg
                )));
            }
        }
        Ok(Value::Integer(result))
    }

    fn primitive_subtract(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        if let Value::Integer(first) = args[0] {
            if args.len() == 1 {
                return Ok(Value::Integer(-first));
            }

            let mut result = first;
            for arg in &args[1..] {
                if let Value::Integer(n) = arg {
                    result -= n;
                } else {
                    return Err(EvalError::TypeError(format!(
                        "- expects numbers, got {}",
                        arg
                    )));
                }
            }
            Ok(Value::Integer(result))
        } else {
            Err(EvalError::TypeError(format!(
                "- expects numbers, got {}",
                args[0]
            )))
        }
    }

    fn primitive_multiply(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result = 1i64;
        for arg in args {
            if let Value::Integer(n) = arg {
                result *= n;
            } else {
                return Err(EvalError::TypeError(format!(
                    "* expects numbers, got {}",
                    arg
                )));
            }
        }
        Ok(Value::Integer(result))
    }

    fn primitive_divide(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        if let Value::Integer(first) = args[0] {
            if args.len() == 1 {
                if first == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                return Ok(Value::Real(1.0 / first as f64));
            }

            // When dividing multiple numbers, use floating point
            let mut result = first as f64;
            for arg in &args[1..] {
                if let Value::Integer(n) = arg {
                    if *n == 0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    result /= *n as f64;
                } else {
                    return Err(EvalError::TypeError(format!(
                        "/ expects numbers, got {}",
                        arg
                    )));
                }
            }
            Ok(Value::Real(result))
        } else {
            Err(EvalError::TypeError(format!(
                "/ expects numbers, got {}",
                args[0]
            )))
        }
    }

    fn primitive_numeric_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        if let Value::Integer(first) = args[0] {
            for arg in &args[1..] {
                if let Value::Integer(n) = arg {
                    if first != *n {
                        return Ok(Value::Boolean(false));
                    }
                } else {
                    return Err(EvalError::TypeError("= expects numbers".to_string()));
                }
            }
            Ok(Value::Boolean(true))
        } else {
            Err(EvalError::TypeError("= expects numbers".to_string()))
        }
    }

    fn primitive_less_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            if let (Value::Integer(a), Value::Integer(b)) = (&args[i], &args[i + 1]) {
                if a >= b {
                    return Ok(Value::Boolean(false));
                }
            } else {
                return Err(EvalError::TypeError("< expects numbers".to_string()));
            }
        }
        Ok(Value::Boolean(true))
    }

    fn primitive_greater_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            if let (Value::Integer(a), Value::Integer(b)) = (&args[i], &args[i + 1]) {
                if a <= b {
                    return Ok(Value::Boolean(false));
                }
            } else {
                return Err(EvalError::TypeError("> expects numbers".to_string()));
            }
        }
        Ok(Value::Boolean(true))
    }

    fn primitive_less_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            if let (Value::Integer(a), Value::Integer(b)) = (&args[i], &args[i + 1]) {
                if a > b {
                    return Ok(Value::Boolean(false));
                }
            } else {
                return Err(EvalError::TypeError("<= expects numbers".to_string()));
            }
        }
        Ok(Value::Boolean(true))
    }

    fn primitive_greater_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        for i in 0..args.len() - 1 {
            if let (Value::Integer(a), Value::Integer(b)) = (&args[i], &args[i + 1]) {
                if a < b {
                    return Ok(Value::Boolean(false));
                }
            } else {
                return Err(EvalError::TypeError(">= expects numbers".to_string()));
            }
        }
        Ok(Value::Boolean(true))
    }

    fn primitive_cons(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Pair(Rc::new((args[0].clone(), args[1].clone()))))
    }

    fn primitive_car(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Pair(pair) => Ok(pair.0.clone()),
            _ => Err(EvalError::TypeError("car expects a pair".to_string())),
        }
    }

    fn primitive_cdr(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Pair(pair) => Ok(pair.1.clone()),
            _ => Err(EvalError::TypeError("cdr expects a pair".to_string())),
        }
    }

    fn primitive_null_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Null)))
    }

    fn primitive_pair_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Pair(_))))
    }

    fn primitive_number_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_)
                | Value::BigInteger(_)
                | Value::Rational(_)
                | Value::Real(_)
                | Value::Complex(_, _)
        )))
    }

    fn primitive_integer_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_) | Value::BigInteger(_)
        )))
    }

    fn primitive_boolean_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Boolean(_))))
    }

    fn primitive_string_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::String(_))))
    }

    fn primitive_symbol_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Symbol(_))))
    }

    fn primitive_eq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        // eq? checks identity/pointer equality
        // For symbols, booleans, null - compare identity
        // For numbers, strings, pairs - only same if same object
        let result = match (&args[0], &args[1]) {
            // Booleans
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            // Null
            (Value::Null, Value::Null) => true,
            // Symbols - compare by content (since we don't intern symbols yet)
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            // For other types, eq? is stricter - only same object
            _ => false,
        };
        Ok(Value::Boolean(result))
    }

    fn primitive_eqv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        // eqv? is like eq? but compares numbers and characters by value
        let result = match (&args[0], &args[1]) {
            // Same as eq? for booleans, null, symbols
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            // Numbers - compare by value
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            // Characters - compare by value
            (Value::Character(a), Value::Character(b)) => a == b,
            // Different types or other types
            _ => false,
        };
        Ok(Value::Boolean(result))
    }

    fn primitive_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Ok(Value::Boolean(Self::values_equal(&args[0], &args[1])))
    }

    fn values_equal(a: &Value, b: &Value) -> bool {
        match (a, b) {
            // Primitives
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Integer(x), Value::Integer(y)) => x == y,
            (Value::Real(x), Value::Real(y)) => x == y,
            (Value::Character(x), Value::Character(y)) => x == y,
            (Value::Null, Value::Null) => true,

            // Strings - compare content
            (Value::String(x), Value::String(y)) => x.as_ref() == y.as_ref(),

            // Symbols - compare by content
            (Value::Symbol(x), Value::Symbol(y)) => x.as_ref() == y.as_ref(),

            // Pairs - recursively compare car and cdr
            (Value::Pair(x), Value::Pair(y)) => {
                Self::values_equal(&x.0, &y.0) && Self::values_equal(&x.1, &y.1)
            }

            // Vectors - compare length and elements
            (Value::Vector(x), Value::Vector(y)) => {
                x.len() == y.len()
                    && x.iter()
                        .zip(y.iter())
                        .all(|(a, b)| Self::values_equal(a, b))
            }

            // Different types
            _ => false,
        }
    }

    fn primitive_map(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        // (map proc list1 list2 ...)
        // Apply proc element-wise to corresponding elements from all lists
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        // Convert all lists to vectors for easier processing
        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            // Check for proper list
            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "map: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        // Find the length of the shortest list
        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        // Apply proc to each set of corresponding elements
        let mut results = Vec::new();
        for i in 0..min_len {
            // Collect the i-th element from each list
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            // Apply the procedure
            let result = self.apply(proc.clone(), proc_args)?;
            results.push(result);
        }

        Ok(self.list_from_vec(results))
    }

    fn primitive_for_each(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        // (for-each proc list1 list2 ...)
        // Like map but for side effects, returns unspecified
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        // Convert all lists to vectors
        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            // Check for proper list
            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "for-each: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        // Find the length of the shortest list
        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        // Apply proc to each set of corresponding elements (in order, for side effects)
        for i in 0..min_len {
            // Collect the i-th element from each list
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            // Apply the procedure (we discard the result)
            self.apply(proc.clone(), proc_args)?;
        }

        // for-each returns unspecified
        Ok(Value::Unspecified)
    }

    fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            ("+", Arity::Min(0)),
            ("-", Arity::Min(1)),
            ("*", Arity::Min(0)),
            ("/", Arity::Min(1)),
            ("=", Arity::Min(2)),
            ("<", Arity::Min(2)),
            (">", Arity::Min(2)),
            ("<=", Arity::Min(2)),
            (">=", Arity::Min(2)),
            ("cons", Arity::Exact(2)),
            ("car", Arity::Exact(1)),
            ("cdr", Arity::Exact(1)),
            ("null?", Arity::Exact(1)),
            ("pair?", Arity::Exact(1)),
            ("list", Arity::Min(0)),
            ("map", Arity::Min(2)),
            ("for-each", Arity::Min(2)),
            ("number?", Arity::Exact(1)),
            ("integer?", Arity::Exact(1)),
            ("boolean?", Arity::Exact(1)),
            ("string?", Arity::Exact(1)),
            ("symbol?", Arity::Exact(1)),
            ("eq?", Arity::Exact(2)),
            ("eqv?", Arity::Exact(2)),
            ("equal?", Arity::Exact(2)),
        ];

        for (name, arity) in primitives {
            env.define(
                name.to_string(),
                Value::Procedure(Procedure::Primitive { name, arity }),
            );
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
