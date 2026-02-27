//! (patina internal control) - Control features (R7RS §6.10)
//!
//! Control flow primitives including procedure application, iteration,
//! multiple values, and continuations.

use crate::Arity;
use crate::environment::Environment;
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
        // Note: map and for-each are now implemented in Scheme (base/higher_order.scm)
        // for CPS compatibility with call/cc
        // Multiple values
        ("values", Arity::Min(0)),
        ("call-with-values", Arity::Exact(2)),
        // Continuations - R7RS standard
        ("call-with-current-continuation", Arity::Exact(1)),
        ("call/cc", Arity::Exact(1)),
        ("dynamic-wind", Arity::Exact(3)),
        // Continuation predicates and delimited control operators
        ("continuation?", Arity::Exact(1)),
        ("make-continuation-prompt-tag", Arity::Range(0, 1)),
        ("continuation-prompt-tag?", Arity::Exact(1)),
        ("default-continuation-prompt-tag", Arity::Exact(0)),
        ("call-with-continuation-prompt", Arity::Min(1)),
        ("abort-current-continuation", Arity::Min(1)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
