//! (scheme file) - File I/O operations
//!
//! R7RS file library providing file I/O operations.
//!
//! Includes:
//! - Textual file ports: open-input-file, open-output-file
//! - Binary file ports: open-binary-input-file, open-binary-output-file
//! - Higher-order: call-with-input-file, call-with-output-file,
//!                 with-input-from-file, with-output-to-file
//! - Utilities: file-exists?, delete-file

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_file(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "file".to_string()];

    // File I/O operations with their arities
    let primitives = [
        // Textual file port operations
        ("open-input-file", Arity::Exact(1)),
        ("open-output-file", Arity::Exact(1)),
        // Binary file port operations
        ("open-binary-input-file", Arity::Exact(1)),
        ("open-binary-output-file", Arity::Exact(1)),
        // Higher-order file operations
        ("call-with-input-file", Arity::Exact(2)),
        ("call-with-output-file", Arity::Exact(2)),
        ("with-input-from-file", Arity::Exact(2)),
        ("with-output-to-file", Arity::Exact(2)),
        // File predicates and utilities
        ("file-exists?", Arity::Exact(1)),
        ("delete-file", Arity::Exact(1)),
    ];

    // Define each primitive in the environment
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

    // Return the list of exported names
    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
