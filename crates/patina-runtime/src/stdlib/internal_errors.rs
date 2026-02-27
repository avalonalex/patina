//! (patina internal errors) - Exception handling (R7RS §6.11)
//!
//! Exception handling primitives. Currently stubs - full implementation pending.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal errors) library
///
/// NOTE: These are currently stubs. Full exception handling requires:
/// - Error object types (file-error, read-error, etc.)
/// - Exception handler stack
/// - Integration with continuations
pub fn build_internal_errors(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "errors".to_string(),
    ];

    let primitives = [
        // Basic error (implemented)
        ("error", Arity::Min(1)),
        // Error object accessors (stubs)
        ("error-object?", Arity::Exact(1)),
        ("error-object-message", Arity::Exact(1)),
        ("error-object-irritants", Arity::Exact(1)),
        // Error type predicates (stubs)
        ("file-error?", Arity::Exact(1)),
        ("read-error?", Arity::Exact(1)),
        // Exception handling (stubs)
        ("raise", Arity::Exact(1)),
        ("raise-continuable", Arity::Exact(1)),
        ("with-exception-handler", Arity::Exact(2)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
