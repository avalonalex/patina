//! (patina internal lazy) - Lazy evaluation primitives
//!
//! Internal primitives for (scheme lazy).

use crate::Arity;
use crate::environment::Environment;
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
        ("%make-forced-promise", Arity::Exact(1)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
