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
                .map_err(|e| EvalError::UndefinedVariable(e))?;
            Ok(Value::Unspecified)
        } else {
            Err(EvalError::InvalidSyntax(
                "set! expects a symbol".to_string(),
            ))
        }
    }

    fn eval_lambda(&self, _args: &Value, _env: &Rc<Environment>) -> Result<Value, EvalError> {
        // TODO: Implement lambda
        Err(EvalError::InvalidSyntax(
            "lambda not yet implemented".to_string(),
        ))
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
            "cons" => self.primitive_cons(args),
            "car" => self.primitive_car(args),
            "cdr" => self.primitive_cdr(args),
            "null?" => self.primitive_null_p(args),
            "pair?" => self.primitive_pair_p(args),
            "list" => Ok(self.list_from_vec(args)),
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

            let mut result = first;
            for arg in &args[1..] {
                if let Value::Integer(n) = arg {
                    if *n == 0 {
                        return Err(EvalError::DivisionByZero);
                    }
                    result /= n;
                } else {
                    return Err(EvalError::TypeError(format!(
                        "/ expects numbers, got {}",
                        arg
                    )));
                }
            }
            Ok(Value::Integer(result))
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
                    return Err(EvalError::TypeError(format!("= expects numbers")));
                }
            }
            Ok(Value::Boolean(true))
        } else {
            Err(EvalError::TypeError(format!("= expects numbers")))
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
                return Err(EvalError::TypeError(format!("< expects numbers")));
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

    fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            ("+", Arity::Min(0)),
            ("-", Arity::Min(1)),
            ("*", Arity::Min(0)),
            ("/", Arity::Min(1)),
            ("=", Arity::Min(2)),
            ("<", Arity::Min(2)),
            ("cons", Arity::Exact(2)),
            ("car", Arity::Exact(1)),
            ("cdr", Arity::Exact(1)),
            ("null?", Arity::Exact(1)),
            ("pair?", Arity::Exact(1)),
            ("list", Arity::Min(0)),
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
