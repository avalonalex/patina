//! (patina debug) library - Debugging utilities
//!
//! Provides debugging primitives for Patina development and troubleshooting.
//! This library is automatically loaded in REPL mode for convenience.

use crate::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina debug) library
pub fn build_patina_debug(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["patina".to_string(), "debug".to_string()];

    // Evaluation debugging primitives
    env.define(
        "debug-enable".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "debug-enable",
            arity: Arity::Min(1),
            library: library_name.clone(),
        })),
    );

    env.define(
        "debug-disable".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "debug-disable",
            arity: Arity::Min(1),
            library: library_name.clone(),
        })),
    );

    env.define(
        "debug-clear".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "debug-clear",
            arity: Arity::Exact(0),
            library: library_name.clone(),
        })),
    );

    env.define(
        "debug-status".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "debug-status",
            arity: Arity::Exact(0),
            library: library_name.clone(),
        })),
    );

    env.define(
        "debug-mode".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "debug-mode",
            arity: Arity::Exact(1),
            library: library_name.clone(),
        })),
    );

    // Macro expansion debugging
    env.define(
        "macro-debug-mode".to_string(),
        Value::Procedure(Rc::new(Procedure::Primitive {
            name: "macro-debug-mode",
            arity: Arity::Exact(1),
            library: library_name,
        })),
    );

    // Return list of exported identifiers
    vec![
        "debug-enable".to_string(),
        "debug-disable".to_string(),
        "debug-clear".to_string(),
        "debug-status".to_string(),
        "debug-mode".to_string(),
        "macro-debug-mode".to_string(),
    ]
}
