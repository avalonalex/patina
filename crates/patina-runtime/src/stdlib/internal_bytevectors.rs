//! (patina internal bytevectors) - Bytevector operations (R7RS §6.9)
//!
//! Bytevector primitives for binary data handling.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal bytevectors) library
pub fn build_internal_bytevectors(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "bytevectors".to_string(),
    ];

    let primitives = [
        // Type predicate
        ("bytevector?", Arity::Exact(1)),
        // Constructors
        ("make-bytevector", Arity::Range(1, 2)),
        ("bytevector", Arity::Min(0)),
        // Accessors
        ("bytevector-length", Arity::Exact(1)),
        ("bytevector-u8-ref", Arity::Exact(2)),
        // Mutators
        ("bytevector-u8-set!", Arity::Exact(3)),
        ("bytevector-copy!", Arity::Range(3, 5)),
        // Operations
        ("bytevector-copy", Arity::Range(1, 3)),
        ("bytevector-append", Arity::Min(0)),
        // UTF-8 conversion
        ("utf8->string", Arity::Range(1, 3)),
        ("string->utf8", Arity::Range(1, 3)),
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
