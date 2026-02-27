//! Eval primitives (R7RS Section 6.12 - scheme eval)
//!
//! Implements:
//! - `environment` - Create an environment from import sets
//! - `eval` - Evaluate an expression in a given environment
//! - `null-environment` - R5RS environment with only syntactic keywords
//! - `scheme-report-environment` - R5RS environment with all bindings

use super::super::Evaluator;
use super::super::cps_eval::eval_cps;
use super::super::error::EvalError;
use super::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_core::TaggedValue;
use patina_frontend::Desugarer;
use patina_runtime::Arity;
use patina_runtime::environment::Environment;
use std::rc::Rc;

/// Extract library name from a TaggedValue list like (scheme base)
fn extract_library_name_tagged(
    tv: TaggedValue,
    heap: &patina_core::Heap,
) -> Result<Vec<String>, EvalError> {
    let mut result = Vec::new();
    let mut current = tv;

    while !current.is_null() {
        if !current.is_pair() {
            return Err(EvalError::TypeError(format!(
                "environment: expected a proper list for library name, got {}",
                heap.type_name(current)
            )));
        }
        let car = heap.car(current);
        match heap.get_symbol_or_identifier_name(car) {
            Some(name) => result.push(name.to_string()),
            None => {
                return Err(EvalError::TypeError(format!(
                    "environment: library name component must be a symbol, got {}",
                    heap.type_name(car)
                )));
            }
        }
        current = heap.cdr(current);
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
fn primitive_environment(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();

    // Extract library names from tagged args
    let lib_names: Vec<Vec<String>> = {
        let heap_ref = heap.borrow();
        args.iter()
            .map(|tv| extract_library_name_tagged(*tv, &heap_ref))
            .collect::<Result<Vec<_>, _>>()?
    };

    // Create a fresh empty environment sharing the global heap for TaggedValue compatibility
    let env = Rc::new(Environment::with_heap(evaluator.global_env.heap().clone()));

    // Process each import set argument
    for lib_name in lib_names {
        // Load the library
        let library = evaluator.load_library(&lib_name).map_err(|e| {
            EvalError::InternalError(format!("environment: cannot load library: {}", e))
        })?;

        // Install exports into the new environment
        for (name, tv) in library.exports_iter_tagged() {
            env.define(name.clone(), tv);
        }
    }

    // Return as immutable environment specifier
    Ok(heap.borrow_mut().alloc_environment_specifier(env, false))
}

/// Check if a tagged value represents a definition form
fn is_definition_tagged(tv: TaggedValue, heap: &patina_core::Heap) -> bool {
    if !tv.is_pair() {
        return false;
    }
    let car = heap.car(tv);
    match heap.get_symbol_or_identifier_name(car) {
        Some(name) => name == "define" || name == "define-values" || name == "define-syntax",
        None => false,
    }
}

/// (eval expr-or-def environment-specifier) → values
///
/// Evaluates an expression in the specified environment.
/// If expr-or-def is a definition, the environment must be mutable.
///
/// Uses CPS evaluation to ensure proper continuation support for lambdas
/// created during evaluation.
fn primitive_eval(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "eval expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    // Extract environment from specifier (using heap method, no Value conversion)
    let (env, mutable) = {
        let heap_ref = heap.borrow();
        match heap_ref.get_environment_specifier(args[1]) {
            Some((env, mutable)) => (env.clone(), mutable),
            None => {
                return Err(EvalError::TypeError(format!(
                    "eval: expected environment, got {}",
                    heap_ref.type_name(args[1])
                )));
            }
        }
    };

    // Check if this is a definition
    if is_definition_tagged(args[0], &heap.borrow()) && !mutable {
        return Err(EvalError::InvalidSyntax(
            "eval: cannot define in immutable environment".to_string(),
        ));
    }

    // Desugar the expression with macro-aware desugarer (tagged path)
    let desugarer = Desugarer::with_env(env.clone());
    let core_expr = desugarer.desugar_tagged(args[0], heap).map_err(|e| {
        EvalError::InternalError(format!("eval: failed to desugar expression: {}", e))
    })?;

    // Evaluate via CPS for full continuation support
    eval_cps(&core_expr, env, evaluator)
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
fn primitive_null_environment(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "null-environment expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let version = if args[0].is_fixnum() {
        args[0].as_fixnum_unchecked()
    } else {
        let heap = evaluator.global_env.heap();
        return Err(EvalError::TypeError(format!(
            "null-environment: expected integer version, got {}",
            heap.borrow().type_name(args[0])
        )));
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "null-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // Create environment with only syntactic keywords, sharing global heap for TaggedValue compatibility
    // These are macros defined in (scheme base), so we need to get them from there
    let env = Rc::new(Environment::with_heap(evaluator.global_env.heap().clone()));

    // Load scheme base to get the macros
    let base_lib = evaluator
        .load_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    // Install only the syntactic keywords
    for name in R5RS_SYNTAX {
        if let Some(tv) = base_lib.get_export_tagged(name) {
            env.define(name.to_string(), tv);
        }
        // Some syntactic keywords are special forms handled by the evaluator
        // and don't need to be in the environment (if, lambda, quote, etc.)
    }

    let heap = evaluator.global_env.heap();
    Ok(heap.borrow_mut().alloc_environment_specifier(env, false))
}

/// (scheme-report-environment version) → environment-specifier
///
/// Returns an environment with all R5RS bindings (syntax + procedures).
/// Currently only version 5 is supported.
fn primitive_scheme_report_environment(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "scheme-report-environment expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let version = if args[0].is_fixnum() {
        args[0].as_fixnum_unchecked()
    } else {
        let heap = evaluator.global_env.heap();
        return Err(EvalError::TypeError(format!(
            "scheme-report-environment: expected integer version, got {}",
            heap.borrow().type_name(args[0])
        )));
    };

    if version != 5 {
        return Err(EvalError::InvalidSyntax(format!(
            "scheme-report-environment: unsupported version {}, only 5 is supported",
            version
        )));
    }

    // Create environment with all R5RS bindings, sharing global heap for TaggedValue compatibility
    let env = Rc::new(Environment::with_heap(evaluator.global_env.heap().clone()));

    // Load scheme base
    let base_lib = evaluator
        .load_library(&["scheme".to_string(), "base".to_string()])
        .map_err(|e| EvalError::InternalError(format!("Cannot load scheme base: {}", e)))?;

    // Install syntactic keywords
    for name in R5RS_SYNTAX {
        if let Some(tv) = base_lib.get_export_tagged(name) {
            env.define(name.to_string(), tv);
        }
    }

    // Install procedures
    for name in R5RS_PROCEDURES {
        if let Some(tv) = base_lib.get_export_tagged(name) {
            env.define(name.to_string(), tv);
        }
    }

    // Try to load additional libraries for R5RS features
    // (scheme inexact) for sin, cos, etc.
    if let Ok(inexact_lib) = evaluator.load_library(&["scheme".to_string(), "inexact".to_string()])
    {
        for name in &[
            "sin", "cos", "tan", "asin", "acos", "atan", "exp", "log", "sqrt",
        ] {
            if let Some(tv) = inexact_lib.get_export_tagged(name) {
                env.define(name.to_string(), tv);
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
            if let Some(tv) = complex_lib.get_export_tagged(name) {
                env.define(name.to_string(), tv);
            }
        }
    }

    // Add the R5RS-specific aliases
    // exact->inexact is an alias for inexact
    if let Some(tv) = base_lib.get_export_tagged("inexact") {
        env.define("exact->inexact".to_string(), tv);
    }
    // inexact->exact is an alias for exact
    if let Some(tv) = base_lib.get_export_tagged("exact") {
        env.define("inexact->exact".to_string(), tv);
    }

    // Add null-environment and scheme-report-environment themselves
    let r5rs_lib = vec!["scheme".to_string(), "r5rs".to_string()];
    env.define_primitive("null-environment", Arity::Exact(1), r5rs_lib.clone());
    env.define_primitive("scheme-report-environment", Arity::Exact(1), r5rs_lib);

    let heap = evaluator.global_env.heap();
    Ok(heap.borrow_mut().alloc_environment_specifier(env, false))
}

/// Register eval primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;

    // environment - create environment from import sets
    registry.register(PrimitiveFn::new_tagged(
        "scheme.eval",
        "environment",
        Arity::Min(0),
        "Creates an immutable environment from the given import sets.",
        |eval, args, _tail| primitive_environment(eval, args).map(EvalResult::Tagged),
    ));

    // eval - evaluate expression in environment
    registry.register(PrimitiveFn::new_tagged(
        "scheme.eval",
        "eval",
        Arity::Exact(2),
        "Evaluates an expression in the specified environment.",
        |eval, args, _tail| primitive_eval(eval, args).map(EvalResult::Tagged),
    ));

    // null-environment - R5RS environment with only syntactic keywords
    registry.register(PrimitiveFn::new_tagged(
        "scheme.r5rs",
        "null-environment",
        Arity::Exact(1),
        "Returns an environment with only the R5RS syntactic keywords.",
        |eval, args, _tail| primitive_null_environment(eval, args).map(EvalResult::Tagged),
    ));

    // scheme-report-environment - R5RS environment with all bindings
    registry.register(PrimitiveFn::new_tagged(
        "scheme.r5rs",
        "scheme-report-environment",
        Arity::Exact(1),
        "Returns an environment with all R5RS bindings.",
        |eval, args, _tail| primitive_scheme_report_environment(eval, args).map(EvalResult::Tagged),
    ));
}
