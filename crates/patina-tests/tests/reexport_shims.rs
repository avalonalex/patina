//! The pure re-export shims: `(srfi 23)`, `(srfi 98)` and `(scheme small)`.
//!
//! Each name is functionality R7RS already provides, imported by the vendored
//! corpus under its pre-R7RS or R7RS-large name — srfi-78 reaches `(srfi 23)`,
//! jkode-sassy imports `(srfi 98)`, chrisoei-cint imports `(scheme small)`.
//! There is nothing to test behaviourally beyond what the backing libraries'
//! own suites cover; what these tests pin is that each shim *loads* and that
//! a binding reached through it is usable, which is exactly the probe the
//! compat harness runs.

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
