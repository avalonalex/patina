//! (patina internal numbers) - Number operations (R7RS §6.2)
//!
//! All numeric primitives grouped together:
//! - Basic arithmetic from (scheme base)
//! - Inexact operations from (scheme inexact)
//! - Complex operations from (scheme complex)
//!
//! Note: The library field is just for documentation. The PrimitiveRegistry
//! looks up primitives by name, so they work regardless of which library
//! exported them.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal numbers) library
pub fn build_internal_numbers(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    // This is just a documentation marker - the registry finds by name
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "numbers".to_string(),
    ];

    let primitives = [
        // === Basic arithmetic (scheme base) ===
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        ("*", Arity::Min(0)),
        ("/", Arity::Min(1)),
        // Comparison
        ("=", Arity::Min(2)),
        ("<", Arity::Min(2)),
        (">", Arity::Min(2)),
        ("<=", Arity::Min(2)),
        (">=", Arity::Min(2)),
        // Division operations
        ("quotient", Arity::Exact(2)),
        ("remainder", Arity::Exact(2)),
        ("modulo", Arity::Exact(2)),
        ("floor/", Arity::Exact(2)),
        ("floor-quotient", Arity::Exact(2)),
        ("floor-remainder", Arity::Exact(2)),
        ("truncate/", Arity::Exact(2)),
        ("truncate-quotient", Arity::Exact(2)),
        ("truncate-remainder", Arity::Exact(2)),
        // Unary operations
        ("abs", Arity::Exact(1)),
        ("floor", Arity::Exact(1)),
        ("ceiling", Arity::Exact(1)),
        ("truncate", Arity::Exact(1)),
        ("round", Arity::Exact(1)),
        ("square", Arity::Exact(1)),
        // Binary operations
        ("expt", Arity::Exact(2)),
        ("max", Arity::Min(1)),
        ("min", Arity::Min(1)),
        // GCD/LCM
        ("gcd", Arity::Min(0)),
        ("lcm", Arity::Min(0)),
        // Rational operations
        ("numerator", Arity::Exact(1)),
        ("denominator", Arity::Exact(1)),
        // Exactness conversion
        ("exact", Arity::Exact(1)),
        ("inexact", Arity::Exact(1)),
        ("exact-integer-sqrt", Arity::Exact(1)),
        ("rationalize", Arity::Exact(2)),
        // String conversion
        ("number->string", Arity::Range(1, 2)),
        ("string->number", Arity::Range(1, 2)),
        // === Type predicates (number-related) ===
        ("number?", Arity::Exact(1)),
        ("complex?", Arity::Exact(1)),
        ("real?", Arity::Exact(1)),
        ("rational?", Arity::Exact(1)),
        ("integer?", Arity::Exact(1)),
        ("exact?", Arity::Exact(1)),
        ("inexact?", Arity::Exact(1)),
        ("exact-integer?", Arity::Exact(1)),
        // === Inexact operations (scheme inexact) ===
        ("finite?", Arity::Exact(1)),
        ("infinite?", Arity::Exact(1)),
        ("nan?", Arity::Exact(1)),
        ("sin", Arity::Exact(1)),
        ("cos", Arity::Exact(1)),
        ("tan", Arity::Exact(1)),
        ("asin", Arity::Exact(1)),
        ("acos", Arity::Exact(1)),
        ("atan", Arity::Range(1, 2)),
        ("exp", Arity::Exact(1)),
        ("log", Arity::Range(1, 2)),
        ("sqrt", Arity::Exact(1)),
        // === Complex operations (scheme complex) ===
        ("make-rectangular", Arity::Exact(2)),
        ("make-polar", Arity::Exact(2)),
        ("real-part", Arity::Exact(1)),
        ("imag-part", Arity::Exact(1)),
        ("magnitude", Arity::Exact(1)),
        ("angle", Arity::Exact(1)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Rc::new(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            })),
        );
        exports.push(name.to_string());
    }

    exports
}
