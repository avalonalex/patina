//! Tests for the high-level Interpreter API
//!
//! These tests verify the public API provided by the main `patina` crate

use patina_interpreter::{TaggedValue, TreeWalkInterpreter};

#[test]
fn test_interpreter_basic_arithmetic() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    assert_eq!(result.as_fixnum(), Some(6));
}

#[test]
fn test_interpreter_define_and_use() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    interp.eval_str("(define x 42)").unwrap();
    let result = interp.eval_str("x").unwrap();
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn test_eval_program() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_program(
            r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#,
        )
        .unwrap();
    assert_eq!(result.as_fixnum(), Some(30));
}

#[test]
fn test_macro_when() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define the when macro
    let define_result = interp.eval_str(
        r#"
(define-syntax test-when
  (syntax-rules ()
    ((test-when test body ...)
     (if test (begin body ...)))))
"#,
    );

    if let Err(e) = define_result {
        panic!("Failed to define when macro: {}", e);
    }

    // Test single body
    let result = interp.eval_str("(test-when #t 42)");
    match &result {
        Ok(val) => println!("when macro result: {}", interp.display_tagged(*val)),
        Err(e) => panic!("when macro expansion error: {}", e),
    }
    let result = result.unwrap();
    assert_eq!(result.as_fixnum(), Some(42));

    // Test multiple body forms
    let result = interp.eval_str("(test-when #t 1 2 3)").unwrap();
    assert_eq!(result.as_fixnum(), Some(3));

    // Test false condition
    let result = interp.eval_str("(test-when #f 42)").unwrap();
    assert_eq!(result, TaggedValue::UNSPECIFIED);
}

#[test]
fn test_macro_unless() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define the unless macro
    interp
        .eval_str(
            r#"
(define-syntax test-unless
  (syntax-rules ()
    ((test-unless test body ...)
     (if (not test) (begin body ...)))))
"#,
        )
        .unwrap();

    // Test with false condition (should execute)
    let result = interp.eval_str("(test-unless #f 42)").unwrap();
    assert_eq!(result.as_fixnum(), Some(42));

    // Test with true condition (should not execute)
    let result = interp.eval_str("(test-unless #t 42)").unwrap();
    assert_eq!(result, TaggedValue::UNSPECIFIED);
}

#[test]
fn test_gcd_with_let_values() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp
        .eval_program(
            r#"
            (define (quotient-and-remainder a b)
              (values (quotient a b) (remainder a b)))

            (define (gcd a b)
              (if (= b 0)
                  a
                  (let-values (((q r) (quotient-and-remainder a b)))
                    (gcd b r))))

            (gcd 48 18)
        "#,
        )
        .unwrap();

    assert_eq!(result.as_fixnum(), Some(6));
}

#[test]
fn source_map_entries_pruned_after_collection() {
    // GC_DESIGN.md §9.1: SourceMap is keyed by raw bits, so a slot the GC
    // reclaims must lose its entry before the slot can be reused — otherwise
    // a later value inherits the old datum's source location. The two
    // programs are identical except that one collects; the quoted list is
    // garbage by then, so its entries must be gone from the returned map.
    fn map_len(middle_form: &str) -> usize {
        let interp = TreeWalkInterpreter::new_tree_walker();
        let program =
            format!("(import (scheme base) (patina debug))\n'(a b c d e f)\n{middle_form}\n42\n");
        let (result, source_map) = interp.eval_program_with_source_name(&program, "prune-test");
        assert_eq!(result.unwrap().as_fixnum(), Some(42));
        let len = source_map.borrow().len();
        assert!(len > 0, "parser recorded nothing");
        len
    }

    let collected = map_len("(gc)");
    let uncollected = map_len("(list)");
    assert!(
        collected < uncollected,
        "collection did not prune the source map: {collected} entries with (gc) \
         vs {uncollected} with (list)"
    );
}
