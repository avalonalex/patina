//! The pure re-export shims: `(srfi 6)`, `(srfi 9)`, `(srfi 11)`,
//! `(srfi 16)`, `(srfi 23)`, `(srfi 39)`, `(srfi 98)` and `(scheme small)`.
//!
//! Each name is functionality R7RS already provides, imported by the vendored
//! corpus under its pre-R7RS or R7RS-large name — srfi-78 reaches `(srfi 23)`,
//! jkode-sassy imports `(srfi 98)`, chrisoei-cint imports `(scheme small)`,
//! and `(srfi 146)`'s Gleckler HAMT libraries import `(srfi 16)`.
//!
//! `(srfi 6)`, `(srfi 9)`, `(srfi 11)` and `(srfi 39)` (#390) have no such
//! requester in the corpus. They are bundled so a program written against the
//! pre-R7RS name loads rather than failing over functionality that is present
//! under another name, which is the same argument as the four above.
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

/// `(srfi 6)`'s three string-port procedures became R7RS 6.13 verbatim, so
/// what this pins is that the shim loads and that a port reached through it
/// round-trips — not that string ports work, which the chibi R7RS suite
/// covers.
#[test]
fn test_srfi_6_string_ports_are_reachable() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 6))
             (let ((op (open-output-string)))
               (write-string \"ab\" op)
               (list (get-output-string op) (read-char (open-input-string \"z\"))))"
        ),
        "(\"ab\" #\\z)"
    );
}

/// `(srfi 9)`'s `define-record-type` is R7RS 5.5's, which is a *superset*: the
/// SRFI requires an accessor per field, R7RS additionally allows a modifier.
/// Both shapes are pinned, because a shim that reached a narrower macro would
/// still pass the first.
#[test]
fn test_srfi_9_define_record_type_accepts_both_shapes() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 9))
             (define-record-type <a> (mk-a x) a? (x a-x))
             (define-record-type <b> (mk-b y) b? (y b-y set-b-y!))
             (define v (mk-b 1))
             (set-b-y! v 9)
             (list (a-x (mk-a 5)) (b-y v))"
        ),
        "(5 9)"
    );
}

/// `(srfi 11)`. Not to be confused with `(srfi 8)`'s `receive`, which is a
/// different SRFI and a separate library here.
#[test]
fn test_srfi_11_value_binding_forms_are_reachable() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 11))
             (list (let-values (((a b) (values 1 2))) (+ a b))
                   (let*-values (((a) (values 1)) ((b) (values (+ a 1)))) b))"
        ),
        "(3 2)"
    );
}

/// `(srfi 39)`. The restore after the body is the part worth pinning: a shim
/// that reached something other than R7RS's `parameterize` could return the
/// new value and still look right inside the body.
#[test]
fn test_srfi_39_parameterize_restores_after_the_body() {
    assert_eq!(
        eval(
            "(import (scheme base) (srfi 39))
             (define p (make-parameter 10))
             (list (p) (parameterize ((p 20)) (p)) (p))"
        ),
        "(10 20 10)"
    );
}
