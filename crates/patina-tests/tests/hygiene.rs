//! Tests for macro hygiene
//!
//! These tests verify that the macro system properly implements hygienic renaming
//! to prevent macro-introduced identifiers from capturing user bindings.
//!
//! **17 of the 18 tests here run on the tree-walker only.** They build
//! `TreeWalkInterpreter::new_tree_walker()` by hand, which is what the file did
//! before `common::assert_program_eval_to` existed. This is a gap and not a
//! decision — the VM is the default backend, and the hygiene defects Larceny
//! families 36 and 40 record are VM-side, so a VM-only regression passes every
//! one of them. Do not read the hand-built interpreter as "this test is about
//! the tree-walker"; where a test really is about one backend,
//! `assert_divergence` says so and the row belongs in `backend_divergence.rs`.
//!
//! **The file is being migrated, not kept.** Its rows are portable value
//! assertions whose home is `tests/scheme/expansion/`, where they run on both
//! backends and under chibi and Gauche. The literal-matching rows went first,
//! to `syntax-rules-literals.scm` and the `let-syntax` rows to
//! `let-syntax.scm`, the ellipsis-escape rows to `ellipsis.scm` and the two
//! about `_` to `syntax-rules-literals.scm` — 49 tests are now 18 — and moving
//! them found
//! two things a Rust comment could not: one row that asserted only "did not
//! error", now pinned at the value all four implementations give, and one whose
//! comment claimed Gauche agreed with it when Gauche never has (shirok/Gauche
//! #1327). **Add a new portable hygiene row to the `.scm` files, not here.**
//!
//! One cross-reference this file used to carry, restored because the first
//! slice deleted it along with a section banner: the rebound-`else` claim lives
//! in `core_syntax_bindings.rs::test_a_rebound_else_does_not_match`, which
//! moved there from here when `else` became a syntactic binding, and its other
//! polarity is now a row of `syntax-rules-literals.scm`.
//! `compliance/derived.rs` holds the unshadowed regression guards for
//! `cond`/`case`. A fourth copy of any of those is what
//! `core_syntax_bindings.rs`'s own comment warns against.
//!
//! Measured 2026-09-09 before the first slice: the 46 programs held by the 44
//! hand-built-interpreter tests answer identically on the VM and the
//! tree-walker, so the gap hides no current defect — closing it buys permanent
//! coverage, not a bug fix. (The five helper-based tests were not in that set;
//! they already ran both backends, which is the point.)
//!
//! `hygiene_matrix.rs` is a different instrument again — 28 shapes scored
//! against chibi and Racket, read as a table when a fix moves a row — and
//! stays in Rust.
//!
//! Related issues:
//! - https://github.com/avalonalex/patina/issues/12

mod common;
use common::assert_program_eval_to;
use patina_interpreter::TreeWalkInterpreter;

/// Test that macro-introduced identifiers don't capture user bindings
///
/// Issue #12: https://github.com/avalonalex/patina/issues/12
///
/// This is the classic hygiene test: when a pattern variable (body) contains
/// an identifier (temp), that identifier should refer to its binding at the
/// USE-SITE, not to any macro-introduced binding with the same name.
///
/// Verified against chibi-scheme which also returns 5.
#[test]
fn test_macro_introduced_temp_variable() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define a macro that introduces a 'temp' variable
    let result = interp.eval_program(
        r#"
        (define-syntax my-let
          (syntax-rules ()
            ((my-let x body)
             (let ((temp x)) body))))

        (let ((temp 5))
          (my-let 10 temp))
        "#,
    );

    // Should return 5 (the user's temp binding from use-site)
    // NOT 10 (the macro-introduced temp binding)
    // The 'temp' in body came from the use-site where temp=5.
    // The macro's (let ((temp 10)) ...) creates a DIFFERENT temp due to hygiene.
    assert!(
        result.is_ok(),
        "Macro should expand correctly: {:?}",
        result
    );
    assert_eq!(interp.display_tagged(result.unwrap()), "5");
}

/// Test hygiene with nested lets
#[test]
fn test_nested_let_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax my-swap
          (syntax-rules ()
            ((my-swap a b)
             (let ((temp a))
               (set! a b)
               (set! b temp)))))

        (let ((x 1)
              (y 2)
              (temp 999))
          (my-swap x y)
          (list x y temp))
        "#,
    );

    assert!(result.is_ok());
    // x and y should be swapped, temp should still be 999
    let val = result.unwrap();
    assert_eq!(interp.display_tagged(val), "(2 1 999)");
}

/// Test that macro-introduced 'if' doesn't capture user 'if'
#[test]
fn test_special_form_not_captured() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax my-cond
          (syntax-rules ()
            ((my-cond test then-clause)
             (if test then-clause #f))))

        (let ((if 'captured))
          (my-cond #t 'success))
        "#,
    );

    assert!(result.is_ok());
    // Should return 'success, not try to use 'if' as a procedure
    assert_eq!(interp.display_tagged(result.unwrap()), "success");
}

/// Test hygiene with multiple macro-introduced bindings
#[test]
fn test_multiple_introduced_bindings() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax with-temps
          (syntax-rules ()
            ((with-temps expr)
             (let ((temp1 1)
                   (temp2 2))
               (+ temp1 temp2 expr)))))

        (let ((temp1 100)
              (temp2 200))
          (with-temps 3))
        "#,
    );

    assert!(result.is_ok());
    // Should return 1 + 2 + 3 = 6, not use the user's temp1/temp2
    assert_eq!(interp.display_tagged(result.unwrap()), "6");
}

/// Test that pattern variables from the call site are preserved
#[test]
fn test_pattern_variable_preservation() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax apply-twice
          (syntax-rules ()
            ((apply-twice f x)
             (f (f x)))))

        (define (double x) (* 2 x))
        (apply-twice double 3)
        "#,
    );

    assert!(result.is_ok());
    // Should return 12 (double applied twice to 3)
    assert_eq!(interp.display_tagged(result.unwrap()), "12");
}

/// Test hygiene with lambda
#[test]
fn test_lambda_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-adder
          (syntax-rules ()
            ((make-adder n)
             (lambda (x) (+ x n)))))

        (let ((x 100)
              (n 200))
          ((make-adder 5) 3))
        "#,
    );

    assert!(result.is_ok());
    // Should return 8 (3 + 5), using the macro's n, not the outer bindings
    assert_eq!(interp.display_tagged(result.unwrap()), "8");
}

/// Test that quoted symbols in templates are not renamed
#[test]
fn test_quoted_symbols_not_renamed() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-symbol
          (syntax-rules ()
            ((make-symbol)
             'temp)))

        (eq? (make-symbol) 'temp)
        "#,
    );

    assert!(result.is_ok());
    // The quoted 'temp should remain as the symbol temp
    assert_eq!(interp.display_tagged(result.unwrap()), "#t");
}

/// Test hygiene doesn't break recursive macros
///
/// This tests that recursive macro expansion properly maintains hygiene
/// by generating multiple nested `let` bindings with the same `temp` name.
/// Each expansion should create a distinct `temp` that doesn't shadow
/// the others due to hygiene.
///
/// Note: The original test tried to match runtime values against compile-time
/// patterns (e.g., `(countdown 0)` matching `(countdown (- temp 1))`), which
/// is impossible - macros expand at compile-time before evaluation.
#[test]
fn test_recursive_macro_hygiene() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Test recursive macro that creates nested let bindings
    // Each expansion introduces a fresh `temp` variable that should be hygienic
    let result = interp.eval_program(
        r#"
        (define-syntax nested-let
          (syntax-rules ()
            ((nested-let () body) body)
            ((nested-let (val) body)
             (let ((temp val)) (+ temp body)))
            ((nested-let (val . rest) body)
             (let ((temp val)) (nested-let rest (+ temp body))))))

        (nested-let (1 2 3) 0)
        "#,
    );

    // Expands to: (let ((temp 1)) (let ((temp 2)) (let ((temp 3)) (+ temp (+ temp (+ temp 0))))))
    // With hygiene, each temp is distinct, so result = 1 + (2 + (3 + 0)) = 6
    assert!(
        result.is_ok(),
        "Recursive macro should evaluate: {:?}",
        result
    );
    assert_eq!(interp.display_tagged(result.unwrap()), "6");
}

// =============================================================================
// Nested Macro Definition Tests
// =============================================================================
//
// R7RS: Macros that generate other macros using ellipsis escape should work
// correctly. The inner macro's ellipsis should be preserved as a literal
// symbol so it can be recognized when the inner macro is compiled.

/// Test simple nested macro definition
///
/// A macro that generates another macro with no ellipsis in the inner template.
#[test]
fn test_nested_macro_simple() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax gen-const-macro
          (syntax-rules ()
            ((gen-const-macro name value)
             (... (define-syntax name
                    (syntax-rules ()
                      ((name) value)))))))

        (gen-const-macro answer 42)
        (answer)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(interp.display_tagged(result.unwrap()), "42");
}

/// Test nested macro with ellipsis in inner template
///
/// This is the classic "be-like-begin" example from chibi-scheme tests.
/// The outer macro generates an inner macro that uses ellipsis.
#[test]
fn test_nested_macro_with_ellipsis() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax be-like-begin
          (syntax-rules ()
            ((be-like-begin name)
             (define-syntax name
               (... (syntax-rules ()
                      ((name expr ...)
                       (begin expr ...))))))))

        (be-like-begin sequence)
        (sequence 1 2 3 4)
        "#,
    );

    assert!(result.is_ok());
    // sequence should work like begin, returning the last value
    assert_eq!(interp.display_tagged(result.unwrap()), "4");
}

/// Test nested macro generating a list macro
#[test]
fn test_nested_macro_listify() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax gen-list-macro
          (syntax-rules ()
            ((gen-list-macro name)
             (... (define-syntax name
                    (syntax-rules ()
                      ((name x ...)
                       (list x ...))))))))

        (gen-list-macro make-list)
        (make-list 1 2 3 4 5)
        "#,
    );

    assert!(result.is_ok());
    let val = result.unwrap();
    assert_eq!(interp.display_tagged(val), "(1 2 3 4 5)");
}

/// Test macro-generating macros: a macro that expands to define-syntax
///
/// This tests that when a macro call expands to a `define-syntax` form,
/// the resulting macro is properly compiled and added to the environment
/// for subsequent expressions.
///
/// This test uses non-conflicting names (my-bar, hello) so it should work.
#[test]
fn test_macro_generating_macro_simple() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define-syntax foo
            (syntax-rules ()
              ((foo bar y)
               (define-syntax bar
                 (syntax-rules ()
                   ((bar x) 'y))))))
          (foo my-bar hello)
          (my-bar 1))
        "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "hello");
}

/// Test macro-generating macros with conflicting names (R7RS requirement)
///
/// This is the chibi r7rs-tests.scm test case that requires correct hygiene
/// for macro-generating macros. When (foo bar x) is called:
/// - Pattern variable `bar` gets bound to symbol `bar`
/// - Pattern variable `y` gets bound to symbol `x`
/// - The inner macro is: (define-syntax bar (syntax-rules () ((bar x) 'x)))
/// - The `x` in `'x` should be the value from `y` (symbol `x`), NOT the inner
///   pattern variable `x` (which would be bound to `1` when (bar 1) is called)
#[test]
fn test_macro_generating_macro_conflicting_names() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // This is the exact test from chibi r7rs-tests.scm
    let result = interp.eval_program(
        r#"
        (let ()
          (define-syntax foo
            (syntax-rules ()
              ((foo bar y)
               (define-syntax bar
                 (syntax-rules ()
                   ((bar x) 'y))))))
          (foo bar x)
          (bar 1))
        "#,
    );

    // Returns 'x (the symbol x from the outer macro call)
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "x");
}

/// Test macro-generating macros with multiple generated macros
#[test]
fn test_macro_generating_macro_multiple() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define-syntax make-const
            (syntax-rules ()
              ((make-const name value)
               (define-syntax name
                 (syntax-rules ()
                   ((name) value))))))
          (make-const ten 10)
          (make-const twenty 20)
          (+ (ten) (twenty)))
        "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "30");
}

/// Test macro-generating macros where generated macro uses the enclosing environment
#[test]
fn test_macro_generating_macro_captures_env() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define base 100)
          (define-syntax make-adder
            (syntax-rules ()
              ((make-adder name offset)
               (define-syntax name
                 (syntax-rules ()
                   ((name x) (+ base offset x)))))))
          (make-adder add10 10)
          (add10 5))
        "#,
    );

    // base=100, offset=10, x=5 => 100 + 10 + 5 = 115
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "115");
}

// ===== Issue 1 Fix Tests: Identifier vs Symbol in Definition Names =====
// These tests verify the fixes for macro-generated definitions where names
// come from pattern variable substitution (producing Identifier, not Symbol)

/// Test macro that generates a define with function name from pattern variable
/// This was failing with "define function name must be a symbol" before the fix
#[test]
fn test_macro_generated_function_definition() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define-syntax def-square
            (syntax-rules ()
              ((def-square name)
               (define (name x) (* x x)))))
          (def-square my-square)
          (my-square 5))
        "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "25");
}

/// Test macro generating let-syntax with macro name from pattern variable
#[test]
fn test_macro_generated_let_syntax() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-local-const
          (syntax-rules ()
            ((make-local-const name val body)
             (let-syntax ((name (syntax-rules () ((name) val))))
               body))))

        (make-local-const answer 42 (+ (answer) 1))
        "#,
    );

    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(interp.display_tagged(result.unwrap()), "43");
}

#[test]
fn test_generated_template_capture_keeps_expansions_distinct() {
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax bind-tmp
           (syntax-rules ()
             ((_ (k ...) v) (k ... (tmp . v)))))
         (define-syntax through-template
           (syntax-rules ()
             ((_ k v)
              (let-syntax ((go (syntax-rules () ((go) (k v)))))
                (go)))))
         (define-syntax done
           (syntax-rules ()
             ((_ (a b)) ((lambda (a b) (list a b)) 1 2))))
         (bind-tmp (bind-tmp (through-template done)) ())",
        "(1 2)",
    );
}
