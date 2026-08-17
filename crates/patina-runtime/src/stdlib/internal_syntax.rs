//! (patina internal syntax) — the syntactic keywords, as bindings.
//!
//! This is the bootstrap end of the binding-based keyword design: it is the one
//! place a `CoreForm` marker enters an environment from nothing. Everywhere
//! else it arrives the ordinary way, by import.
//!
//! `lib/scheme/base.sld` imports this library and re-exports the subset R7RS
//! puts in `(scheme base)`. Anything that needs core syntax must import a
//! library that exports it — which is exactly what R7RS requires and what
//! chibi and Gauche enforce.
//!
//! See `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`.

use crate::environment::Environment;
use patina_core::core_syntax::ALL_CORE_FORMS;
use std::rc::Rc;

/// Build the (patina internal syntax) library.
pub fn build_internal_syntax(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    ALL_CORE_FORMS
        .iter()
        .map(|&form| {
            // The marker must be out of the heap borrow before `define`, which
            // reaches into the environment's own `RefCell`s.
            let marker = env.heap().borrow_mut().core_syntax(form);
            let name = form.name().to_string();
            env.define(name.clone(), marker);
            name
        })
        .collect()
}
