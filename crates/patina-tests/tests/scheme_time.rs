//! Tests for (scheme time) library
//!
//! R7RS §6.13 Time procedures

use patina_interpreter::TreeWalkInterpreter;

// ============================================================================
// current-second
// ============================================================================

#[test]
fn test_current_second_returns_inexact() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(inexact? (current-second))").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_second_is_positive() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(> (current-second) 0)").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_second_is_after_2020() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    // 2020-01-01 00:00:00 UTC = 1577836800
    let result = interp.eval_str("(> (current-second) 1577836800)").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_second_increases() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    // Two successive calls should return >= values
    let result = interp
        .eval_str(
            r#"
            (let ((t1 (current-second)))
              (>= (current-second) t1))
            "#,
        )
        .unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

// ============================================================================
// current-jiffy
// ============================================================================

#[test]
fn test_current_jiffy_returns_exact() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(exact? (current-jiffy))").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_jiffy_is_integer() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(integer? (current-jiffy))").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_jiffy_is_non_negative() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(>= (current-jiffy) 0)").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_current_jiffy_increases() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    // Two successive calls should return >= values
    let result = interp
        .eval_str(
            r#"
            (let ((j1 (current-jiffy)))
              (>= (current-jiffy) j1))
            "#,
        )
        .unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

// ============================================================================
// jiffies-per-second
// ============================================================================

#[test]
fn test_jiffies_per_second_returns_exact() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(exact? (jiffies-per-second))").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_jiffies_per_second_is_integer() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(integer? (jiffies-per-second))").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_jiffies_per_second_is_positive() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(> (jiffies-per-second) 0)").unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_jiffies_per_second_is_constant() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    // Multiple calls should return the same value
    let result = interp
        .eval_str("(= (jiffies-per-second) (jiffies-per-second))")
        .unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}

#[test]
fn test_jiffies_per_second_value() {
    // We use microsecond resolution (1,000,000 jiffies per second)
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp.eval_str("(jiffies-per-second)").unwrap();
    assert_eq!(interp.display_tagged(result), "1000000");
}

// ============================================================================
// Integration: time-length example from R7RS spec
// ============================================================================

#[test]
fn test_time_length_example() {
    // Example from R7RS spec §6.13
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(import (scheme time))").unwrap();

    let result = interp
        .eval_program(
            r#"
            (define (time-length)
              (let ((list (make-list 1000))
                    (start (current-jiffy)))
                (length list)
                (/ (- (current-jiffy) start)
                   (jiffies-per-second))))

            ;; Should return a small positive number (fraction of a second)
            (let ((duration (time-length)))
              (and (>= duration 0)
                   (< duration 1)))
            "#,
        )
        .unwrap();
    assert_eq!(interp.display_tagged(result), "#t");
}
