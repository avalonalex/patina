//! (patina internal lists) - Pair and list operations (R7RS §6.4)
//!
//! Core list primitives. Note: map and for-each are in internal_control
//! per R7RS §6.10 (Control features).

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal lists) library
pub fn build_internal_lists(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "lists".to_string(),
    ];

    let primitives = [
        // Constructors
        ("cons", Arity::Exact(2)),
        ("list", Arity::Min(0)),
        ("make-list", Arity::Range(1, 2)),
        // Accessors
        ("car", Arity::Exact(1)),
        ("cdr", Arity::Exact(1)),
        ("list-ref", Arity::Exact(2)),
        ("list-tail", Arity::Exact(2)),
        // Mutators
        ("set-car!", Arity::Exact(2)),
        ("set-cdr!", Arity::Exact(2)),
        ("list-set!", Arity::Exact(3)),
        // List operations
        ("length", Arity::Exact(1)),
        ("append", Arity::Min(0)),
        ("reverse", Arity::Exact(1)),
        ("list-copy", Arity::Exact(1)),
        // Search
        ("memq", Arity::Exact(2)),
        ("memv", Arity::Exact(2)),
        ("member", Arity::Range(2, 3)),
        ("assq", Arity::Exact(2)),
        ("assv", Arity::Exact(2)),
        ("assoc", Arity::Range(2, 3)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
