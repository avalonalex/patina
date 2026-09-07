//! Each backend names itself to `cond-expand`.
//!
//! `(features)` is otherwise a property of the *process*, and both backends
//! live in one test binary — so `patina-vm` and `patina-tree-walker` cannot be
//! global. They are carried on the `Desugarer`, which is the narrowest thing
//! constructed per backend.
//!
//! Why this file exists rather than a comment: the identifier is set at each
//! of the backends' several `Desugarer` construction sites, and a missed one
//! does not error — `cond-expand` simply falls through to `else`, which is a
//! plausible-looking answer. These tests are what make a missed site fail.
//!
//! The point of the mechanism is #193: a migrated `.scm` test file expresses a
//! per-backend expectation in portable R7RS —
//! `(cond-expand (patina-vm (test-expect-fail 1)) (else))` — instead of the
//! harness carrying which backend it is. On another implementation the same
//! file takes `else`, which is what keeps it portable.

mod common;
use common::{eval_program_tree_walker, eval_program_vm};

/// The identifier each backend advertises, and that it is *exclusive*: seeing
/// only `patina` would still pick a branch, so the negative half is what shows
/// the identifier is really doing the selecting.
#[test]
fn each_backend_names_itself() {
    const PROBE: &str = "(cond-expand (patina-vm 'vm) (patina-tree-walker 'tw) (else 'neither))";
    assert_eq!(eval_program_vm(PROBE), "vm");
    assert_eq!(eval_program_tree_walker(PROBE), "tw");
}

/// The implementation identifier is still there; the backend one is additional.
#[test]
fn the_implementation_identifier_survives() {
    const PROBE: &str = "(cond-expand ((and patina patina-vm) 'vm-and-patina)
                                      ((and patina patina-tree-walker) 'tw-and-patina)
                                      (patina 'patina-only)
                                      (else 'neither))";
    assert_eq!(eval_program_vm(PROBE), "vm-and-patina");
    assert_eq!(eval_program_tree_walker(PROBE), "tw-and-patina");
}

/// Nested `cond-expand` sees it too.
///
/// A child `Desugarer` is built for every nested binding form, and the first
/// version of this defaulted their feature set instead of inheriting it — so
/// a top-level `cond-expand` resolved correctly while one inside a `let`
/// silently took `else`. That is the shape a missed construction site has.
#[test]
fn a_nested_cond_expand_sees_the_backend() {
    const PROBE: &str = "(let ((x 1))
                           (lambda (y)
                             (cond-expand (patina-vm 'vm) (patina-tree-walker 'tw) (else 'neither))))";
    assert_eq!(eval_program_vm(&format!("({PROBE} 0)")), "vm");
    assert_eq!(eval_program_tree_walker(&format!("({PROBE} 0)")), "tw");
}

/// `not` and `or` over the identifier, since an expectation table will use
/// them to say "every backend but this one".
#[test]
fn the_identifier_composes_with_not_and_or() {
    const PROBE: &str = "(cond-expand ((not patina-vm) 'not-vm) (else 'is-vm))";
    assert_eq!(eval_program_vm(PROBE), "is-vm");
    assert_eq!(eval_program_tree_walker(PROBE), "not-vm");

    const EITHER: &str = "(cond-expand ((or patina-vm patina-tree-walker) 'a-patina-backend)
                                       (else 'neither))";
    assert_eq!(eval_program_vm(EITHER), "a-patina-backend");
    assert_eq!(eval_program_tree_walker(EITHER), "a-patina-backend");
}
