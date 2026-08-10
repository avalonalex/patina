//! (patina internal bitwise) - Bitwise operations on exact integers
//!
//! The SRFI 151 core. Only the operations that cannot be written efficiently in
//! Scheme are primitives; the ~30 derived procedures are in
//! `lib/srfi/151/bitwise.scm`, which is the same split the reference
//! implementation makes.
//!
//! Semantics are two's complement over integers of unbounded width, so negative
//! operands behave as if sign-extended infinitely to the left.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal bitwise) library
pub fn build_internal_bitwise(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "bitwise".to_string(),
    ];

    let primitives = [
        // n-ary, with SRFI 151's identity for the empty case
        ("bitwise-and", Arity::Min(0)),
        ("bitwise-ior", Arity::Min(0)),
        ("bitwise-xor", Arity::Min(0)),
        ("bitwise-not", Arity::Exact(1)),
        ("arithmetic-shift", Arity::Exact(2)),
        ("bit-count", Arity::Exact(1)),
        ("integer-length", Arity::Exact(1)),
        ("bit-set?", Arity::Exact(2)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }
    exports
}
