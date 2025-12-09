//! (patina internal control) - Control features (R7RS §6.10)
//!
//! Control flow primitives including procedure application, iteration,
//! multiple values, and continuations.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal control) library
pub fn build_internal_control(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "control".to_string(),
    ];

    let primitives = [
        // Procedure predicate
        ("procedure?", Arity::Exact(1)),
        // Application
        ("apply", Arity::Min(2)),
        // Iteration (R7RS §6.10 lists these under control)
        ("map", Arity::Min(2)),
        ("for-each", Arity::Min(2)),
        // Multiple values
        ("values", Arity::Min(0)),
        ("call-with-values", Arity::Exact(2)),
        // Continuations
        ("call-with-current-continuation", Arity::Exact(1)),
        ("call/cc", Arity::Exact(1)),
        ("dynamic-wind", Arity::Exact(3)),
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
