//! Tests for the high-level Interpreter API
//!
//! These tests verify the public API provided by the main `patina` crate

use patina_interpreter::{Interpreter, TaggedValue, TreeWalkInterpreter};
use patina_runtime::Backend;
use patina_vm::VmBackend;

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

/// A form that fails must not leave the interpreter in the middle of it. The
/// REPL and the resilient script mode both evaluate form by form on one
/// interpreter, and an error is where the VM used to be left holding the
/// frames, handlers and winds of the form it had abandoned: the next form's
/// own error was then delivered to the *previous* form's handler, or its
/// return landed in a frame that no longer had a program behind it (review of
/// triage families 22/28, 2026-09-01). Both failing shapes below stop the
/// program with something still installed — an unhandled exception from an
/// after-thunk mid-unwind, and a handler raising from inside a handler — and
/// the probe after each is that `car`'s own error is what comes back, then
/// that a plain form evaluates.
///
/// The after-thunk shape has no handler at all, on purpose: with a `guard`
/// around it the secondary is *caught* under the `finally` rule (R7RS 6.10,
/// `wind_thunk_exceptions.rs`), so that form stops being an error on a
/// backend that meets the rule, and it did on the tree-walker.
#[test]
fn an_error_leaves_the_interpreter_ready_for_the_next_form() {
    fn check<B: Backend>(interp: Interpreter<B>, name: &str) {
        for failing in [
            "(call/cc (lambda (k) (dynamic-wind (lambda () #f) (lambda () (k 1)) (lambda () (raise 'sec)))))",
            "(with-exception-handler (lambda (e) (raise 'inner)) (lambda () (car 5)))",
        ] {
            assert!(
                interp.eval_str(failing).is_err(),
                "{name}: `{failing}` should fail"
            );
            let next = interp.eval_str("(car 5)");
            let message = next.as_ref().map(|_| ()).map_err(|e| e.to_string());
            assert!(
                message.as_ref().is_err_and(|m| m.contains("car")),
                "{name}: after `{failing}`, `(car 5)` should fail as car's own error, got {message:?}"
            );
            let result = interp
                .eval_str("(+ 1 2)")
                .unwrap_or_else(|e| panic!("{name}: after `{failing}`, `(+ 1 2)` failed: {e}"));
            assert_eq!(result.as_fixnum(), Some(3), "{name}: after `{failing}`");
        }
    }
    check(TreeWalkInterpreter::new_tree_walker(), "tree-walker");
    check(Interpreter::new(VmBackend::new()), "vm");
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
