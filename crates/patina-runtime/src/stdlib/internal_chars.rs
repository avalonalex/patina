//! (patina internal chars) - Character operations (R7RS §6.6)
//!
//! Character primitives including comparisons, predicates, and case conversion.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal chars) library
pub fn build_internal_chars(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "chars".to_string(),
    ];

    let primitives = [
        // Type predicate
        ("char?", Arity::Exact(1)),
        // Conversion
        ("char->integer", Arity::Exact(1)),
        ("integer->char", Arity::Exact(1)),
        // Comparison (scheme base)
        ("char=?", Arity::Min(2)),
        ("char<?", Arity::Min(2)),
        ("char>?", Arity::Min(2)),
        ("char<=?", Arity::Min(2)),
        ("char>=?", Arity::Min(2)),
        // Case-insensitive comparison (scheme char)
        ("char-ci=?", Arity::Min(2)),
        ("char-ci<?", Arity::Min(2)),
        ("char-ci>?", Arity::Min(2)),
        ("char-ci<=?", Arity::Min(2)),
        ("char-ci>=?", Arity::Min(2)),
        // Character predicates (scheme char)
        ("char-alphabetic?", Arity::Exact(1)),
        ("char-numeric?", Arity::Exact(1)),
        ("char-whitespace?", Arity::Exact(1)),
        ("char-upper-case?", Arity::Exact(1)),
        ("char-lower-case?", Arity::Exact(1)),
        // Case conversion (scheme char)
        ("char-upcase", Arity::Exact(1)),
        ("char-downcase", Arity::Exact(1)),
        ("char-foldcase", Arity::Exact(1)),
        // Digit value (scheme char)
        ("digit-value", Arity::Exact(1)),
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
