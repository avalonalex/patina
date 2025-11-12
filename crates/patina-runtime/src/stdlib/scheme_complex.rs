//! (scheme complex) - Complex number operations
//!
//! Re-exports complex number operations from (scheme base)

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_complex(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    // Complex number operations
    let primitives = [
        ("make-rectangular", Arity::Exact(2)),
        ("make-polar", Arity::Exact(2)),
        ("real-part", Arity::Exact(1)),
        ("imag-part", Arity::Exact(1)),
        ("magnitude", Arity::Exact(1)),
        ("angle", Arity::Exact(1)),
    ];

    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Procedure::Primitive {
                name,
                arity: arity.clone(),
            }),
        );
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
