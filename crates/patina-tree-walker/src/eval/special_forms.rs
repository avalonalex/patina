//! Special forms evaluation
//!
//! This module implements evaluation for Scheme special forms including:
//! - `quote` - Quote expressions
//! - `if` - Conditional evaluation
//! - `define` - Variable and function definition
//! - `set!` - Assignment
//! - `lambda` - Procedure creation
//! - `begin` - Sequential evaluation
//! - `apply` - Apply procedure to list of arguments
//! - `do` - Iteration construct
//!
//! NOTE: Many derived expressions (cond, case, let, let*, letrec, and, or, etc.)
//! are now implemented as macros in lib/bootstrap.scm rather than special forms.

use patina_runtime::environment::Environment;
use patina_runtime::value::{Procedure, Value};
use std::cell::RefCell;
use std::rc::Rc;

use super::Evaluator;
use super::error::EvalError;

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

    // NOTE: eval_arrow_syntax, ensure_proper_list, and expect_symbol were removed
    // as they're now handled by cond/case macros or no longer needed after macro migrations.

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

    /// Evaluate quasiquote special form: (quasiquote template)
    ///
    /// Quasiquote is like quote, but allows selective evaluation via unquote (,) and
    /// unquote-splicing (,@). It tracks nesting depth to handle nested quasiquotes.
    ///
    /// Examples:
    /// - `(a b c) => (a b c)  (like quote)
    /// - `(a ,(+ 1 2) c) => (a 3 c)  (unquote evaluates)
    /// - `(a ,@(list 1 2) b) => (a 1 2 b)  (splicing)
    /// - ``(a ,,x) => `(a ,value-of-x)  (nested)
    pub(super) fn eval_quasiquote(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        let (template, rest) = self.extract_pair(args)?;
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quasiquote expects exactly one argument".to_string(),
            ));
        }
        self.eval_quasiquote_impl(&template, env, 0)
    }

    /// Implementation of quasiquote with depth tracking
    ///
    /// The depth parameter tracks nesting level:
    /// - depth 0: at current quasiquote level (unquotes are active)
    /// - depth > 0: inside nested quasiquote (unquotes become quoted)
    fn eval_quasiquote_impl(
        &self,
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
                let list = self.vector_to_list(&vec.borrow());
                let processed = self.eval_quasiquote_impl(&list, env, depth)?;
                self.list_to_vector(&processed)
            }

            // Pairs: the interesting case
            Value::Pair(pair) => {
                let car = &pair.0;
                let cdr = &pair.1;

                // Check if car is a symbol that requires special handling
                if let Value::Symbol(sym) = car {
                    match sym.as_ref() {
                        // Nested quasiquote: increment depth
                        "quasiquote" => {
                            let (inner, rest) = self.extract_pair(cdr)?;
                            if !matches!(rest, Value::Null) {
                                return Err(EvalError::InvalidSyntax(
                                    "quasiquote expects exactly one argument".to_string(),
                                ));
                            }
                            let processed = self.eval_quasiquote_impl(&inner, env, depth + 1)?;
                            Ok(self.list_from_vec(vec![
                                Value::Symbol(Rc::from("quasiquote")),
                                processed,
                            ]))
                        }

                        // Unquote: evaluate if at depth 0, otherwise decrement depth
                        "unquote" => {
                            let (inner, rest) = self.extract_pair(cdr)?;
                            if !matches!(rest, Value::Null) {
                                return Err(EvalError::InvalidSyntax(
                                    "unquote expects exactly one argument".to_string(),
                                ));
                            }

                            if depth == 0 {
                                // At quasiquote level: evaluate the unquoted expression
                                self.eval_in_env(&inner, env)
                            } else {
                                // Inside nested quasiquote: preserve unquote, decrement depth
                                let processed =
                                    self.eval_quasiquote_impl(&inner, env, depth - 1)?;
                                Ok(self.list_from_vec(vec![
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
                                let (inner, rest) = self.extract_pair(cdr)?;
                                if !matches!(rest, Value::Null) {
                                    return Err(EvalError::InvalidSyntax(
                                        "unquote-splicing expects exactly one argument".to_string(),
                                    ));
                                }
                                let processed =
                                    self.eval_quasiquote_impl(&inner, env, depth - 1)?;
                                Ok(self.list_from_vec(vec![
                                    Value::Symbol(Rc::from("unquote-splicing")),
                                    processed,
                                ]))
                            }
                        }

                        _ => {
                            // Regular symbol: process as normal pair
                            self.process_quasiquote_pair(expr, env, depth)
                        }
                    }
                } else {
                    // Non-symbol car: process as normal pair
                    self.process_quasiquote_pair(expr, env, depth)
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
        &self,
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
            match &current {
                Value::Null => break,
                Value::Pair(pair) => {
                    let car = &pair.0;
                    let cdr = &pair.1;

                    // Check if CDR is an unquote form (for improper lists like (a . ,x))
                    // We need to check this BEFORE processing CAR
                    if depth == 0
                        && let Value::Pair(cdr_pair) = cdr
                        && let Value::Symbol(sym) = &cdr_pair.0
                        && sym.as_ref() == "unquote"
                    {
                        // This is an improper list: (... car . ,expr)
                        // Process car normally, then evaluate the unquote as tail
                        let processed_car = self.eval_quasiquote_impl(car, env, depth)?;
                        elements.push(processed_car);

                        // Evaluate the unquote expression
                        let (unquote_expr, rest) = self.extract_pair(&cdr_pair.1)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote expects exactly one argument".to_string(),
                            ));
                        }
                        tail = self.eval_in_env(&unquote_expr, env)?;
                        break;
                    }

                    // Check if this element is (unquote-splicing ...)
                    if depth == 0
                        && let Value::Pair(inner_pair) = car
                        && let Value::Symbol(sym) = &inner_pair.0
                        && sym.as_ref() == "unquote-splicing"
                    {
                        // Evaluate the splicing expression
                        let (splice_expr, rest) = self.extract_pair(&inner_pair.1)?;
                        if !matches!(rest, Value::Null) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing expects exactly one argument".to_string(),
                            ));
                        }

                        let splice_result = self.eval_in_env(&splice_expr, env)?;

                        // Must be a list
                        if !self.is_list(&splice_result) {
                            return Err(EvalError::InvalidSyntax(
                                "unquote-splicing result must be a list".to_string(),
                            ));
                        }

                        // Append all elements from the spliced list
                        let mut splice_current = splice_result;
                        while let Value::Pair(splice_pair) = splice_current {
                            elements.push(splice_pair.0.clone());
                            splice_current = splice_pair.1.clone();
                        }

                        current = cdr.clone();
                        continue;
                    }

                    // Regular element: process recursively
                    let processed = self.eval_quasiquote_impl(car, env, depth)?;
                    elements.push(processed);
                    current = cdr.clone();
                }
                _ => {
                    // Improper list (dotted pair with non-list tail)
                    tail = self.eval_quasiquote_impl(&current, env, depth)?;
                    break;
                }
            }
        }

        // Reconstruct the list
        if matches!(tail, Value::Null) {
            Ok(self.list_from_vec(elements))
        } else {
            // Improper list
            let mut result = tail;
            for elem in elements.iter().rev() {
                result = Value::Pair(Rc::new((elem.clone(), result)));
            }
            Ok(result)
        }
    }

    /// Helper: check if a value is a proper list
    fn is_list(&self, val: &Value) -> bool {
        let mut current = val;
        loop {
            match current {
                Value::Null => return true,
                Value::Pair(pair) => current = &pair.1,
                _ => return false,
            }
        }
    }

    /// Helper: convert vector to list
    fn vector_to_list(&self, vec: &[Value]) -> Value {
        self.list_from_vec(vec.to_vec())
    }

    /// Helper: convert list to vector
    fn list_to_vector(&self, list: &Value) -> Result<Value, EvalError> {
        let mut vec = Vec::new();
        let mut current = list;

        loop {
            match current {
                Value::Null => break,
                Value::Pair(pair) => {
                    vec.push(pair.0.clone());
                    current = &pair.1;
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
                ));
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
                super::EvalResult::TailCall { .. }
                | super::EvalResult::TailCallPrimitive { .. } => Err(EvalError::InternalError(
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
                        ));
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
                let lambda = Value::Procedure(patina_runtime::Procedure::Lambda {
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
                            ));
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

    // NOTE: 'cond' and 'case' are now implemented as macros in lib/bootstrap.scm
    // This reduces code duplication and aligns with R7RS reference implementations.
    // Previous Rust implementations (eval_cond_impl, eval_cond, eval_case_impl,
    // eval_case, and eval_arrow_syntax helper) were removed in favor of the macro approach.

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
                super::EvalResult::TailCall { .. }
                | super::EvalResult::TailCallPrimitive { .. } => Err(EvalError::InternalError(
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

    // NOTE: call-with-values was previously a special form for tail call optimization,
    // but has been migrated to a pure primitive that participates in TCO via TailCallPrimitive.
    // See src/eval/primitives/values.rs for the implementation.

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
        // Note: apply special form is never in tail position (legacy eval_list)
        match self.apply(proc, final_args, false)? {
            super::EvalResult::Value(v) => Ok(v),
            _ => Err(EvalError::InternalError(
                "Unexpected tail call in apply special form".to_string(),
            )),
        }
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
                ));
            }
        };

        // Transformer must be (syntax-rules (literals) rules...)
        let macro_def = self.parse_syntax_rules(&transformer_expr, env)?;

        // Create a Macro value with this name
        let macro_data = patina_frontend::macro_expander::Macro {
            name: name.clone(),
            rules: macro_def.rules,
            literals: macro_def.literals,
            env: env.clone(),
        };
        let macro_value = Value::Macro {
            name: name.clone(),
            data: Rc::new(macro_data),
        };

        // Bind the macro in the environment
        env.define(name.to_string(), macro_value);

        Ok(Value::Unspecified)
    }

    /// Parse a syntax-rules form: (syntax-rules (literals) (pattern template)...)
    fn parse_syntax_rules(
        &self,
        expr: &Value,
        env: &Rc<Environment>,
    ) -> Result<patina_frontend::macro_expander::Macro, EvalError> {
        // Must be a list starting with 'syntax-rules
        let (keyword, rest) = self.extract_pair(expr)?;

        match keyword {
            Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Expected syntax-rules".to_string(),
                ));
            }
        }

        // Parse literals list
        let (literals_expr, rules_expr) = self.extract_pair(&rest)?;

        let literals = self.parse_literals_list(&literals_expr)?;

        // Parse rules
        let rules = self.parse_macro_rules(&rules_expr)?;

        Ok(patina_frontend::macro_expander::Macro {
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
                    ));
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
    ) -> Result<Vec<patina_frontend::macro_expander::MacroRule>, EvalError> {
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
            let pattern = patina_frontend::macro_expander::parse_pattern(&pattern_expr)?;
            let template = patina_frontend::macro_expander::parse_template(&template_expr)?;

            rules.push(patina_frontend::macro_expander::MacroRule { pattern, template });

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
        macro_val: &patina_frontend::macro_expander::Macro,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        patina_frontend::macro_expander::expand_macro(macro_val, args, env).map_err(|e| e.into())
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
                super::EvalResult::TailCall { .. }
                | super::EvalResult::TailCallPrimitive { .. } => Err(EvalError::InternalError(
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
                    ));
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
                    ));
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

    /// Evaluate import special form: (import import-set ...)
    ///
    /// R7RS Section 5.2: Import declarations
    ///
    /// Import sets can be:
    /// - (library name) - direct library import
    /// - (only import-set identifier ...) - import only specified identifiers
    /// - (except import-set identifier ...) - import all except specified
    /// - (prefix import-set prefix) - import with prefix
    /// - (rename import-set (old new) ...) - import with renaming
    ///
    /// This implementation loads the libraries and imports their exports
    /// into the current environment.
    pub(super) fn eval_import(
        &self,
        args: &Value,
        env: &Rc<Environment>,
    ) -> Result<Value, EvalError> {
        use patina_frontend::LibraryDefinition;

        // Parse all import sets
        let import_sets = self.collect_list_items(args)?;

        for import_set_expr in import_sets {
            // Parse the import set
            let import_set = LibraryDefinition::parse_import_set(&import_set_expr)
                .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;

            // Process the import set: this will import the identifiers into env
            self.process_import_for_eval(&import_set, env)?;
        }

        Ok(Value::Unspecified)
    }

    /// Process an import set for eval context
    ///
    /// This is similar to process_import_set in library_support.rs but works
    /// in the eval context (importing into a regular environment, not building a library)
    fn process_import_for_eval(
        &self,
        import_set: &patina_frontend::ImportSet,
        env: &Rc<Environment>,
    ) -> Result<(), EvalError> {
        use patina_frontend::ImportSet;
        use std::collections::HashSet;

        match import_set {
            ImportSet::Library(lib_name) => {
                // Load the library
                let lib = self.load_library(lib_name).map_err(|e| {
                    EvalError::InvalidSyntax(format!("Failed to load library: {}", e))
                })?;

                // Import all exports into the current environment
                for export_name in lib.export_names() {
                    if let Some(value) = lib.get_export(export_name) {
                        env.define(export_name.to_string(), value.clone());
                    }
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // First process the nested import set into a temporary environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Then import only the specified identifiers
                let allowed: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if allowed.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all except specified identifiers
                let excluded: HashSet<_> = identifiers.iter().collect();
                for (name, value) in temp_env.bindings() {
                    if !excluded.contains(&name) {
                        env.define(name, value);
                    }
                }
                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Import all with prefix
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    env.define(prefixed_name, value);
                }
                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Process nested import set into temp environment
                let temp_env = Rc::new(Environment::new());
                self.process_import_for_eval(import_set, &temp_env)?;

                // Build rename map
                let rename_map: std::collections::HashMap<_, _> = renames.iter().cloned().collect();

                // Import with renaming
                for (name, value) in temp_env.bindings() {
                    let final_name = rename_map.get(&name).unwrap_or(&name);
                    env.define(final_name.clone(), value);
                }
                Ok(())
            }
        }
    }
}
