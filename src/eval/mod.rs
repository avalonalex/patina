// Module declarations
mod application;
mod error;
mod primitives;
mod special_forms;

// Re-export error type for public API
pub use error::EvalError;

use crate::env::Environment;
use crate::value::Value;
use std::rc::Rc;

pub struct Evaluator {
    global_env: Rc<Environment>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        Self::install_primitives(&global_env);
        let evaluator = Evaluator { global_env };

        // Load bootstrap library
        evaluator.load_bootstrap();

        evaluator
    }

    fn load_bootstrap(&self) {
        // Embed bootstrap.scm at compile time
        const BOOTSTRAP: &str = include_str!("../../lib/bootstrap.scm");

        // Parse and evaluate all expressions in bootstrap
        // Silently ignore any errors (shouldn't happen in bootstrap)
        let mut parser = match crate::parser::Parser::new(BOOTSTRAP) {
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
                Err(crate::parser::ParseError::UnexpectedEof) => break,
                Err(_) => break, // Stop on other errors
            }
        }
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
                "let-values" => return self.eval_let_values(&cdr, env),
                "let*-values" => return self.eval_let_star_values(&cdr, env),
                "and" => return self.eval_and(&cdr, env),
                "or" => return self.eval_or(&cdr, env),
                "case" => return self.eval_case(&cdr, env),
                "apply" => return self.eval_apply(&cdr, env),
                _ => {}
            }
        }

        // Regular procedure call
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;
        self.apply(proc, args)
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
