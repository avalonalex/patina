//! Tests for circular/shared data handling (datum labels)

mod common;
use common::*;

// =============================================================================
// write on circular data (via read)
// =============================================================================

#[test]
fn test_write_circular_pair() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_write_circular_list() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a b . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a b . #0#)""##);
}

#[test]
fn test_display_circular_pair() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (display x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_write_shared_circular() {
    let code = r##"
        (import (scheme read) (scheme write))
        (define p (open-input-string "#0=(a . #0#)"))
        (define x (read p))
        (let ((out (open-output-string)))
          (write-shared x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

// =============================================================================
// Quoted circular literals (previously caused stack overflow)
// =============================================================================

#[test]
fn test_quoted_circular_literal_write() {
    let code = r##"
        (import (scheme write))
        (define x '#0=(a . #0#))
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a . #0#)""##);
}

#[test]
fn test_quoted_circular_literal_car() {
    let code = r##"
        (define x '#0=(hello . #0#))
        (car x)
    "##;
    assert_program_eval_to(code, "hello");
}

#[test]
fn test_quoted_circular_eq() {
    // The cdr of a circular pair should be eq? to the pair itself
    let code = r##"
        (define x '#0=(a . #0#))
        (eq? x (cdr x))
    "##;
    assert_program_eval_to(code, "#t");
}

// =============================================================================
// Non-circular data still works
// =============================================================================

#[test]
fn test_write_non_circular_list() {
    let code = r#"
        (import (scheme write))
        (let ((out (open-output-string)))
          (write '(1 2 3) out)
          (get-output-string out))
    "#;
    assert_program_eval_to(code, r#""(1 2 3)""#);
}

#[test]
fn test_write_non_circular_vector() {
    let code = r##"
        (import (scheme write))
        (let ((out (open-output-string)))
          (write '#(a b c) out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#(a b c)""##);
}

// =============================================================================
// Mutable circular structures (set-cdr!)
// =============================================================================

#[test]
fn test_write_set_cdr_circular() {
    let code = r##"
        (import (scheme write))
        (define x (list 'a 'b))
        (set-cdr! (cdr x) x)
        (let ((out (open-output-string)))
          (write x out)
          (get-output-string out))
    "##;
    assert_program_eval_to(code, r##""#0=(a b . #0#)""##);
}
