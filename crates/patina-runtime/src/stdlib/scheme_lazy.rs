//! (scheme lazy) - R7RS Lazy Evaluation Library
//!
//! This library provides support for lazy evaluation through promises.
//! A promise is a delayed computation that is evaluated only when forced.
//!
//! Exports:
//! - `delay` - Create a lazy promise (macro)
//! - `delay-force` - Create a lazy promise with proper tail recursion (macro)
//! - `force` - Force evaluation of a promise
//! - `promise?` - Check if a value is a promise
//! - `make-promise` - Convert a value to a promise

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (scheme lazy) library
///
/// Populates the environment with lazy evaluation primitives
/// and returns the list of exported identifiers.
pub fn build_scheme_lazy(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "lazy".to_string()];

    // Define primitives
    let primitives = [
        ("force", Arity::Exact(1)),
        ("promise?", Arity::Exact(1)),
        ("make-promise", Arity::Exact(1)),
        // Internal helper for delay/delay-force macros (not exported)
        ("%make-delayed-promise", Arity::Exact(1)),
    ];

    // Install all primitives into the environment
    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Box::new(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            })),
        );
    }

    // Return list of exported identifiers
    // Note: delay and delay-force are macros defined in lazy-extras.scm
    // Note: %make-delayed-promise is internal, not exported
    let mut exports: Vec<String> = primitives
        .iter()
        .filter_map(|(name, _)| {
            if name.starts_with('%') {
                None // Don't export internal helpers
            } else {
                Some(name.to_string())
            }
        })
        .collect();

    // Add macros that will be defined in extras file
    exports.push("delay".to_string());
    exports.push("delay-force".to_string());

    exports
}
