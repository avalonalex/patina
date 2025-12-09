//! Eval primitives (R7RS Section 6.12 - scheme eval)
//!
//! Implements:
//! - `environment` - Create an environment from import sets
//! - `eval` - Evaluate an expression in a given environment
//! - `null-environment` - R5RS environment with only syntactic keywords
//! - `scheme-report-environment` - R5RS environment with all bindings

use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_runtime::environment::Environment;
use patina_runtime::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Extract library name from a value like (scheme base) or a quoted list
///
/// Handles both:
/// - Direct list: (scheme base) when args are already evaluated
/// - The list structure from a quoted expression
fn extract_library_name(value: &Value) -> Result<Vec<String>, EvalError> {
    let mut result = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => break,
            Value::Pair(pair) => {
                let (car, cdr) = pair.borrow().clone();
                match car {
                    Value::Symbol(s) => result.push(s.to_string()),
                    Value::Identifier(id) => result.push(id.name.to_string()),
                    _ => {
                        return Err(EvalError::TypeError(format!(
                            "environment: library name component must be a symbol, got {}",
                            car.type_name()
                        )));
                    }
                }
                current = cdr;
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "environment: expected a proper list for library name, got {}",
                    current.type_name()
                )));
            }
        }
    }

    if result.is_empty() {
        return Err(EvalError::TypeError(
            "environment: library name cannot be empty".to_string(),
        ));
    }

    Ok(result)
}

/// (environment list1 ...) → environment-specifier
///
/// Creates an immutable environment from the given import sets.
/// Each argument should be a quoted list like '(scheme base).
fn primitive_environment(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Create a fresh empty environment
    let env = Rc::new(Environment::new());

    // Process each import set argument
    for arg in args {
        let lib_name = extract_library_name(&arg)?;

        // Load the library
        let library = evaluator.load_library(&lib_name).map_err(|e| {
            EvalError::InternalError(format!("environment: cannot load library: {}", e))
        })?;

        // Install exports into the new environment
        for (name, value) in &library.exports {
            env.define(name.clone(), value.clone());
        }
    }

    // Return as immutable environment specifier
    Ok(Value::EnvironmentSpecifier {
        env,
        mutable: false,
    })
}

/// Check if a value represents a definition form
fn is_definition(expr: &Value) -> bool {
    match expr {
        Value::Pair(pair) => {
            let (car, _) = &*pair.borrow();
            match car {
                Value::Symbol(s) => {
                    let name = s.as_ref();
                    name == "define" || name == "define-values" || name == "define-syntax"
                }
                Value::Identifier(id) => {
                    let name = id.name.as_ref();
                    name == "define" || name == "define-values" || name == "define-syntax"
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// (eval expr-or-def environment-specifier) → values
///
/// Evaluates an expression in the specified environment.
/// If expr-or-def is a definition, the environment must be mutable.
fn primitive_eval(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "eval")?;

    let expr = &args[0];
    let env_spec = &args[1];

    // Extract environment from specifier
    let (env, mutable) = match env_spec {
        Value::EnvironmentSpecifier { env, mutable } => (env.clone(), *mutable),
        _ => {
            return Err(EvalError::TypeError(format!(
                "eval: expected environment, got {}",
                env_spec.type_name()
            )));
        }
    };

    // Check if this is a definition
    if is_definition(expr) && !mutable {
        return Err(EvalError::InvalidSyntax(
            "eval: cannot define in immutable environment".to_string(),
        ));
    }

    // Evaluate expression in the environment
    evaluator.eval_in_env(expr, &env)
}

/// R5RS syntactic keywords (special forms)
/// These are the only bindings in (null-environment 5)
const R5RS_SYNTAX: &[&str] = &[
    "and",
    "begin",
    "case",
    "cond",
    "define",
    "define-syntax",
    "delay",
    "do",
    "if",
    "lambda",
    "let",
    "let*",
    "let-syntax",
    "letrec",
    "letrec-syntax",
    "or",
    "quasiquote",
    "quote",
    "set!",
    "syntax-rules",
];

/// R5RS procedures (subset of scheme base that existed in R5RS)
/// Combined with R5RS_SYNTAX, these form (scheme-report-environment 5)
const R5RS_PROCEDURES: &[&str] = &[
    // Equivalence predicates
    "eq?",
    "eqv?",
    "equal?",
    // Numbers
    "+",
    "-",
    "*",
    "/",
    "=",
    "<",
    ">",
    "<=",
    ">=",
    "abs",
    "quotient",
    "remainder",
    "modulo",
    "gcd",
    "lcm",
    "numerator",
    "denominator",
    "floor",
    "ceiling",
    "truncate",
    "round",
    "rationalize",
    "exp",
    "log",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sqrt",
    "expt",
    "make-rectangular",
    "make-polar",
    "real-part",
    "imag-part",
    "magnitude",
    "angle",
    "exact->inexact",
    "inexact->exact",
    "number->string",
    "string->number",
    // Number predicates
    "number?",
    "complex?",
    "real?",
    "rational?",
    "integer?",
    "exact?",
    "inexact?",
    "zero?",
    "positive?",
    "negative?",
    "odd?",
    "even?",
    // Booleans
    "not",
    "boolean?",
    // Pairs and lists
    "pair?",
    "cons",
    "car",
    "cdr",
    "set-car!",
    "set-cdr!",
    "caar",
    "cadr",
    "cdar",
    "cddr",
    "caaar",
    "caadr",
    "cadar",
    "caddr",
    "cdaar",
    "cdadr",
    "cddar",
    "cdddr",
    "caaaar",
    "caaadr",
    "caadar",
    "caaddr",
    "cadaar",
    "cadadr",
    "caddar",
    "cadddr",
    "cdaaar",
    "cdaadr",
    "cdadar",
    "cdaddr",
    "cddaar",
    "cddadr",
    "cdddar",
    "cddddr",
    "null?",
    "list?",
    "list",
    "length",
    "append",
    "reverse",
    "list-tail",
    "list-ref",
    "memq",
    "memv",
    "member",
    "assq",
    "assv",
    "assoc",
    // Symbols
    "symbol?",
    "symbol->string",
    "string->symbol",
    // Characters
    "char?",
    "char=?",
    "char<?",
    "char>?",
    "char<=?",
    "char>=?",
    "char-ci=?",
    "char-ci<?",
    "char-ci>?",
    "char-ci<=?",
    "char-ci>=?",
    "char-alphabetic?",
    "char-numeric?",
    "char-whitespace?",
    "char-upper-case?",
    "char-lower-case?",
    "char->integer",
    "integer->char",
    "char-upcase",
    "char-downcase",
    // Strings
    "string?",
    "make-string",
    "string",
    "string-length",
    "string-ref",
    "string-set!",
    "string=?",
    "string-ci=?",
    "string<?",
    "string>?",
    "string<=?",
    "string>=?",
    "string-ci<?",
    "string-ci>?",
    "string-ci<=?",
    "string-ci>=?",
    "substring",
    "string-append",
    "string->list",
    "list->string",
    "string-copy",
    "string-fill!",
    // Vectors
    "vector?",
    "make-vector",
    "vector",
    "vector-length",
    "vector-ref",
    "vector-set!",
    "vector->list",
    "list->vector",
    "vector-fill!",
    // Control features
    "procedure?",
    "apply",
    "map",
    "for-each",
    "force",
    "call-with-current-continuation",
    "values",
    "call-with-values",
    "dynamic-wind",
    // Eval
    "eval",
    "scheme-report-environment",
    "null-environment",
    // I/O
    "call-with-input-file",
    "call-with-output-file",
    "input-port?",
    "output-port?",
    "current-input-port",
    "current-output-port",
    "with-input-from-file",
    "with-output-to-file",
    "open-input-file",
    "open-output-file",
    "close-input-port",
    "close-output-port",
    "read",
    "read-char",
    "peek-char",
    "eof-object?",
    "char-ready?",
    "write",
    "display",
    "newline",
    "write-char",
    "load",
    // REPL
    "interaction-environment",
];

/// (null-environment version) → environment-specifier
///
/// Returns an environment with only the R5RS syntactic keywords.
/// Currently only version 5 is supported.
fn primitive_null_environment(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "null-environment")?;

    let version = match &args[0] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError(format!(
                "null-environment: expected integer version, got {}",
                args[0].type_name()
            )));
        }
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "null-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // Create environment with only syntactic keywords
    // These are macros defined in (scheme base), so we need to get them from there
    let env = Rc::new(Environment::new());

    // Load scheme base to get the macros
    let base_lib = evaluator
        .load_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    // Install only the syntactic keywords
    for name in R5RS_SYNTAX {
        if let Some(value) = base_lib.exports.get(*name) {
            env.define(name.to_string(), value.clone());
        }
        // Some syntactic keywords are special forms handled by the evaluator
        // and don't need to be in the environment (if, lambda, quote, etc.)
    }

    Ok(Value::EnvironmentSpecifier {
        env,
        mutable: false,
    })
}

/// (scheme-report-environment version) → environment-specifier
///
/// Returns an environment with all R5RS bindings (syntax + procedures).
/// Currently only version 5 is supported.
fn primitive_scheme_report_environment(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "scheme-report-environment")?;

    let version = match &args[0] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError(format!(
                "scheme-report-environment: expected integer version, got {}",
                args[0].type_name()
            )));
        }
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "scheme-report-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // Create environment with all R5RS bindings
    let env = Rc::new(Environment::new());

    // Load scheme base
    let base_lib = evaluator
        .load_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    // Install syntactic keywords
    for name in R5RS_SYNTAX {
        if let Some(value) = base_lib.exports.get(*name) {
            env.define(name.to_string(), value.clone());
        }
    }

    // Install procedures
    for name in R5RS_PROCEDURES {
        if let Some(value) = base_lib.exports.get(*name) {
            env.define(name.to_string(), value.clone());
        }
    }

    // Try to load additional libraries for R5RS features
    // (scheme inexact) for sin, cos, etc.
    if let Ok(inexact_lib) = evaluator.load_library(&["scheme".to_string(), "inexact".to_string()])
    {
        for name in &[
            "sin", "cos", "tan", "asin", "acos", "atan", "exp", "log", "sqrt",
        ] {
            if let Some(value) = inexact_lib.exports.get(*name) {
                env.define(name.to_string(), value.clone());
            }
        }
    }

    // (scheme complex) for make-rectangular, etc.
    if let Ok(complex_lib) = evaluator.load_library(&["scheme".to_string(), "complex".to_string()])
    {
        for name in &[
            "make-rectangular",
            "make-polar",
            "real-part",
            "imag-part",
            "magnitude",
            "angle",
        ] {
            if let Some(value) = complex_lib.exports.get(*name) {
                env.define(name.to_string(), value.clone());
            }
        }
    }

    // Add the R5RS-specific aliases
    // exact->inexact is an alias for inexact
    if let Some(inexact) = base_lib.exports.get("inexact") {
        env.define("exact->inexact".to_string(), inexact.clone());
    }
    // inexact->exact is an alias for exact
    if let Some(exact) = base_lib.exports.get("exact") {
        env.define("inexact->exact".to_string(), exact.clone());
    }

    // Add null-environment and scheme-report-environment themselves
    let r5rs_lib = vec!["scheme".to_string(), "r5rs".to_string()];
    env.define(
        "null-environment".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "null-environment",
            arity: Arity::Exact(1),
            library: r5rs_lib.clone(),
        })),
    );
    env.define(
        "scheme-report-environment".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "scheme-report-environment",
            arity: Arity::Exact(1),
            library: r5rs_lib,
        })),
    );

    Ok(Value::EnvironmentSpecifier {
        env,
        mutable: false,
    })
}

/// Register eval primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;

    // environment - create environment from import sets
    registry.register(PrimitiveFn::new(
        "scheme.eval",
        "environment",
        Arity::Min(0),
        "Creates an immutable environment from the given import sets.",
        |eval, args, _tail| primitive_environment(eval, args).map(EvalResult::Value),
    ));

    // eval - evaluate expression in environment
    registry.register(PrimitiveFn::new(
        "scheme.eval",
        "eval",
        Arity::Exact(2),
        "Evaluates an expression in the specified environment.",
        |eval, args, _tail| primitive_eval(eval, args).map(EvalResult::Value),
    ));

    // null-environment - R5RS environment with only syntactic keywords
    registry.register(PrimitiveFn::new(
        "scheme.r5rs",
        "null-environment",
        Arity::Exact(1),
        "Returns an environment with only the R5RS syntactic keywords.",
        |eval, args, _tail| primitive_null_environment(eval, args).map(EvalResult::Value),
    ));

    // scheme-report-environment - R5RS environment with all bindings
    registry.register(PrimitiveFn::new(
        "scheme.r5rs",
        "scheme-report-environment",
        Arity::Exact(1),
        "Returns an environment with all R5RS bindings.",
        |eval, args, _tail| primitive_scheme_report_environment(eval, args).map(EvalResult::Value),
    ));
}
