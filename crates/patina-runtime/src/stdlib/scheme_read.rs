//! (scheme read) - Read operations
//!
//! Provides the `read` procedure for parsing Scheme expressions from ports.
//!
//! R7RS specifies:
//! - read: Parse and return the next Scheme expression from a port
//!
//! Note: `read` is also exported from (scheme base) for convenience.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_read(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "read".to_string()];

    // Read procedure - also available in (scheme base)
    let primitives = [("read", Arity::Range(0, 1))];

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
