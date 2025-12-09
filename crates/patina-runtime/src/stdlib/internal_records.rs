//! (patina internal records) - Record type primitives (R7RS §5.5)
//!
//! Internal primitives used by the define-record-type macro.
//! These are not directly exported to users.

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal records) library
pub fn build_internal_records(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "records".to_string(),
    ];

    let primitives = [
        // Record type creation
        ("%make-record-type", Arity::Exact(2)),
        // Record type predicates
        ("%record-type?", Arity::Exact(1)),
        ("%record?", Arity::Exact(1)),
        // Record type accessors
        ("%record-type-of", Arity::Exact(1)),
        ("%record-type-name", Arity::Exact(1)),
        ("%record-type-fields", Arity::Exact(1)),
        ("%record-type-field-index", Arity::Exact(2)),
        // Record instance operations
        ("%make-record", Arity::Exact(2)),
        ("%record-ref", Arity::Exact(2)),
        ("%record-set!", Arity::Exact(3)),
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
