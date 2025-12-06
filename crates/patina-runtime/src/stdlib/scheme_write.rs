//! (scheme write) - Write operations
//!
//! Provides procedures for writing Scheme data with various levels of
//! circular/shared structure handling.
//!
//! R7RS specifies:
//! - display: Human-readable output (strings without quotes, etc.)
//! - write: Machine-readable output with datum labels for circular structures
//! - write-shared: Like write, but also labels shared (multiply-referenced) structures
//! - write-simple: No datum labels (may loop on circular structures)
//!
//! Note: display and write are also exported from (scheme base) for convenience.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_write(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "write".to_string()];

    // Write procedures
    let primitives = [
        // display and write are in (scheme base), but also exported here
        ("display", Arity::Range(1, 2)),
        ("write", Arity::Range(1, 2)),
        // write-shared and write-simple are specific to (scheme write)
        ("write-shared", Arity::Range(1, 2)),
        ("write-simple", Arity::Range(1, 2)),
    ];

    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Rc::new(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            })),
        );
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
