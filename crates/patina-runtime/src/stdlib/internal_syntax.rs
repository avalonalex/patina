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
use patina_core::core_syntax::{ALL_CORE_FORMS, CoreForm};
use std::rc::Rc;

/// Build the (patina internal syntax) library.
pub fn build_internal_syntax(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    ALL_CORE_FORMS
        .iter()
        .map(|&form| {
            let name = form.name().to_string();
            define_core_syntax(&env, form);
            name
        })
        .collect()
}

/// Keywords a fresh top level has that `(scheme base)` does not export.
///
/// R7RS does not put `import` in `(scheme base)` — it is a top-level and
/// library declaration keyword — and `expand` is a Patina extension. Patina's
/// top level accepts both, so both must be *bound* there. Until they were,
/// `(import …)` resolved only through the desugarer's spelling fallback, which
/// stage 2 removes: the first form of every script would have stopped working.
const TOP_LEVEL_ONLY_SYNTAX: &[CoreForm] = &[CoreForm::Import, CoreForm::Expand];

/// Seed the syntactic keywords a top-level environment has beyond
/// `(scheme base)`'s exports.
///
/// Called by each backend's bootstrap after it copies `(scheme base)` in.
/// Shared rather than written twice because the two bootstraps already differ
/// enough to drift, and a backend that missed this would fail only under
/// stage 2.
pub fn seed_top_level_syntax(env: &Rc<Environment>) {
    for &form in TOP_LEVEL_ONLY_SYNTAX {
        define_core_syntax(env, form);
    }
}

/// Bind `form`'s marker under its own name in `env`.
///
/// The marker must leave the heap borrow before `define`, which reaches into
/// the environment's own `RefCell`s.
fn define_core_syntax(env: &Rc<Environment>, form: CoreForm) {
    let marker = env.heap().borrow_mut().core_syntax(form);
    env.define(form.name().to_string(), marker);
}
