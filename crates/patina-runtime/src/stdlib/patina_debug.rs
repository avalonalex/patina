//! (patina debug) library - Debugging utilities
//!
//! Provides debugging primitives for Patina development and troubleshooting.
//! This library is automatically loaded in REPL mode for convenience.

use crate::Arity;
use crate::Environment;
use std::rc::Rc;

/// Build the (patina debug) library
pub fn build_patina_debug(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["patina".to_string(), "debug".to_string()];

    env.define_primitive("debug-enable", Arity::Min(1), library_name.clone());
    env.define_primitive("debug-disable", Arity::Min(1), library_name.clone());
    env.define_primitive("debug-clear", Arity::Exact(0), library_name.clone());
    env.define_primitive("debug-status", Arity::Exact(0), library_name.clone());
    env.define_primitive("debug-mode", Arity::Exact(1), library_name.clone());
    env.define_primitive("macro-debug-mode", Arity::Exact(1), library_name.clone());
    env.define_primitive("library?", Arity::Exact(1), library_name);

    // Return list of exported identifiers
    vec![
        "debug-enable".to_string(),
        "debug-disable".to_string(),
        "debug-clear".to_string(),
        "debug-status".to_string(),
        "debug-mode".to_string(),
        "macro-debug-mode".to_string(),
        "library?".to_string(),
    ]
}
