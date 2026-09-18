//! The pure re-export shims: `(srfi 16)`, `(srfi 23)`, `(srfi 98)` and
//! `(scheme small)`.
//!
//! Each name is functionality R7RS already provides, imported by the vendored
//! corpus under its pre-R7RS or R7RS-large name — srfi-78 reaches `(srfi 23)`,
//! jkode-sassy imports `(srfi 98)`, chrisoei-cint imports `(scheme small)`,
//! and `(srfi 146)`'s Gleckler HAMT libraries import `(srfi 16)`.
//! There is nothing to test behaviourally beyond what the backing libraries'
//! own suites cover; what these tests pin is that each shim *loads* and that
//! a binding reached through it is usable, which is exactly the probe the
//! compat harness runs.
//!
//! One test at the end is not about a re-export: `(srfi 2)`'s rejection of a
//! malformed claw. It sits here because it has to be a Rust test rather than a
//! row in `tests/scheme/srfi/and-let.scm` — the error is a compile-time one —
//! and because `(srfi 2)` is bundled for the same reason `(srfi 16)` is. Its
//! own doc comment says what it guards.

mod common;
use common::eval_program as eval;

#[test]
fn test_srfi_23_error_is_reachable_and_catchable() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 23))
             (guard (e (#t (error-object-message e))) (error \"boom\" 1 2))"
        ),
        "\"boom\""
    );
}

#[test]
fn test_srfi_98_environment_access() {
    // The variable's presence is environment-dependent; the shape is not.
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 98))
             (let ((v (get-environment-variable \"PATH\")))
               (or (string? v) (eq? v #f)))"
        ),
        "#t"
    );
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 98))
             (list? (get-environment-variables))"
        ),
        "#t"
    );
}

/// One expression per constituent library, so a name that stopped resolving
/// points at the sub-library that lost it.
#[test]
fn test_scheme_small_bindings_span_the_constituent_libraries() {
    let cases = [
        ("(vector-map + #(1 2) #(10 20))", "#(11 22)"), // base
        ("((case-lambda ((a) a) ((a b) b)) 1 2)", "2"), // case-lambda
        ("(char-upcase #\\a)", "#\\A"),                 // char
        ("(real-part 42)", "42"),                       // complex
        ("(caddr '(1 2 3))", "3"),                      // cxr
        ("(eval '(+ 1 2) (environment '(scheme base)))", "3"), // eval
        ("(file-exists? \"/nonexistent-patina-shim-test\")", "#f"), // file
        ("(exact (floor (sqrt 16)))", "4"),             // inexact
        ("(force (delay 7))", "7"),                     // lazy
        ("(string? (car (command-line)))", "#t"),       // process-context
        ("(read (open-input-string \"(a b)\"))", "(a b)"), // read
        ("(procedure? interaction-environment)", "#t"), // repl
        ("(positive? (jiffies-per-second))", "#t"),     // time
        (
            "(let ((p (open-output-string))) (write 'x p) (get-output-string p))",
            "\"x\"",
        ), // write
    ];
    for (expr, expected) in cases {
        assert_eq!(
            eval(&format!("(import (scheme small)) {expr}")),
            expected,
            "under (scheme small): {expr}"
        );
    }
}

/// `(srfi 16)` re-exports `(scheme case-lambda)`, which is itself SRFI 16's
/// reference implementation — so this pins that the shim loads and that the
/// macro reached through it still dispatches on arity, rather than that
/// `case-lambda` works (the chibi R7RS suite covers that).
#[test]
fn test_srfi_16_case_lambda_is_reachable_and_dispatches() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 16))
             (define f (case-lambda ((a) (list 'one a)) ((a b) (list 'two a b))))
             (list (f 1) (f 1 2))"
        ),
        "((one 1) (two 1 2))"
    );
}

/// A malformed `and-let*` claw must be rejected by name, on both backends.
///
/// This cannot be a row in `tests/scheme/srfi/and-let.scm` beside the rest of
/// SRFI 2: `syntax-error` fires during desugaring, so `guard` never sees it
/// and the whole file would fail to load. Hence a Rust test, here rather than
/// in a file of its own, because `(srfi 2)` is bundled for the same reason
/// `(srfi 16)` above it is.
///
/// What it guards is not the message but the *rule ordering*. `and-let*`'s
/// three claw shapes are distinguished by shape alone, and a claw of three or
/// more elements matches none of them — yet two of the rules would accept one
/// and mean something else, differing only in whether a body is present:
/// with a body it reads as a test claw plus bare claws, without one as a
/// single bare claw that is an application. Both then report "unbound
/// variable: a", naming neither the claw nor `and-let*`. So both shapes are
/// checked; a rule reordered above the rejection would fail exactly one.
#[test]
fn test_srfi_2_rejects_a_malformed_claw_by_name() {
    for program in [
        "(import (scheme base) (srfi 2)) (and-let* ((a 1 2)) 'body)",
        // The no-body shape, which a rejection placed below the trailing-claw
        // rules would let through.
        "(import (scheme base) (srfi 2)) (and-let* ((a 1 2)))",
    ] {
        for (backend, outcome) in [
            ("tree-walker", common::try_eval_program_tree_walker(program)),
            ("vm", common::try_eval_program_vm(program)),
        ] {
            let err = outcome.expect_err(&format!(
                "[{backend}] a three-element and-let* claw must not compile: {program}"
            ));
            assert!(
                err.contains("and-let* claw must be"),
                "[{backend}] the claw must be named as the fault, not reported as an \
                 unbound variable — got: {err}"
            );
        }
    }
}
