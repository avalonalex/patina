//! (scheme process-context) - R7RS Process Context Library
//!
//! Provides access to the process environment:
//! - command-line: Returns command line arguments as a list
//! - exit: Exit program (would run dynamic-wind handlers if implemented)
//! - emergency-exit: Exit immediately without handlers
//! - get-environment-variable: Get a single environment variable
//! - get-environment-variables: Get all environment variables as alist

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (scheme process-context) library
pub fn build_scheme_process_context(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "process-context".to_string()];

    let primitives = [
        ("command-line", Arity::Exact(0)),
        ("exit", Arity::Range(0, 1)),
        ("emergency-exit", Arity::Range(0, 1)),
        ("get-environment-variable", Arity::Exact(1)),
        ("get-environment-variables", Arity::Exact(0)),
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
