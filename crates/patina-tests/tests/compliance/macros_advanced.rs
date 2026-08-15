//! Advanced R7RS Macro Tests
//!
//! Tests for well-known, non-trivial macros that exercise the full
//! capabilities of the hygienic macro system.

use crate::common::*;

// ============================================================================
// Classic Control Flow Macros
// ============================================================================

#[test]
fn test_my_when_macro() {
    assert_program_eval_to(
        r#"
        (define-syntax my-when
          (syntax-rules ()
            ((my-when test body ...)
             (if test (begin body ...)))))

        (define result 0)
        (my-when (> 5 3)
          (set! result 10)
          (set! result (+ result 5)))
        result
        "#,
        "15",
    );
}

#[test]
fn test_my_unless_macro() {
    assert_program_eval_to(
        r#"
        (define-syntax my-unless
          (syntax-rules ()
            ((my-unless test body ...)
             (if (not test) (begin body ...)))))

        (define result 0)
        (my-unless (< 5 3)
          (set! result 20))
        result
        "#,
        "20",
    );
}

#[test]
fn test_cond_else_macro() {
    // Simplified cond using macros (testing macro-generating patterns)
    assert_program_eval_to(
        r#"
        (define-syntax my-cond
          (syntax-rules (else)
            ((my-cond (else result))
             result)
            ((my-cond (test result))
             (if test result))
            ((my-cond (test result) clause ...)
             (if test result (my-cond clause ...)))))

        (my-cond
          ((< 5 3) 'less)
          ((> 5 3) 'greater)
          (else 'equal))
        "#,
        "greater",
    );
}

// ============================================================================
// Binding Macros
// ============================================================================

#[test]
fn test_let_macro_implementation() {
    // Implement let in terms of lambda (classic macro example)
    assert_program_eval_to(
        r#"
        (define-syntax my-let
          (syntax-rules ()
            ((my-let ((var val) ...) body ...)
             ((lambda (var ...) body ...) val ...))))

        (my-let ((x 10) (y 20))
          (+ x y))
        "#,
        "30",
    );
}

#[test]
fn test_named_let_for_recursion() {
    // Named let for recursion
    assert_program_eval_to(
        r#"
        (define-syntax named-let
          (syntax-rules ()
            ((named-let name ((var val) ...) body ...)
             (letrec ((name (lambda (var ...) body ...)))
               (name val ...)))))

        (named-let loop ((n 5) (acc 1))
          (if (= n 0)
              acc
              (loop (- n 1) (* acc n))))
        "#,
        "120",
    );
}

// ============================================================================
// Mutation Macros
// ============================================================================

#[test]
fn test_push_macro() {
    // Classic push! macro (like Lisp)
    assert_program_eval_to(
        r#"
        (define-syntax push!
          (syntax-rules ()
            ((push! item lst)
             (set! lst (cons item lst)))))

        (define my-list '(2 3))
        (push! 1 my-list)
        my-list
        "#,
        "(1 2 3)",
    );
}

#[test]
fn test_inc_macro() {
    // Increment macro
    assert_program_eval_to(
        r#"
        (define-syntax inc!
          (syntax-rules ()
            ((inc! var)
             (set! var (+ var 1)))
            ((inc! var delta)
             (set! var (+ var delta)))))

        (define x 10)
        (inc! x)
        (inc! x 5)
        x
        "#,
        "16",
    );
}

#[test]
fn test_swap_macro_with_hygiene() {
    // The classic hygiene test - swap without capture
    assert_program_eval_to(
        r#"
        (define-syntax swap!
          (syntax-rules ()
            ((swap! a b)
             (let ((temp a))
               (set! a b)
               (set! b temp)))))

        (define temp 999)
        (define x 1)
        (define y 2)
        (swap! x y)
        (list x y temp)
        "#,
        "(2 1 999)",
    );
}

// ============================================================================
// Logic and Utility Macros
// ============================================================================

#[test]
fn test_and_macro() {
    // Short-circuit and
    assert_program_eval_to(
        r#"
        (define-syntax my-and
          (syntax-rules ()
            ((my-and) #t)
            ((my-and test) test)
            ((my-and test1 test2 ...)
             (if test1 (my-and test2 ...) #f))))

        (list
          (my-and (> 5 3) (< 2 4))
          (my-and (> 5 3) (> 2 4))
          (my-and))
        "#,
        "(#t #f #t)",
    );
}

#[test]
fn test_or_macro() {
    // Short-circuit or
    assert_program_eval_to(
        r#"
        (define-syntax my-or
          (syntax-rules ()
            ((my-or) #f)
            ((my-or test) test)
            ((my-or test1 test2 ...)
             (let ((temp test1))
               (if temp temp (my-or test2 ...))))))

        (list
          (my-or (< 5 3) (> 2 4) (= 1 1))
          (my-or (< 5 3) (> 2 4))
          (my-or))
        "#,
        "(#t #f #f)",
    );
}

#[test]
fn test_begin0_macro() {
    // Return first value but evaluate all
    assert_program_eval_to(
        r#"
        (define-syntax begin0
          (syntax-rules ()
            ((begin0 first rest ...)
             (let ((temp first))
               rest ...
               temp))))

        (define x 1)
        (begin0
          (+ x 10)
          (set! x 20)
          (set! x 30))
        "#,
        "11", // Returns (+ 1 10) even though x changed
    );
}

// ============================================================================
// List Processing Macros
// ============================================================================

#[test]
fn test_dotimes_macro() {
    // Loop n times
    assert_program_eval_to(
        r#"
        (define-syntax dotimes
          (syntax-rules ()
            ((dotimes (var count) body ...)
             (letrec ((loop (lambda (var)
                              (if (< var count)
                                  (begin
                                    body ...
                                    (loop (+ var 1)))))))
               (loop 0)))))

        (define sum 0)
        (dotimes (i 5)
          (set! sum (+ sum i)))
        sum
        "#,
        "10", // 0+1+2+3+4
    );
}

#[test]
fn test_while_macro() {
    // While loop
    assert_program_eval_to(
        r#"
        (define-syntax while
          (syntax-rules ()
            ((while test body ...)
             (letrec ((loop (lambda ()
                              (if test
                                  (begin
                                    body ...
                                    (loop))))))
               (loop)))))

        (define n 5)
        (define result 1)
        (while (> n 0)
          (set! result (* result n))
          (set! n (- n 1)))
        result
        "#,
        "120",
    );
}

// ============================================================================
// Pattern Matching and Destructuring
// ============================================================================

#[test]
fn test_let_values_simple() {
    // Simplified let-values (R7RS has this built-in but testing macro version)
    assert_program_eval_to(
        r#"
        (define-syntax simple-let-values
          (syntax-rules ()
            ((simple-let-values ((var ...)) expr body ...)
             (call-with-values
               (lambda () expr)
               (lambda (var ...) body ...)))))

        (simple-let-values ((a b))
          (values 10 20)
          (+ a b))
        "#,
        "30",
    );
}

// ============================================================================
// Ellipsis Tests (Complex Patterns)
// ============================================================================

#[test]
fn test_list_star_macro() {
    // list* - like cons but with multiple args
    assert_program_eval_to(
        r#"
        (define-syntax list*
          (syntax-rules ()
            ((list* last)
             last)
            ((list* first rest ...)
             (cons first (list* rest ...)))))

        (list* 1 2 3 '(4 5))
        "#,
        "(1 2 3 4 5)",
    );
}

#[test]
fn test_nested_ellipsis() {
    // Multiple items with nested ellipsis
    // This requires nested ellipsis support: (expr ...) ...
    // This IS part of R7RS but rarely used in practice
    assert_program_eval_to(
        r#"
        (define-syntax multi-begin
          (syntax-rules ()
            ((multi-begin (expr ...) ...)
             (begin expr ... ...))))

        (define x 0)
        (multi-begin
          ((set! x 1) (set! x (+ x 1)))
          ((set! x (+ x 10)) (set! x (+ x 20))))
        x
        "#,
        "32", // 0 → 1 → 2 → 12 → 32
    );
}

#[test]
fn test_repeat_expr_macro() {
    // Repeat an expression multiple times
    assert_program_eval_to(
        r#"
        (define-syntax repeat
          (syntax-rules ()
            ((repeat (times ...) expr)
             (begin times ... expr))))

        (define x 0)
        (repeat (#f #f #f) (set! x (+ x 1)))
        x
        "#,
        "1", // expr evaluated once, dummy times ignored
    );
}

// ============================================================================
// Hygiene Stress Tests
// ============================================================================

#[test]
fn test_triple_nested_macros() {
    // Three levels of macro nesting
    assert_program_eval_to(
        r#"
        (define-syntax my-when
          (syntax-rules ()
            ((my-when test body ...)
             (if test (begin body ...)))))

        (define-syntax my-unless
          (syntax-rules ()
            ((my-unless test body ...)
             (if (not test) (begin body ...)))))

        (define-syntax safe-div
          (syntax-rules ()
            ((safe-div a b default)
             (my-unless (= b 0)
               (my-when (> a 0)
                 (/ a b))))))

        (safe-div 10 2 0)
        "#,
        "5",
    );
}

#[test]
fn test_hygiene_with_multiple_temps() {
    // Multiple temporary variables - all should be renamed
    assert_program_eval_to(
        r#"
        (define-syntax complex-swap
          (syntax-rules ()
            ((complex-swap a b c)
             (let ((temp1 a) (temp2 b) (temp3 c))
               (set! a temp3)
               (set! b temp1)
               (set! c temp2)))))

        (define temp1 100)
        (define temp2 200)
        (define temp3 300)
        (define x 1)
        (define y 2)
        (define z 3)
        (complex-swap x y z)
        (list x y z temp1 temp2 temp3)
        "#,
        "(3 1 2 100 200 300)", // x,y,z rotated; temps unchanged
    );
}

#[test]
fn test_recursive_macro() {
    // Macro that expands into itself (like list construction)
    assert_program_eval_to(
        r#"
        (define-syntax build-list
          (syntax-rules ()
            ((build-list)
             '())
            ((build-list x)
             (cons x '()))
            ((build-list x y ...)
             (cons x (build-list y ...)))))

        (build-list 1 2 3 4 5)
        "#,
        "(1 2 3 4 5)",
    );
}

// ============================================================================
// Practical Macros
// ============================================================================

#[test]
fn test_assert_macro() {
    // Simple assertion macro with quoted symbols
    // Tests that hygiene doesn't rename inside quote forms
    assert_program_eval_to(
        r#"
        (define-syntax assert
          (syntax-rules ()
            ((assert test)
             (if (not test)
                 'assertion-failed
                 'ok))))

        (list
          (assert (= 2 2))
          (assert (> 5 3)))
        "#,
        "(ok ok)",
    );
}

#[test]
fn test_comment_macro() {
    // Comment macro - discards all arguments
    assert_program_eval_to(
        r#"
        (define-syntax comment
          (syntax-rules ()
            ((comment anything ...)
             (begin))))

        (define x 10)
        (comment
          (set! x 999)
          (set! x 'should-not-execute)
          this is all ignored)
        x
        "#,
        "10",
    );
}

#[test]
fn test_trace_macro() {
    // Macro that evaluates and returns (simplified tracing)
    assert_program_eval_to(
        r#"
        (define-syntax trace
          (syntax-rules ()
            ((trace expr)
             (let ((result expr))
               result))))

        (trace (+ 1 2 3))
        "#,
        "6",
    );
}

// ============================================================================
// Vector Handling in Macros (Regression Test)
// ============================================================================

#[test]
fn test_vector_literal_in_macro_pattern_variable() {
    // This tests that literal vectors passed through macro pattern variables
    // are properly handled and compare equal to quoted vectors.
    // Regression test: Previously, vectors from pattern variables would have
    // their symbols marked with scopes, causing comparison failures.
    assert_program_eval_to(
        r#"
        (define-syntax test-vector
          (syntax-rules ()
            ((test-vector expected actual)
             (equal? expected actual))))

        (test-vector #(a b c) '#(a b c))
        "#,
        "#t",
    );

    // Also test with the test macro pattern (actual chibi test case)
    assert_program_eval_to(
        r#"
        (define-syntax my-test
          (syntax-rules ()
            ((my-test expected expr)
             (equal? expected expr))))

        (my-test #(a b c) (quote #(a b c)))
        "#,
        "#t",
    );
}

#[test]
fn test_nested_macro_with_quoted_symbols() {
    // This tests that when a macro defines another macro,
    // pattern variable substitution produces symbols that don't
    // get re-interpreted as pattern variables in the inner macro.
    // This is the fix for the chibi test case:
    //   (define-syntax foo (syntax-rules () ((foo bar y) (define-syntax bar ...))))
    assert_program_eval_to(
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
        "x",
    );
}

#[test]
fn test_quasiquote_in_macro_with_unquote_splicing() {
    // Test that quasiquote with unquote-splicing works correctly inside macros.
    // Regression test: After macro expansion, unquote-splicing becomes an Identifier
    // rather than a Symbol, and the quasiquote evaluator must handle both.
    assert_program_eval_to(
        r#"
        (define-syntax test-qq
          (syntax-rules ()
            ((test-qq expected expr)
             (equal? expected expr))))

        (test-qq '(a 3 4 5 6 b) `(a ,(+ 1 2) ,@(map abs '(4 -5 6)) b))
        "#,
        "#t",
    );
}

#[test]
fn test_quasiquote_in_macro_with_list_comparison() {
    // Test that quoted lists from pattern variables compare equal to quasiquote results.
    // Regression test: Symbols in quoted data from pattern variables were becoming
    // Identifiers with empty scopes, failing comparison with quasiquote results.
    assert_program_eval_to(
        r#"
        (define-syntax test-list
          (syntax-rules ()
            ((test-list expected expr)
             (equal? expected expr))))

        (test-list '(list 3 4) `(list ,(+ 1 2) 4))
        "#,
        "#t",
    );
}

// ============================================================================
// Edge Cases and Complex Patterns
// ============================================================================

#[test]
fn test_macro_with_literals() {
    // Using literal identifiers (like '=>' as a literal keyword)
    assert_program_eval_to(
        r#"
        (define-syntax arrow-if
          (syntax-rules (=>)
            ((arrow-if test => then-expr)
             (if test then-expr #f))))

        (arrow-if (> 5 3) => 42)
        "#,
        "42",
    );
}

#[test]
fn test_empty_ellipsis() {
    // Ellipsis matching zero items
    assert_program_eval_to(
        r#"
        (define-syntax maybe-begin
          (syntax-rules ()
            ((maybe-begin body ...)
             (begin body ...))))

        (list
          (maybe-begin 42)
          (maybe-begin))
        "#,
        "(42 #<unspecified>)",
    );
}

#[test]
fn test_pattern_with_nested_structure() {
    // Pattern matching nested list structure
    assert_program_eval_to(
        r#"
        (define-syntax let-pair
          (syntax-rules ()
            ((let-pair ((a b) pair-expr) body ...)
             (let ((temp pair-expr))
               (let ((a (car temp))
                     (b (car (cdr temp))))
                 body ...)))))

        (let-pair ((x y) '(10 20))
          (+ x y))
        "#,
        "30",
    );
}

// ============================================================================
// SRFI-46 and R7RS Ellipsis Extensions
// ============================================================================

#[test]
fn test_srfi46_ellipsis_as_literal() {
    // SRFI-46 / R7RS 4.3.2: "Literals have priority over ellipsis"
    // When ... is both the ellipsis AND in the literals list,
    // it should be treated as a literal pattern element.
    assert_program_eval_to(
        r#"
        (let ()
          (define-syntax elli-lit-1
            (syntax-rules ... (...)
              ((_ x)
               '(x ...))))
          (elli-lit-1 100))
        "#,
        "(100 ...)",
    );
}

#[test]
fn test_srfi46_custom_ellipsis() {
    // SRFI-46: Custom ellipsis identifier
    // Using ::: instead of ... as the ellipsis
    assert_program_eval_to(
        r#"
        (let ()
          (define-syntax my-list
            (syntax-rules ::: ()
              ((_ x :::)
               '(x :::))))
          (my-list 1 2 3))
        "#,
        "(1 2 3)",
    );
}

#[test]
fn test_srfi46_ellipsis_as_literal_with_custom_ellipsis() {
    // SRFI-46: Custom ellipsis with ... as literal
    // Using ::: as ellipsis allows ... to be used as a literal
    assert_program_eval_to(
        r#"
        (let ()
          (define-syntax with-dots
            (syntax-rules ::: (...)
              ((_ x ... y)
               '(x y))))
          (with-dots a ... b))
        "#,
        "(a b)",
    );
}

// ============================================================================
// Bound-identifier=? Hygiene Tests (Nested Macro Definitions)
// ============================================================================

#[test]
fn test_bound_identifier_equality_in_nested_macros() {
    // The inner literals list `(k)` is written in the outer *template*, so its
    // `k` is introduced and carries the outer expansion's scopes. The inner
    // pattern's `x` is substituted from the use site and carries none. They are
    // spelled alike but are different identifiers, so `x` is NOT a literal --
    // it is a pattern variable, it matches `z`, and the answer is
    // `bound-identifier=?`.
    //
    // Verified against Chez Scheme. Note this passes because literal membership
    // compares identity; it is not the substituted `k` acting as a literal that
    // matches anything, which would instead answer `free-identifier=?` here and
    // would break the `(chibi parse)` guard -- see
    // `test_chibi_parse_new_symbol_guard`.
    assert_program_eval_to(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m x)
                           (let-syntax ((n (syntax-rules (k)
                                             ((n x) 'bound-identifier=?)
                                             ((n y) 'free-identifier=?))))
                             (n z))))))
          (m k))
        "#,
        "bound-identifier=?",
    );
}

#[test]
fn test_bound_identifier_with_different_input() {
    // Same shape as above with a different name, confirming the outcome follows
    // from introduced-vs-substituted identity rather than from anything about
    // the name `k`. Verified against Chez Scheme.
    assert_program_eval_to(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m x)
                           (let-syntax ((n (syntax-rules (foo)
                                             ((n x) 'bound-identifier=?)
                                             ((n y) 'free-identifier=?))))
                             (n z))))))
          (m foo))
        "#,
        "bound-identifier=?",
    );
}

#[test]
fn test_nested_macro_literal_not_matching_different_symbol() {
    // When the literal in the inner macro's pattern is NOT substituted
    // (i.e., it's a fresh symbol with scopes), it should only match
    // identifiers with the same name AND scopes.
    //
    // In this case, the literal 'k' in the inner macro's (syntax-rules (k) ...)
    // is a fresh symbol, not substituted from outer. So (n z) should NOT
    // match the first rule and should fall through to the second.
    assert_program_eval_to(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m ignored)
                           (let-syntax ((n (syntax-rules (k)
                                             ((n k) 'matched-k)
                                             ((n y) 'no-match))))
                             (n z))))))
          (m anything))
        "#,
        "no-match",
    );
}

#[test]
fn test_nested_macro_literal_matching_same_symbol() {
    // When the input to the inner macro IS the literal symbol,
    // it should match (according to chibi/Gauche).
    //
    // This tests R7RS bound-identifier=? semantics for literal matching:
    // - The literal `k` in (syntax-rules (k) ...) gets definition scopes {S1, S2}
    // - The input `k` in (n k) has scopes {S1, S2, S3} (after flip)
    // - Since {S1, S2} ⊆ {S1, S2, S3}, they match as the same identifier
    //
    // This matches chibi-scheme and Gauche behavior.
    assert_program_eval_to(
        r#"
        (let-syntax ((m (syntax-rules ()
                          ((m ignored)
                           (let-syntax ((n (syntax-rules (k)
                                             ((n k) 'matched-k)
                                             ((n y) 'no-match))))
                             (n k))))))
          (m anything))
        "#,
        "matched-k",
    );
}

// ============================================================================
// Ellipsis Escape in Nested Macro Definitions
// ============================================================================

#[test]
fn test_nested_macro_with_ellipsis_escape() {
    // Test that (... template) properly escapes ellipsis in nested macro definitions.
    // The inner macro should be able to use ellipsis normally after escaping.
    // This test case is from the chibi r7rs test suite (listify example).
    assert_program_eval_to(
        r#"
        (let ()
          (define-syntax listify
            (syntax-rules ()
              ((listify e)
               (define-syntax apply-to-list
                 (syntax-rules ()
                   ((apply-to-list arg (... ...))
                    (e (list arg (... ...)))))))))
          (listify car)
          (apply-to-list 1 2 3))
        "#,
        "1",
    );
}

#[test]
fn test_nested_macro_ellipsis_escape_with_pattern_var() {
    // Test ellipsis escape where the outer macro substitutes a value
    // into the inner macro's template.
    assert_program_eval_to(
        r#"
        (let ()
          (define-syntax make-wrapper
            (syntax-rules ()
              ((make-wrapper wrapper-name tag)
               (define-syntax wrapper-name
                 (syntax-rules ()
                   ((wrapper-name item (... ...))
                    '(tag item (... ...))))))))
          (make-wrapper wrap-with-x x)
          (wrap-with-x 1 2 3))
        "#,
        "(x 1 2 3)",
    );
}

// ============================================================================
// Template-introduced identifiers across recursive expansions
// ============================================================================

/// A recursive macro that introduces the *same* template identifier on each
/// expansion must produce a *distinct* binding each time.
///
/// The accumulator below spells every parameter `a`, but each one is
/// introduced by a different expansion and so carries that expansion's scope.
/// Rejecting them as duplicates broke SRFI 156's `is`/`isnt`, whose
/// `extract-placeholders` builds a lambda exactly this way.
///
/// Verified against Gauche and Chez Scheme, which both return `(10 20)`.
#[test]
fn test_recursive_macro_introduces_distinct_params() {
    assert_program_eval_to(
        r#"
        (define-syntax gen
          (syntax-rules ()
            ((_ () (args ...)) (lambda (args ...) (list args ...)))
            ((_ (x . rest) (args ...)) (gen rest (args ... a)))))
        ((gen (1 2) ()) 10 20)
        "#,
        "(10 20)",
    );
}

/// The same rule applies to the rest parameter of an improper formals list.
#[test]
fn test_recursive_macro_distinct_params_with_rest() {
    assert_program_eval_to(
        r#"
        (define-syntax gen2
          (syntax-rules ()
            ((_ () (args ...)) (lambda (args ... . r) (list args ... r)))
            ((_ (x . rest) (args ...)) (gen2 rest (args ... a)))))
        ((gen2 (1 2) ()) 10 20 30 40)
        "#,
        "(10 20 (30 40))",
    );
}

/// Hand-written duplicates share a scope set, so they are still an error.
#[test]
fn test_genuine_duplicate_params_still_rejected() {
    assert_program_eval_error("(lambda (q q) q)");
    assert_program_eval_error("(lambda (q . q) q)");
}

/// SRFI 156's reference implementation, reduced to the shape that failed:
/// each `_` placeholder becomes a fresh lambda parameter named `arg`.
#[test]
fn test_srfi_156_placeholder_shape() {
    assert_program_eval_to(
        r#"
        (define-syntax infix/postfix
          (syntax-rules ()
            ((infix/postfix x somewhat?) (somewhat? x))
            ((infix/postfix left related-to? right) (related-to? left right))))
        (define-syntax extract-placeholders
          (syntax-rules (_)
            ((extract-placeholders final () () body)
             (final (infix/postfix . body)))
            ((extract-placeholders final () args body)
             (lambda args (final (infix/postfix . body))))
            ((extract-placeholders final (_ op . rest) (args ...) (body ...))
             (extract-placeholders final rest (args ... arg) (body ... arg op)))
            ((extract-placeholders final (arg op . rest) args (body ...))
             (extract-placeholders final rest args (body ... arg op)))
            ((extract-placeholders final (_) (args ...) (body ...))
             (extract-placeholders final () (args ... arg) (body ... arg)))
            ((extract-placeholders final (arg) args (body ...))
             (extract-placeholders final () args (body ... arg)))))
        (define-syntax identity-syntax
          (syntax-rules () ((identity-syntax form) form)))
        (define-syntax is
          (syntax-rules ()
            ((is . something)
             (extract-placeholders identity-syntax something () ()))))
        (list ((is _ < _) 1 2) ((is _ < _) 2 1))
        "#,
        "(#t #f)",
    );
}

// ============================================================================
// Identifier identity in a macro that writes another macro
//
// Every case below is checked against Chez Scheme. The common thread: an
// identifier's classification inside an inner `syntax-rules` depends on its
// *identity* -- name plus scopes -- and never on its name alone. An identifier
// substituted from the outer use site and one introduced by the outer template
// can be spelled the same and still be different identifiers.
// ============================================================================

/// A substituted identifier that is not in the inner literals list is an
/// ordinary pattern variable, so it binds whatever the inner macro is called
/// with.
#[test]
fn test_substituted_identifier_is_a_pattern_variable() {
    assert_program_eval_to(
        r#"
        (define-syntax gen
          (syntax-rules ()
            ((_ nm v) (define-syntax nm (syntax-rules () ((_ v) (list v v)))))))
        (gen m q)
        (m 5)
        "#,
        "(5 5)",
    );
}

/// A substituted identifier that *is* in the inner literals list stays a
/// literal, so it matches only itself.
#[test]
fn test_substituted_identifier_in_literals_stays_a_literal() {
    assert_program_eval_to(
        r#"
        (define-syntax gen
          (syntax-rules ()
            ((_ nm k)
             (define-syntax nm
               (syntax-rules (k) ((_ k) 'got-key) ((_ x) 'other))))))
        (gen m key)
        (list (m key) (m zzz))
        "#,
        "(got-key other)",
    );
}

/// The `new-symbol?` guard from `(chibi parse)`'s `grammar-bind`, reduced.
///
/// `syntax-rules` has no way to compare two identifiers, so the guard builds an
/// inner macro whose literals list holds the names bound so far and calls it
/// with an identifier that matches nothing. If the name is a literal, rule 1
/// cannot match and rule 2 answers "already bound"; otherwise the name is a
/// pattern variable, rule 1 matches, and the answer is "new".
///
/// This is what blocked chibi-parse and edn: a literal that matched *any*
/// identifier made the guard answer "new" every time, so the same grammar
/// nonterminal got a variable more than once.
#[test]
fn test_chibi_parse_new_symbol_guard() {
    assert_program_eval_to(
        r#"
        (define-syntax probe-test
          (syntax-rules ()
            ((_ name (lit ...))
             (let-syntax ((probe (syntax-rules (lit ...)
                                   ((probe name sk fk) sk)
                                   ((probe _ sk fk) fk))))
               (probe random-symbol-to-match 'new 'already)))))
        (list (probe-test space (space term))
              (probe-test other (space term)))
        "#,
        "(already new)",
    );
}

/// The macro-keyword position of a rule is positional, so a substituted macro
/// name still matches whatever the call spells.
#[test]
fn test_substituted_macro_name_still_matches() {
    assert_program_eval_to(
        r#"
        (define-syntax make-wrapper
          (syntax-rules ()
            ((_ wrapper-name tag)
             (define-syntax wrapper-name
               (syntax-rules ()
                 ((wrapper-name item (... ...)) '(tag item (... ...))))))))
        (make-wrapper wrap-with-x x)
        (wrap-with-x 1 2 3)
        "#,
        "(x 1 2 3)",
    );
}

/// A substituted identifier is still a duplicate of *itself*.
///
/// edn passes a whole expression where `(chibi parse)`'s `grammar-bind` expects
/// a name, and that expression mentions `ch` twice; substituting it into the
/// guard's pattern asks for two pattern variables with one identity. Chez
/// rejects this with the same "duplicate pattern variable ch", so the package
/// depends on chibi's leniency rather than on anything portable.
#[test]
fn test_repeated_substituted_identifier_in_pattern_is_an_error() {
    assert_program_eval_error(
        r#"
        (define-syntax gen
          (syntax-rules ()
            ((_ nm blob) (define-syntax nm (syntax-rules () ((_ blob) 'matched))))))
        (gen m (f ch ch))
        (m (f 1 2))
        "#,
    );
}

/// A literal matches an input identifier when both are unbound and share a
/// name, even though one is introduced and the other substituted.
///
/// R7RS 4.3.2 gives literal *matching* `free-identifier=?` semantics — same
/// binding, **or both unbound with the same name**. That is a different
/// question from literal *membership* (is this pattern identifier a literal at
/// all?), which compares identity; see
/// `test_substituted_identifier_in_literals_stays_a_literal`. Conflating the
/// two is what made the introduced literal `k` below unable to match the
/// substituted `k`.
///
/// Verified against Chez Scheme and Gauche, which both return `(lit notlit)`.
#[test]
fn test_literal_matches_when_both_unbound() {
    assert_program_eval_to(
        r#"
        (define-syntax m
          (syntax-rules ()
            ((_ e) (let-syntax ((n (syntax-rules (k) ((n k) 'lit) ((n x) 'notlit))))
                     (n e)))))
        (list (m k) (m other))
        "#,
        "(lit notlit)",
    );
}
