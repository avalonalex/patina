//! Stub implementations for R7RS libraries not yet implemented
//!
//! These provide empty library definitions so that code can import them
//! without errors, even though the procedures aren't implemented yet.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// (scheme eval) - Evaluation
///
/// Exports:
/// - eval: Evaluate an expression in a given environment
/// - environment: Create an environment from import sets
///
/// Note: null-environment and scheme-report-environment are in (scheme r5rs)
pub fn build_scheme_eval(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "eval".to_string()];

    // Define primitives with their arities
    let primitives = [("eval", Arity::Exact(2)), ("environment", Arity::Min(0))];

    // Install each primitive into the library environment
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

    // Return exported names
    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

/// (scheme r5rs) - R5RS compatibility
///
/// Exports:
/// - null-environment: Returns environment with only R5RS syntactic keywords
/// - scheme-report-environment: Returns environment with all R5RS bindings
pub fn build_scheme_r5rs(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "r5rs".to_string()];

    // Define primitives with their arities
    let primitives = [
        ("null-environment", Arity::Exact(1)),
        ("scheme-report-environment", Arity::Exact(1)),
    ];

    // Install each primitive into the library environment
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

    // Return exported names
    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
