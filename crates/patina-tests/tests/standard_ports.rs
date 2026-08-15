//! The three standard ports are parameter objects (R7RS §6.13.1).
//!
//! They used to be plain 0-argument procedures, so `parameterize` rejected them
//! outright — the one failure in SRFI 158's upstream suite, which defines its
//! own `with-input-from-string` in exactly those terms. Why writing the backing
//! thread-local *is* rebinding rather than a way around it is explained above
//! the three procedures in `patina-primitives/src/primitives/io/ports.rs`.
//!
//! Restoration is asserted by where output *lands*, never by port identity:
//! `(current-output-port)` allocates a fresh wrapper per call, so `eq?` on two
//! reads is `#f` regardless (pre-existing, recorded in the Track L PRD).
//! Nesting two string ports also keeps these tests off real stdout.

mod common;
use common::eval_program as eval;

/// Sibling per-topic files (`srfi_151_bitwise.rs`, `srfi_130_string.rs`) wrap
/// the import prologue in a small helper rather than repeating it per case.
fn ports(expr: &str) -> String {
    eval(&format!(
        "(import (scheme base) (scheme read) (scheme write)) {expr}"
    ))
}

/// The case the SRFI 158 suite hits, and the one the defect was reported as.
#[test]
fn test_parameterize_current_input_port() {
    assert_eq!(
        ports(r#"(parameterize ((current-input-port (open-input-string "a b c"))) (read))"#),
        "a"
    );
}

/// The point of the exercise: a primitive given no port argument reads the
/// thread-local, so it must observe the rebinding.
#[test]
fn test_primitives_with_no_port_argument_follow_the_parameter() {
    assert_eq!(
        ports(
            r#"(define out (open-output-string))
               (parameterize ((current-output-port out))
                 (display "a") (write 42) (newline) (write-string "b"))
               (get-output-string out)"#
        ),
        r#""a42\nb""#
    );
}

#[test]
fn test_the_previous_port_is_restored() {
    assert_eq!(
        ports(
            r#"(define outer (open-output-string))
               (define inner (open-output-string))
               (parameterize ((current-output-port outer))
                 (parameterize ((current-output-port inner)) (display "in"))
                 (display "out"))
               (list (get-output-string inner) (get-output-string outer))"#
        ),
        r#"("in" "out")"#
    );
}

/// `parameterize` restores through `dynamic-wind`, so an escape must not leave
/// the world writing into a string port.
#[test]
fn test_escaping_restores_the_port() {
    assert_eq!(
        ports(
            r#"(define outer (open-output-string))
               (define inner (open-output-string))
               (parameterize ((current-output-port outer))
                 (call-with-current-continuation
                   (lambda (k)
                     (parameterize ((current-output-port inner)) (display "in") (k 'escaped))))
                 (display "after"))
               (list (get-output-string inner) (get-output-string outer))"#
        ),
        r#"("in" "after")"#
    );
}

/// The third port, which the cases above do not otherwise reach.
#[test]
fn test_current_error_port_is_a_parameter() {
    assert_eq!(
        ports(
            r#"(define e (open-output-string))
               (parameterize ((current-error-port e)) (display 2 (current-error-port)))
               (get-output-string e)"#
        ),
        r#""2""#
    );
}

/// Reading is still the zero-argument case.
#[test]
fn test_reading_takes_no_argument() {
    assert_eq!(ports("(output-port? (current-output-port))"), "#t");
    assert_eq!(ports("(input-port? (current-input-port))"), "#t");
    assert_eq!(ports("(output-port? (current-error-port))"), "#t");
}

/// The setter arity must not turn these into procedures that accept anything:
/// a non-port, and a port facing the wrong way, are both still rejected.
#[test]
fn test_a_non_port_or_wrong_direction_is_rejected() {
    for expr in [
        "(current-output-port 5)",
        r#"(current-input-port (open-output-string))"#,
        r#"(current-output-port (open-input-string "x"))"#,
    ] {
        assert_eq!(
            ports(&format!("(guard (e (#t 'rejected)) {expr} 'accepted)")),
            "rejected",
            "{expr} should be rejected"
        );
    }
}
