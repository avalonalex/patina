//! (scheme base) - R7RS Base Library
//!
//! This is the core R7RS library containing all fundamental procedures and syntax.
//! It includes:
//! - Arithmetic operations
//! - List operations
//! - String operations
//! - Vector operations
//! - Type predicates
//! - Equality predicates
//! - I/O operations
//!
//! Note: Some forms like define, lambda, if, etc. are special forms handled by
//! the evaluator, not primitives. This library only exports primitive procedures.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (scheme base) library
///
/// Populates the environment with all R7RS base library primitives
/// and returns the list of exported identifiers.
pub fn build_scheme_base(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "base".to_string()];

    // Define all primitives with their arities
    let primitives = [
        // Arithmetic operations
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        ("*", Arity::Min(0)),
        ("/", Arity::Min(1)),
        ("=", Arity::Min(2)),
        ("<", Arity::Min(2)),
        (">", Arity::Min(2)),
        ("<=", Arity::Min(2)),
        (">=", Arity::Min(2)),
        ("quotient", Arity::Exact(2)),
        ("remainder", Arity::Exact(2)),
        ("modulo", Arity::Exact(2)),
        ("abs", Arity::Exact(1)),
        ("max", Arity::Min(1)),
        ("min", Arity::Min(1)),
        ("floor", Arity::Exact(1)),
        ("ceiling", Arity::Exact(1)),
        ("truncate", Arity::Exact(1)),
        ("round", Arity::Exact(1)),
        // TODO: Research if sqrt should be here - R7RS spec lists it only in (scheme inexact)
        // but it may need to be in both for exact square roots (e.g., sqrt(4) = 2)
        ("sqrt", Arity::Exact(1)),
        ("square", Arity::Exact(1)),
        ("expt", Arity::Exact(2)),
        ("gcd", Arity::Min(0)),
        ("lcm", Arity::Min(0)),
        ("numerator", Arity::Exact(1)),
        ("denominator", Arity::Exact(1)),
        ("exact", Arity::Exact(1)),
        ("inexact", Arity::Exact(1)),
        ("exact-integer-sqrt", Arity::Exact(1)),
        ("rationalize", Arity::Exact(2)),
        // TODO: Move to (scheme complex) library when implemented
        ("real-part", Arity::Exact(1)),
        ("imag-part", Arity::Exact(1)),
        // List operations
        ("cons", Arity::Exact(2)),
        ("car", Arity::Exact(1)),
        ("cdr", Arity::Exact(1)),
        ("list", Arity::Min(0)),
        ("length", Arity::Exact(1)),
        ("append", Arity::Min(0)),
        ("make-list", Arity::Range(1, 2)),
        ("list-copy", Arity::Exact(1)),
        ("reverse", Arity::Exact(1)),
        ("list-ref", Arity::Exact(2)),
        ("list-tail", Arity::Exact(2)),
        ("memq", Arity::Exact(2)),
        ("memv", Arity::Exact(2)),
        ("member", Arity::Range(2, 3)),
        ("assq", Arity::Exact(2)),
        ("assv", Arity::Exact(2)),
        ("assoc", Arity::Range(2, 3)),
        // Higher-order functions
        ("map", Arity::Min(2)),
        ("for-each", Arity::Min(2)),
        // Type predicates
        ("number?", Arity::Exact(1)),
        ("complex?", Arity::Exact(1)),
        ("real?", Arity::Exact(1)),
        ("rational?", Arity::Exact(1)),
        ("integer?", Arity::Exact(1)),
        ("boolean?", Arity::Exact(1)),
        ("string?", Arity::Exact(1)),
        ("symbol?", Arity::Exact(1)),
        ("null?", Arity::Exact(1)),
        ("pair?", Arity::Exact(1)),
        ("list?", Arity::Exact(1)),
        ("exact?", Arity::Exact(1)),
        ("inexact?", Arity::Exact(1)),
        ("boolean=?", Arity::Min(2)),
        ("procedure?", Arity::Exact(1)),
        ("char?", Arity::Exact(1)),
        ("vector?", Arity::Exact(1)),
        ("exact-integer?", Arity::Exact(1)),
        // TODO: library? is a Patina extension, not part of R7RS.
        // Consider moving to a (patina core) or similar library in the future.
        ("library?", Arity::Exact(1)),
        // Equality operations
        ("eq?", Arity::Exact(2)),
        ("eqv?", Arity::Exact(2)),
        ("equal?", Arity::Exact(2)),
        // Multiple values
        ("values", Arity::Min(0)),
        ("call-with-values", Arity::Exact(2)),
        // String operations
        ("string-length", Arity::Exact(1)),
        ("string-ref", Arity::Exact(2)),
        ("string-set!", Arity::Exact(3)),
        ("make-string", Arity::Range(1, 2)),
        ("string", Arity::Min(0)),
        ("string=?", Arity::Min(2)),
        ("string<?", Arity::Min(2)),
        ("string>?", Arity::Min(2)),
        ("string<=?", Arity::Min(2)),
        ("string>=?", Arity::Min(2)),
        ("string-ci=?", Arity::Min(2)),
        ("string-ci<?", Arity::Min(2)),
        ("string-ci>?", Arity::Min(2)),
        ("string-ci<=?", Arity::Min(2)),
        ("string-ci>=?", Arity::Min(2)),
        ("string-append", Arity::Min(0)),
        ("substring", Arity::Exact(3)),
        ("string->list", Arity::Range(1, 3)),
        ("list->string", Arity::Exact(1)),
        ("string-copy", Arity::Range(1, 3)),
        ("string-fill!", Arity::Range(2, 4)),
        ("string-copy!", Arity::Range(3, 5)),
        // Vector operations
        ("make-vector", Arity::Range(1, 2)),
        ("vector", Arity::Min(0)),
        ("vector-length", Arity::Exact(1)),
        ("vector-ref", Arity::Exact(2)),
        ("vector-set!", Arity::Exact(3)),
        ("vector->list", Arity::Range(1, 3)),
        ("list->vector", Arity::Exact(1)),
        ("vector->string", Arity::Range(1, 3)),
        ("string->vector", Arity::Range(1, 3)),
        ("vector-copy", Arity::Range(1, 3)),
        ("vector-copy!", Arity::Range(3, 5)),
        ("vector-append", Arity::Min(0)),
        ("vector-fill!", Arity::Range(2, 4)),
        ("vector-map", Arity::Min(2)),
        ("vector-for-each", Arity::Min(2)),
        // Conversion operations
        ("number->string", Arity::Range(1, 2)),
        ("string->number", Arity::Range(1, 2)),
        // I/O operations
        ("display", Arity::Exact(1)),
        ("write", Arity::Exact(1)),
        ("newline", Arity::Exact(0)),
        // Parameter operations
        ("make-parameter", Arity::Range(1, 2)),
    ];

    // Install all primitives into the environment
    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            }),
        );
    }

    // Return list of exported identifiers
    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
