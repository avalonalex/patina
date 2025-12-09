//! (patina internal vectors) - Vector operations (R7RS §6.8)
//!
//! Vector primitives including construction, access, mutation, and iteration.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal vectors) library
pub fn build_internal_vectors(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "vectors".to_string(),
    ];

    let primitives = [
        // Type predicate
        ("vector?", Arity::Exact(1)),
        // Constructors
        ("make-vector", Arity::Range(1, 2)),
        ("vector", Arity::Min(0)),
        // Accessors
        ("vector-length", Arity::Exact(1)),
        ("vector-ref", Arity::Exact(2)),
        // Mutators
        ("vector-set!", Arity::Exact(3)),
        ("vector-fill!", Arity::Range(2, 4)),
        ("vector-copy!", Arity::Range(3, 5)),
        // Conversion
        ("vector->list", Arity::Range(1, 3)),
        ("list->vector", Arity::Exact(1)),
        ("vector->string", Arity::Range(1, 3)),
        ("string->vector", Arity::Range(1, 3)),
        // Operations
        ("vector-copy", Arity::Range(1, 3)),
        ("vector-append", Arity::Min(0)),
        // Higher-order
        ("vector-map", Arity::Min(2)),
        ("vector-for-each", Arity::Min(2)),
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
