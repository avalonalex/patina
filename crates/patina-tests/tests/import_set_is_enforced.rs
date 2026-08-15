//! An import set bounds what a program can name — at the top level too.
//!
//! The VM used to bind the entire primitive registry into globals by short
//! name, so a program importing only `(scheme base)` could still call `cadddr`
//! from `(scheme cxr)` or `bitwise-and` from `(srfi 151)`. Scheme-level exports
//! were scoped correctly and *libraries* enforced their imports properly, so
//! the hole was specific to registered primitives at the top level.
//!
//! That asymmetry is why it survived: the same expression succeeded at the top
//! level and failed inside a `define-library`, and the top level is where one
//! naturally checks. It had already cost a real bug — `(srfi 132)`'s
//! `vector-merge` called `cadddr` without importing `(scheme cxr)`, which failed
//! only inside the library.
//!
//! The tree-walker was right all along, which is why these run on both backends:
//! the point is that they now agree.

mod common;
use common::{assert_program_eval_error, eval_program as eval};

/// Names a program never imported must not resolve, whether they are backed by
/// Rust primitives or by Scheme.
#[test]
fn test_unimported_names_do_not_resolve() {
    // Asserted as "fails on both backends" rather than by catching it: whether
    // an unbound variable is a catchable Scheme condition or a hard error is
    // itself a backend divergence, pre-existing and unrelated to imports.
    for expr in [
        "(cadddr (list 1 2 3 4))",  // (scheme cxr)
        "(bitwise-and 12 10)",      // (srfi 151)
        "(bit-count 12)",           // (srfi 151)
        "(arithmetic-shift 1 10)",  // (srfi 151)
        "(list-sort < (list 2 1))", // (srfi 132)
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {expr}"));
    }
}

/// The same names must still work once their library *is* imported — the point
/// is scoping, not removal.
#[test]
fn test_importing_the_library_makes_them_resolve() {
    assert_eq!(
        eval("(import (scheme base) (scheme cxr)) (cadddr (list 1 2 3 4))"),
        "4"
    );
    assert_eq!(
        eval("(import (scheme base) (srfi 151)) (list (bitwise-and 12 10) (bit-count 12))"),
        "(8 2)"
    );
    assert_eq!(
        eval("(import (scheme base) (srfi 132)) (list-sort < (list 3 1 2))"),
        "(1 2 3)"
    );
}

/// A fresh top level is still usable without importing anything: the bootstrap
/// defines `(scheme base)`'s exports, which is what it always meant to define.
#[test]
fn test_the_default_baseline_still_works() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(car (list 9 8))"), "9");
    assert_eq!(eval("(map (lambda (x) (* x x)) (list 1 2 3))"), "(1 4 9)");
}

/// The asymmetry that hid the defect: top level and library scope must agree
/// about the same expression under the same imports.
/// The asymmetry that hid the defect: an expression rejected inside a
/// `define-library` must be rejected at the top level under the same imports.
/// `sld_file_loading.rs` covers the library half; this is the top-level half.
#[test]
fn test_top_level_agrees_with_library_scope() {
    assert_program_eval_error("(import (scheme base)) (cadddr (list 1 2 3 4))");
}
