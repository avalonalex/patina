//! (patina internal predicates) - Booleans, symbols, and equality (R7RS §6.1, §6.3)
//!
//! Type predicates and equality operations.

use crate::Arity;
use crate::environment::Environment;
use std::rc::Rc;

/// Build the (patina internal predicates) library
pub fn build_internal_predicates(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec![
        "patina".to_string(),
        "internal".to_string(),
        "predicates".to_string(),
    ];

    let primitives = [
        // === Booleans (§6.1) ===
        ("not", Arity::Exact(1)),
        ("boolean?", Arity::Exact(1)),
        ("boolean=?", Arity::Min(2)),
        // === Pairs/Lists (§6.4 predicates only) ===
        ("pair?", Arity::Exact(1)),
        ("null?", Arity::Exact(1)),
        ("list?", Arity::Exact(1)),
        // === Symbols (§6.3) ===
        ("symbol?", Arity::Exact(1)),
        ("symbol=?", Arity::Min(2)),
        ("symbol->string", Arity::Exact(1)),
        ("string->symbol", Arity::Exact(1)),
        // === Equivalence predicates (§6.1) ===
        ("eq?", Arity::Exact(2)),
        ("eqv?", Arity::Exact(2)),
        ("equal?", Arity::Exact(2)),
        // === Hashing ===
        ("equal-hash", Arity::Exact(1)),
        ("identity-hash", Arity::Exact(1)),
    ];

    let mut exports = Vec::new();
    for (name, arity) in &primitives {
        env.define_primitive(name, arity.clone(), library_name.clone());
        exports.push(name.to_string());
    }

    exports
}
