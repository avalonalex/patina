//! (patina internal time) - Time operations
//!
//! Time-related primitives for (scheme time).

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal time) library
pub fn build_internal_time(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "time".to_string(),
    ];

    let primitives = [
        ("current-second", Arity::Exact(0)),
        ("current-jiffy", Arity::Exact(0)),
        ("jiffies-per-second", Arity::Exact(0)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
