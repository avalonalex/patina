//! Tests for macro hygiene
//!
//! These tests verify that the macro system properly implements hygienic renaming
//! to prevent macro-introduced identifiers from capturing user bindings.
//!
//! Related issues:
//! - https://github.com/avalonalex/patina/issues/12

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
    assert_eq!(result.unwrap().to_string(), "5");
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
    assert_eq!(result.unwrap().to_string(), "(2 1 999)");
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
    assert_eq!(result.unwrap().to_string(), "success");
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
    assert_eq!(result.unwrap().to_string(), "6");
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
    assert_eq!(result.unwrap().to_string(), "12");
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
    assert_eq!(result.unwrap().to_string(), "8");
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
    assert_eq!(result.unwrap().to_string(), "#t");
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
    assert_eq!(result.unwrap().to_string(), "6");
}

/// Test scope-based hygiene: macro captures binding from definition site
///
/// This is the classic R7RS hygiene test case where a macro defined inside
/// a let should capture the outer binding of 'x', not the inner one.
///
/// From R7RS Section 4.3: "If a macro transformer inserts a free reference
/// to an identifier, the reference refers to the binding that was visible
/// where the transformer was specified, regardless of any local bindings
/// that surround the use of the macro."
#[test]
fn test_let_syntax_captures_definition_site_binding() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((x 'outer))
          (let-syntax ((m (syntax-rules () ((m) x))))
            (let ((x 'inner))
              (m))))
        "#,
    );

    assert!(result.is_ok());
    // The macro's 'x' should refer to 'outer' (definition site),
    // not 'inner' (expansion site)
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with define wrapper
///
/// Same as above but wrapped in a define to ensure hygiene works
/// across function boundaries.
#[test]
fn test_let_syntax_hygiene_in_define() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define (test-hygiene)
          (let ((x 'outer))
            (let-syntax ((m (syntax-rules () ((m) x))))
              (let ((x 'inner))
                (m)))))
        (test-hygiene)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with lambda wrapper
///
/// Ensure hygiene works when the macro is defined inside a lambda parameter scope.
#[test]
fn test_let_syntax_hygiene_in_lambda() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        ((lambda (x)
           (let-syntax ((m (syntax-rules () ((m) x))))
             (let ((x 'inner))
               (m))))
         'outer)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "outer");
}

/// Test scope-based hygiene with multiple nested lets
#[test]
fn test_let_syntax_hygiene_multiple_nesting() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((x 'level1))
          (let ((y 'level2))
            (let-syntax ((m (syntax-rules () ((m) x))))
              (let ((x 'inner))
                (m)))))
        "#,
    );

    assert!(result.is_ok());
    // Should return 'level1, not 'inner
    assert_eq!(result.unwrap().to_string(), "level1");
}

// =============================================================================
// Underscore Literal Tests
// =============================================================================
//
// R7RS Section 4.3.2: When `_` appears in the literals list, it should only
// match the literal symbol `_`, not act as a wildcard pattern.

/// Test underscore as wildcard (default behavior when not in literals)
#[test]
fn test_underscore_as_wildcard() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax count-args
          (syntax-rules ()
            ((_ a) 1)
            ((_ a b) 2)
            ((_ a b c) 3)))

        (list (count-args x) (count-args x y) (count-args x y z))
        "#,
    );

    assert!(result.is_ok());
    // _ should match any symbol in macro keyword position
    assert_eq!(result.unwrap().to_string(), "(1 2 3)");
}

/// Test underscore as literal (when explicitly in literals list)
///
/// R7RS: When `_` is in the literals list, `_` in patterns should only
/// match the literal symbol `_`, not act as a wildcard.
#[test]
fn test_underscore_as_literal() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax count-to-2_
          (syntax-rules (_)
            ((_) 0)
            ((_ _) 1)
            ((_ _ _) 2)
            ((x . y) 'fail)))

        (list (count-to-2_ _ _)
              (count-to-2_)
              (count-to-2_ a b)
              (count-to-2_ a b c d))
        "#,
    );

    assert!(result.is_ok());
    // Pattern (_ _ _) should only match literal _, not a and b
    assert_eq!(result.unwrap().to_string(), "(2 0 fail fail)");
}

// =============================================================================
// Ellipsis Escape Tests
// =============================================================================
//
// R7RS Section 4.3.2: "(... template)" is an escape form that produces
// "template" with ellipsis treated as a regular symbol.

/// Test basic ellipsis escape: (... ...) produces literal ...
#[test]
fn test_ellipsis_escape_basic() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-ellipsis
          (syntax-rules ()
            ((_) (quote (... ...)))))

        (make-ellipsis)
        "#,
    );

    assert!(result.is_ok());
    // (... ...) should produce just ...
    assert_eq!(result.unwrap().to_string(), "...");
}

/// Test ellipsis escape preserves pattern variable substitution
#[test]
fn test_ellipsis_escape_with_pattern_variable() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-esc-1
          (syntax-rules ()
            ((_ x) (quote (... (x ...))))))

        (elli-esc-1 foo)
        "#,
    );

    assert!(result.is_ok());
    // x should be substituted, but ... remains literal
    assert_eq!(result.unwrap().to_string(), "(foo ...)");
}

/// Test ellipsis escape with multiple pattern variables
#[test]
fn test_ellipsis_escape_multiple_vars() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-esc-2
          (syntax-rules ()
            ((_ x y) (quote (... (... x y))))))

        (elli-esc-2 bar baz)
        "#,
    );

    assert!(result.is_ok());
    // Both x and y should be substituted, ... remains literal
    assert_eq!(result.unwrap().to_string(), "(... bar baz)");
}

/// Test that ellipsis escape produces proper Symbol values
#[test]
fn test_ellipsis_escape_produces_symbol() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax make-ellipsis
          (syntax-rules ()
            ((_) (quote (... ...)))))

        (equal? (make-ellipsis) '...)
        "#,
    );

    assert!(result.is_ok());
    // Should be equal to '...
    assert_eq!(result.unwrap().to_string(), "#t");
}

/// Test ellipsis escape in list produces equal? results
#[test]
fn test_ellipsis_escape_equal_list() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define-syntax elli-with-var
          (syntax-rules ()
            ((_ x) (quote (... (x ...))))))

        (equal? (elli-with-var 100) '(100 ...))
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "#t");
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
    assert_eq!(result.unwrap().to_string(), "42");
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
    assert_eq!(result.unwrap().to_string(), "4");
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
    assert_eq!(result.unwrap().to_string(), "(1 2 3 4 5)");
}

// =============================================================================
// Literal Shadowing Tests
// =============================================================================
//
// R7RS Section 4.3.2: When a literal identifier in a macro pattern is shadowed
// by a local binding at the macro use site, it should NOT match as a literal.
// This allows user code to override literal keywords.

/// Test that shadowed literal `=>` in cond doesn't match the arrow clause
///
/// This is the classic R7RS hygiene test where `=>` is shadowed by a local
/// binding. The cond macro should NOT recognize the shadowed `=>` as its
/// literal arrow syntax.
#[test]
fn test_shadowed_literal_cond_arrow() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((=> #f))
          (cond (#t => 'ok)))
        "#,
    );

    assert!(result.is_ok());
    // Since => is shadowed, cond should treat it as a regular expression,
    // falling through to the (test result1 result2 ...) pattern.
    // The result should be 'ok, not an error from trying to call 'ok as a procedure.
    assert_eq!(result.unwrap().to_string(), "ok");
}

/// Test shadowed literal with else in cond
///
/// Similar to above, but testing the `else` literal.
#[test]
fn test_shadowed_literal_cond_else() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ((else #f))
          (cond (else 'matched-else)
                (#t 'fallback)))
        "#,
    );

    assert!(result.is_ok());
    // With `else` shadowed, the first clause should be (test result...) form
    // where test=#f, so it should fall through to the second clause
    assert_eq!(result.unwrap().to_string(), "fallback");
}

/// Test that non-shadowed literal still works normally
#[test]
fn test_non_shadowed_literal_cond_arrow() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (cond (#t => (lambda (x) 'got-true)))
        "#,
    );

    assert!(result.is_ok());
    // Normal => behavior: call the procedure with the test result
    assert_eq!(result.unwrap().to_string(), "got-true");
}

/// Test shadowed literal in nested scope
#[test]
fn test_shadowed_literal_nested_scope() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (define (test-shadow)
          (let ((=> #f))
            (cond (#t => 'shadowed-ok))))
        (test-shadow)
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "shadowed-ok");
}

// =============================================================================
// Internal Define Scoping in let-syntax Tests
// =============================================================================
//
// R7RS: Internal defines in a let-syntax body should create local bindings
// that don't escape to the outer scope.

/// Test that define inside let-syntax creates a local binding
///
/// This is the chibi test case where (define x 2) inside let-syntax
/// should NOT modify the outer x.
#[test]
fn test_let_syntax_internal_define_scoping() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define x 1)
          (let-syntax ()
            (define x 2)
            #f)
          x)
        "#,
    );

    assert!(result.is_ok());
    // The outer x should still be 1, not 2
    assert_eq!(result.unwrap().to_string(), "1");
}

/// Test multiple internal defines in let-syntax body
#[test]
fn test_let_syntax_multiple_internal_defines() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define a 1)
          (define b 2)
          (let-syntax ()
            (define a 10)
            (define b 20)
            (+ a b))  ; Should use local a=10, b=20
          (+ a b))    ; Should use outer a=1, b=2
        "#,
    );

    assert!(result.is_ok());
    // Outer a + b = 1 + 2 = 3
    assert_eq!(result.unwrap().to_string(), "3");
}

/// Test that define inside let-syntax can access outer variables
#[test]
fn test_let_syntax_internal_define_can_read_outer() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define x 10)
          (let-syntax ()
            (define y (+ x 5))  ; y = 10 + 5 = 15
            y))
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "15");
}

/// Test let-syntax with both macros and internal defines
#[test]
fn test_let_syntax_macro_and_internal_define() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define x 1)
          (let-syntax ((double (syntax-rules ()
                                 ((double e) (+ e e)))))
            (define x 5)
            (double x)))  ; Should use local x=5, result = 10
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "10");
}

/// Test letrec-syntax also has proper internal define scoping
#[test]
fn test_letrec_syntax_internal_define_scoping() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (define x 1)
          (letrec-syntax ()
            (define x 2)
            #f)
          x)
        "#,
    );

    assert!(result.is_ok());
    // The outer x should still be 1
    assert_eq!(result.unwrap().to_string(), "1");
}

/// Test define-syntax inside let-syntax body still works
#[test]
fn test_let_syntax_with_internal_define_syntax() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let ()
          (let-syntax ()
            (define-syntax triple
              (syntax-rules ()
                ((triple e) (+ e e e))))
            (triple 4)))
        "#,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), "12");
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
    assert_eq!(result.unwrap().to_string(), "hello");
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
///
/// This test is currently IGNORED because Patina doesn't correctly handle the
/// case where a substituted symbol happens to have the same name as a pattern
/// variable in a nested syntax-rules. Symbols from pattern variable substitution
/// need to carry scope information to distinguish them from new pattern variables.
///
/// See PRD/phase1/MACRO_GENERATING_MACRO_HYGIENE.md for detailed analysis and
/// implementation roadmap. This should be fixed AFTER DEFINE_SYNTAX_ELIMINATION.md
/// is complete.
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

    // Expected: 'x (the symbol x from the outer macro call)
    // Actual (broken): 1 (the inner pattern variable x bound to the argument)
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "x");
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
    assert_eq!(result.unwrap().to_string(), "30");
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
    assert_eq!(result.unwrap().to_string(), "115");
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
    assert_eq!(result.unwrap().to_string(), "25");
}

/// Test macro generating syntax-rules with literals from pattern variable
/// This exercises the fix for parse_literals_list accepting Identifier
#[test]
fn test_macro_with_pattern_var_in_literals() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // The literal 'k' comes from a pattern variable in the outer macro
    let result = interp.eval_program(
        r#"
        (let-syntax
            ((m (syntax-rules ()
                  ((m x) (let-syntax
                             ((n (syntax-rules (k)
                                   ((n x) 'bound)
                                   ((n y) 'free))))
                           (n z))))))
          (m k))
        "#,
    );

    // Note: This tests that 'k' in the literals list works even when it
    // comes from a pattern variable substitution (producing Identifier)
    // The actual result depends on hygiene semantics (bound vs free identifier)
    assert!(result.is_ok(), "Failed: {:?}", result);
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
    assert_eq!(result.unwrap().to_string(), "43");
}

// ============================================================================
// Literal Identifier Matching Tests (bound-identifier=? semantics)
// ============================================================================
//
// These tests verify R7RS 4.3.2 literal matching semantics:
// "An element in the input matches a literal identifier if and only if
//  it is an identifier and either both its occurrence in the macro
//  expression and its occurrence in the macro definition have the same
//  lexical binding, or the two identifiers are the same and both have
//  no lexical binding."

/// Test: Literal in nested macro matches same literal in template
///
/// When a macro generates another macro with a literal, and the template
/// uses that same literal, they should match (both refer to the same binding).
///
/// Verified against chibi-scheme and Gauche.
#[test]
fn test_nested_macro_literal_matching() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m ignored)
                           (let-syntax ((n (syntax-rules (k)
                                             ((n k) 'matched-k)
                                             ((n y) 'no-match))))
                             (n k))))))
          (m anything))
        "#,
    );

    // The literal `k` in (syntax-rules (k)...) matches the `k` in (n k)
    // because both come from the outer macro's template (same binding context).
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "matched-k");
}

/// Test: Literal shadowed by let AFTER macro definition should NOT match
///
/// When the input identifier is bound by a let AFTER the macro is defined,
/// it refers to a different binding than the pattern literal, so it should
/// not match.
///
/// Verified against chibi-scheme and Gauche.
#[test]
fn test_literal_shadowed_in_macro_body() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let-syntax ((n (syntax-rules (k)
                          ((n k) 'matched)
                          ((n y) 'no-match))))
          (let ((k 42))
            (n k)))
        "#,
    );

    // The `k` in (n k) is shadowed by the let binding, so it doesn't
    // refer to the same binding as the literal `k` in the macro pattern.
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "no-match");
}

/// Test: Literal shadowed by let in nested macro template should NOT match
///
/// Same as above, but with the shadowing happening inside a macro template.
///
/// Verified against chibi-scheme and Gauche.
#[test]
fn test_literal_shadowed_in_nested_macro_template() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m ignored)
                           (let-syntax ((n (syntax-rules (k)
                                             ((n k) 'matched-k)
                                             ((n y) 'no-match))))
                             (let ((k 99))
                               (n k)))))))
          (m anything))
        "#,
    );

    // The `k` in (n k) is shadowed by the let inside the template.
    assert!(result.is_ok(), "Failed: {:?}", result);
    assert_eq!(result.unwrap().to_string(), "no-match");
}
