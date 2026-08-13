//! (patina internal strings) - String operations (R7RS §6.7)
//!
//! String primitives including construction, access, comparison, and case conversion.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal strings) library
pub fn build_internal_strings(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "strings".to_string(),
    ];

    let primitives = [
        // Type predicate
        ("string?", Arity::Exact(1)),
        // Constructors
        ("make-string", Arity::Range(1, 2)),
        ("string", Arity::Min(0)),
        // Accessors
        ("string-length", Arity::Exact(1)),
        ("string-ref", Arity::Exact(2)),
        // Mutators
        ("string-set!", Arity::Exact(3)),
        ("string-fill!", Arity::Range(2, 4)),
        ("string-copy!", Arity::Range(3, 5)),
        // Comparison
        ("string=?", Arity::Min(2)),
        ("string<?", Arity::Min(2)),
        ("string>?", Arity::Min(2)),
        ("string<=?", Arity::Min(2)),
        ("string>=?", Arity::Min(2)),
        // Case-insensitive comparison
        ("string-ci=?", Arity::Min(2)),
        ("string-ci<?", Arity::Min(2)),
        ("string-ci>?", Arity::Min(2)),
        ("string-ci<=?", Arity::Min(2)),
        ("string-ci>=?", Arity::Min(2)),
        // String operations
        ("substring", Arity::Exact(3)),
        ("string-append", Arity::Min(0)),
        ("string-copy", Arity::Range(1, 3)),
        // Conversion
        ("string->list", Arity::Range(1, 3)),
        ("list->string", Arity::Exact(1)),
        // string-map / string-for-each deliberately absent: the live
        // implementations are Scheme (lib/scheme/base/higher_order.scm) —
        // a Rust higher-order frame drops captured continuations, so the
        // Rust versions were deleted rather than left as a second, broken
        // implementation behind (patina internal strings).
        // Case conversion
        ("string-upcase", Arity::Exact(1)),
        ("string-downcase", Arity::Exact(1)),
        ("string-foldcase", Arity::Exact(1)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
