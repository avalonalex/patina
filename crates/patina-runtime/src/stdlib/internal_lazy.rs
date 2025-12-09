//! (patina internal lazy) - Lazy evaluation primitives
//!
//! Internal primitives for (scheme lazy).

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (patina internal lazy) library
pub fn build_internal_lazy(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "lazy".to_string(),
    ];

    let primitives = [
        ("force", Arity::Exact(1)),
        ("promise?", Arity::Exact(1)),
        ("make-promise", Arity::Exact(1)),
        // Internal helper for delay/delay-force macros
        ("%make-delayed-promise", Arity::Exact(1)),
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
