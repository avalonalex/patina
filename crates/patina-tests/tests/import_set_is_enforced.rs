//! An import set bounds what a program can name — at the top level too.
//!
//! The VM used to bind the entire primitive registry into globals by short
//! name, so a program importing only `(scheme base)` could still call `cadddr`
//! from `(scheme cxr)` or `bitwise-and` from `(srfi 151)`. The hole was
//! specific to registered primitives at the top level: libraries enforced
//! their imports all along (`sld_file_loading.rs` covers that half), and so
//! did the tree-walker — which is why these run on both backends: the point
//! is that they now agree. History in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.

mod common;
use common::{assert_program_eval_error, assert_program_eval_to, eval_program as eval};

/// `case-lambda` is unavailable until `(scheme case-lambda)` is imported.
///
/// Migrated from `case_lambda.rs`, where it was named
/// `test_case_lambda_empty_clause_list` and believed to assert that
/// `(case-lambda)` with no clauses is an error. It is not — with the library
/// imported that returns a procedure matching no call. The row passed because
/// it ran without the import, so the property it actually held is this one.
/// #193's migration is what surfaced the difference.
#[test]
fn case_lambda_needs_its_import() {
    assert_program_eval_error("(case-lambda)");
    assert_program_eval_to(
        "(import (scheme case-lambda)) (procedure? (case-lambda ((x) x)))",
        "#t",
    );
}

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
