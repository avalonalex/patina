//! (patina internal system) - System operations
//!
//! System-related primitives including features and process context.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal system) library
pub fn build_internal_system(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "system".to_string(),
    ];

    let primitives = [
        // Features (scheme base)
        ("features", Arity::Exact(0)),
        // Process context (scheme process-context)
        ("command-line", Arity::Exact(0)),
        ("exit", Arity::Range(0, 1)),
        ("emergency-exit", Arity::Range(0, 1)),
        ("get-environment-variable", Arity::Exact(1)),
        ("get-environment-variables", Arity::Exact(0)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
