//! (scheme inexact) - Inexact (floating point) operations
//!
//! Re-exports inexact operations from (scheme base)

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

pub fn build_scheme_inexact(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    // Inexact/floating point operations
    let primitives = [
        ("finite?", Arity::Exact(1)),
        ("infinite?", Arity::Exact(1)),
        ("nan?", Arity::Exact(1)),
        ("sin", Arity::Exact(1)),
        ("cos", Arity::Exact(1)),
        ("tan", Arity::Exact(1)),
        ("asin", Arity::Exact(1)),
        ("acos", Arity::Exact(1)),
        ("atan", Arity::Range(1, 2)),
        ("exp", Arity::Exact(1)),
        ("log", Arity::Range(1, 2)),
        ("sqrt", Arity::Exact(1)),
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
