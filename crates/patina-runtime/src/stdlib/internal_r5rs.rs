//! (patina internal r5rs) - R5RS compatibility primitives
//!
//! Internal primitives for (scheme r5rs).

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal r5rs) library
pub fn build_internal_r5rs(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "r5rs".to_string(),
    ];

    let primitives = [
        ("null-environment", Arity::Exact(1)),
        ("scheme-report-environment", Arity::Exact(1)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Rc::new(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            })),
        );
        exports.push(name.to_string());
    }

    exports
}
