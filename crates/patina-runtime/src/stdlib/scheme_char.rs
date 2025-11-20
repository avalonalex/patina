//! (scheme char) - Character operations
//!
//! R7RS character operations library

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_char(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "char".to_string()];

    // Character operations with their arities
    let primitives = [
        // Character comparisons
        ("char=?", Arity::Min(2)),
        ("char<?", Arity::Min(2)),
        ("char>?", Arity::Min(2)),
        ("char<=?", Arity::Min(2)),
        ("char>=?", Arity::Min(2)),
        // Case-insensitive comparisons
        ("char-ci=?", Arity::Min(2)),
        ("char-ci<?", Arity::Min(2)),
        ("char-ci>?", Arity::Min(2)),
        ("char-ci<=?", Arity::Min(2)),
        ("char-ci>=?", Arity::Min(2)),
        // Type predicates
        ("char-alphabetic?", Arity::Exact(1)),
        ("char-numeric?", Arity::Exact(1)),
        ("char-whitespace?", Arity::Exact(1)),
        ("char-upper-case?", Arity::Exact(1)),
        ("char-lower-case?", Arity::Exact(1)),
        // Case conversion
        ("char-upcase", Arity::Exact(1)),
        ("char-downcase", Arity::Exact(1)),
        ("char-foldcase", Arity::Exact(1)),
        // Conversion
        ("char->integer", Arity::Exact(1)),
        ("integer->char", Arity::Exact(1)),
        // Digit value
        ("digit-value", Arity::Exact(1)),
    ];

    // Define each primitive in the environment
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

    // Return the list of exported names
    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
