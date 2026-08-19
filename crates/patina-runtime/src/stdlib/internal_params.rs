//! (patina internal params) - Dynamic bindings (R7RS §4.2.6)
//!
//! Parameter objects for dynamic binding.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal params) library
pub fn build_internal_params(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "params".to_string(),
    ];

    let primitives = [
        // Parameter creation
        ("make-parameter", Arity::Range(1, 2)),
        // Used by the `parameterize` macro, which has to convert every new
        // value before entering its `dynamic-wind` and then install and
        // restore raw ones (R7RS §4.2.6). Not exported to users.
        ("%parameter-convert", Arity::Exact(2)),
        ("%parameterize-swap!", Arity::Exact(2)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
