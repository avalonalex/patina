//! The three standard ports are parameter objects (R7RS §6.13.1).
//!
//! They used to be plain 0-argument procedures, so `parameterize` rejected them
//! outright — the one failure in SRFI 158's upstream suite, which defines its
//! own `with-input-from-string` in exactly those terms.
//!
//! What makes this more than an arity change: the current port lives in a
//! thread-local that primitives read directly, so `display` with no port
//! argument consults it rather than any Scheme binding. `parameterize` drives a
//! parameter through the object itself — `(p)` to read, `(p v)` to install,
//! `(p old)` from `dynamic-wind` to restore — so installing through the setter
//! arity *is* rebinding the thing those primitives read. A Scheme-level
//! rebinding that left the thread-local alone would look right and redirect
//! nothing.

mod common;
use common::eval_program as eval;

const BASE: &str = "(import (scheme base) (scheme read) (scheme write))";

#[test]
fn test_parameterize_current_input_port() {
    // The case the SRFI 158 suite hits.
    assert_eq!(
        eval(&format!(
            "{BASE} (parameterize ((current-input-port (open-input-string \"a b c\"))) (read))"
        )),
        "a"
    );
}

/// The point of the exercise: a primitive that takes an optional port and was
/// given none must observe the rebinding.
#[test]
fn test_primitives_with_no_port_argument_follow_the_parameter() {
    assert_eq!(
        eval(&format!(
            "{BASE}
             (define out (open-output-string))
             (parameterize ((current-output-port out))
               (display \"a\") (write 42) (newline) (write-string \"b\"))
             (get-output-string out)"
        )),
        "\"a42\\nb\""
    );
}

/// Restoration is checked by where output *lands*, not by port identity:
/// `(current-output-port)` allocates a fresh wrapper per call, so `eq?` on two
/// reads is `#f` regardless (pre-existing, noted in the Track L PRD). Nesting
/// two string ports also keeps the test off real stdout.
#[test]
fn test_the_previous_port_is_restored() {
    assert_eq!(
        eval(&format!(
            "{BASE}
             (define outer (open-output-string))
             (define inner (open-output-string))
             (parameterize ((current-output-port outer))
               (parameterize ((current-output-port inner)) (display \"in\"))
               (display \"out\"))
             (list (get-output-string inner) (get-output-string outer))"
        )),
        "(\"in\" \"out\")"
    );
}

/// `parameterize` restores through `dynamic-wind`, so an escape must not leave
/// the world writing into a string port.
#[test]
fn test_escaping_restores_the_port() {
    assert_eq!(
        eval(&format!(
            "{BASE}
             (define outer (open-output-string))
             (define inner (open-output-string))
             (parameterize ((current-output-port outer))
               (call-with-current-continuation
                 (lambda (k)
                   (parameterize ((current-output-port inner)) (display \"in\") (k 'escaped))))
               (display \"after\"))
             (list (get-output-string inner) (get-output-string outer))"
        )),
        "(\"in\" \"after\")"
    );
}

#[test]
fn test_all_three_ports_are_parameters() {
    assert_eq!(
        eval(&format!(
            "{BASE}
             (define o (open-output-string))
             (define e (open-output-string))
             (list (parameterize ((current-input-port (open-input-string \"x\"))) (read))
                   (begin (parameterize ((current-output-port o)) (display 1))
                          (get-output-string o))
                   (begin (parameterize ((current-error-port e)) (display 2 (current-error-port)))
                          (get-output-string e)))"
        )),
        "(x \"1\" \"2\")"
    );
}

/// Reading is still the zero-argument case, and a non-port is still rejected —
/// the setter arity must not turn these into procedures that accept anything.
#[test]
fn test_reading_and_type_checking() {
    assert_eq!(
        eval(&format!("{BASE} (output-port? (current-output-port))")),
        "#t"
    );
    assert_eq!(
        eval(&format!("{BASE} (input-port? (current-input-port))")),
        "#t"
    );
    for expr in [
        "(current-output-port 5)",
        "(current-input-port (open-output-string))",
        "(current-output-port (open-input-string \"x\"))",
    ] {
        let out = eval(&format!(
            "{BASE} (guard (e (#t 'rejected)) {expr} 'accepted)"
        ));
        assert_eq!(out, "rejected", "{expr} should be rejected");
    }
}
