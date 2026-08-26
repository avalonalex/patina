//! (patina internal ephemeron) - SRFI 124 ephemerons
//!
//! Internal primitives for `(srfi 124)` / `(scheme ephemeron)`. The weak-key
//! behaviour is the collector's; see `heap::gc`'s ephemeron fixpoint.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal ephemeron) library
pub fn build_internal_ephemeron(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "ephemeron".to_string(),
    ];

    let primitives = [
        ("make-ephemeron", Arity::Exact(2)),
        ("ephemeron?", Arity::Exact(1)),
        ("ephemeron-broken?", Arity::Exact(1)),
        ("ephemeron-key", Arity::Exact(1)),
        ("ephemeron-datum", Arity::Exact(1)),
        ("reference-barrier", Arity::Exact(1)),
    ];

    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
