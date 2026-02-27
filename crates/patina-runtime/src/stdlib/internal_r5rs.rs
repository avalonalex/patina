//! (patina internal r5rs) - R5RS compatibility primitives
//!
//! Internal primitives for (scheme r5rs).

use crate::Arity;
use crate::environment::Environment;
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
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
