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
        // Car/cdr compositions (two- through four-deep)
        ("caar", Arity::Exact(1)),
        ("cadr", Arity::Exact(1)),
        ("cdar", Arity::Exact(1)),
        ("cddr", Arity::Exact(1)),
        ("caaar", Arity::Exact(1)),
        ("caadr", Arity::Exact(1)),
        ("cadar", Arity::Exact(1)),
        ("caddr", Arity::Exact(1)),
        ("cdaar", Arity::Exact(1)),
        ("cdadr", Arity::Exact(1)),
        ("cddar", Arity::Exact(1)),
        ("cdddr", Arity::Exact(1)),
        ("caaaar", Arity::Exact(1)),
        ("caaadr", Arity::Exact(1)),
        ("caadar", Arity::Exact(1)),
        ("caaddr", Arity::Exact(1)),
        ("cadaar", Arity::Exact(1)),
        ("cadadr", Arity::Exact(1)),
        ("caddar", Arity::Exact(1)),
        ("cadddr", Arity::Exact(1)),
        ("cdaaar", Arity::Exact(1)),
        ("cdaadr", Arity::Exact(1)),
        ("cdadar", Arity::Exact(1)),
        ("cdaddr", Arity::Exact(1)),
        ("cddaar", Arity::Exact(1)),
        ("cddadr", Arity::Exact(1)),
        ("cdddar", Arity::Exact(1)),
        ("cddddr", Arity::Exact(1)),
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
