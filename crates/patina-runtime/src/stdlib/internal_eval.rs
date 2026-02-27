//! (patina internal eval) - Eval primitives
//!
//! Internal primitives for (scheme eval).

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal eval) library
pub fn build_internal_eval(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "eval".to_string(),
    ];

    let primitives = [("eval", Arity::Exact(2)), ("environment", Arity::Min(0))];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
