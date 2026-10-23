//! (patina debug) library - Debugging utilities
//!
//! Provides debugging primitives for Patina development and troubleshooting.
//! This library is automatically loaded in REPL mode for convenience.
//!
//! Every export is registered for both backends (#742). `debug-enable`,
//! `debug-disable`, `debug-clear`, `debug-status` and `debug-mode` left it:
//! no backend ever implemented them (owner decision, 2026-10-22), and a
//! debugger that does brings them back.

use crate::Arity;
use crate::Environment;
use std::rc::Rc;

/// Build the (patina debug) library
pub fn build_patina_debug(
    _name: Vec<String>,
    env: Rc<Environment>,
    heap: &patina_core::SharedHeap,
) -> Vec<String> {
    let library_name = vec!["patina".to_string(), "debug".to_string()];

    env.define_primitive(
        heap,
        "macro-debug-mode",
        Arity::Exact(1),
        library_name.clone(),
    );
    env.define_primitive(heap, "library?", Arity::Exact(1), library_name.clone());
    env.define_primitive(heap, "gc", Arity::Exact(0), library_name.clone());
    env.define_primitive(heap, "gc-stats", Arity::Exact(0), library_name);

    // Return list of exported identifiers
    vec![
        "macro-debug-mode".to_string(),
        "library?".to_string(),
        "gc".to_string(),
        "gc-stats".to_string(),
    ]
}
