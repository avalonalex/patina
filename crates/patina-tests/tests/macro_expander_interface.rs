//! Tests for the macro expander interface
//!
//! These tests demonstrate the clean TestExpander API for testing macros.
//! This is much simpler than the old approach that required manual parsing,
//! compiling, and expansion.

use patina_frontend::macro_expander::TestExpander;

#[test]
fn test_simple_when_macro() {
    let expander = TestExpander::from_definition(
        "when",
        r#"
        (syntax-rules ()
          ((when test body ...)
           (if test (begin body ...))))
        "#,
    )
    .expect("Failed to create expander");

    // Test basic expansion
    expander
        .assert_expands_to(
            "(when #t (display 1) (display 2))",
            "(if #t (begin (display 1) (display 2)))",
        )
        .expect("Expansion should match");

    // Test with single body expression
    expander
        .assert_expands_to("(when #f x)", "(if #f (begin x))")
        .expect("Single body expansion should match");
}

#[test]
fn test_unless_macro() {
    let expander = TestExpander::from_definition(
        "unless",
        r#"
        (syntax-rules ()
          ((unless test body ...)
           (if (not test) (begin body ...))))
        "#,
    )
    .expect("Failed to create expander");

    // Test that it expands - hygiene will rename 'not' and 'display'
    let result = expander
        .expand_to_string("(unless #f (display 1))")
        .expect("Should expand");

    // Verify structure: should have 'if' and 'begin'
    assert!(result.contains("if"));
    assert!(result.contains("begin"));
    assert!(result.contains("#f"));
    assert!(result.contains("1"));
}

#[test]
fn test_let_macro() {
    let expander = TestExpander::from_definition(
        "my-let",
        r#"
        (syntax-rules ()
          ((my-let ((var val) ...) body ...)
           ((lambda (var ...) body ...) val ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(my-let ((x 1) (y 2)) (+ x y))",
            "((lambda (x y) (+ x y)) 1 2)",
        )
        .expect("Let expansion should match");

    // Test with no bindings
    expander
        .assert_expands_to("(my-let () body)", "((lambda () body))")
        .expect("Empty let should match");
}

#[test]
fn test_and_macro() {
    let expander = TestExpander::from_definition(
        "my-and",
        r#"
        (syntax-rules ()
          ((my-and) #t)
          ((my-and test) test)
          ((my-and test1 test2 ...)
           (if test1 (my-and test2 ...) #f)))
        "#,
    )
    .expect("Failed to create expander");

    // Test empty and
    expander
        .assert_expands_to("(my-and)", "#t")
        .expect("Empty and should expand to #t");

    // Test single argument
    expander
        .assert_expands_to("(my-and x)", "x")
        .expect("Single argument should return itself");

    // Test multiple arguments
    expander
        .assert_expands_to("(my-and #t #f)", "(if #t (my-and #f) #f)")
        .expect("Two arguments should expand correctly");
}

#[test]
fn test_cond_macro() {
    let expander = TestExpander::from_definition(
        "my-cond",
        r#"
        (syntax-rules (else)
          ((my-cond (else result1 result2 ...))
           (begin result1 result2 ...))
          ((my-cond (test result1 result2 ...))
           (if test (begin result1 result2 ...)))
          ((my-cond (test result1 result2 ...) clause ...)
           (if test
               (begin result1 result2 ...)
               (my-cond clause ...))))
        "#,
    )
    .expect("Failed to create expander");

    // Test single clause
    expander
        .assert_expands_to("(my-cond (#t (display 1)))", "(if #t (begin (display 1)))")
        .expect("Single clause should match");

    // Test with else
    expander
        .assert_expands_to("(my-cond (else (display 1)))", "(begin (display 1))")
        .expect("Else clause should match");
}

#[test]
#[ignore] // TODO: Re-enable when nested ellipsis with no pattern variables is supported
fn test_nested_ellipsis_macro() {
    let expander = TestExpander::from_definition(
        "multi-list",
        r#"
        (syntax-rules ()
          ((multi-list (item ...) ...)
           (list (list item ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to("(multi-list (1 2) (3 4))", "(list (list 1 2) (list 3 4))")
        .expect("Nested ellipsis should expand correctly");
}

#[test]
fn test_hygiene_with_expand_to_string() {
    let expander = TestExpander::from_definition(
        "my-let",
        r#"
        (syntax-rules ()
          ((my-let x body)
           (let ((temp x)) body)))
        "#,
    )
    .expect("Failed to create expander");

    let result = expander
        .expand_to_string("(my-let 42 temp)")
        .expect("Expansion should succeed");

    // The result should contain 'let' and the gensym for 'temp'
    assert!(result.contains("let"));
    assert!(result.contains("42"));
    // The inner 'temp' should not be renamed because it came from the input
    assert!(result.contains("temp"));
}

#[test]
fn test_literal_matching() {
    let expander = TestExpander::from_definition(
        "my-case",
        r#"
        (syntax-rules (else)
          ((my-case (else result))
           result)
          ((my-case key (else result))
           result)
          ((my-case key (test result))
           (if (eqv? key 'test) result)))
        "#,
    )
    .expect("Failed to create expander");

    // Test literal else
    expander
        .assert_expands_to("(my-case (else 42))", "42")
        .expect("Else should match");

    // Test with key
    expander
        .assert_expands_to("(my-case x (else 42))", "42")
        .expect("Else with key should match");

    // Test pattern match - hygiene will rename eqv?
    let result = expander
        .expand_to_string("(my-case x (foo 42))")
        .expect("Should expand");
    assert!(result.contains("if"));
    assert!(result.contains("x"));
    assert!(result.contains("'foo"));
    assert!(result.contains("42"));
}

#[test]
fn test_recursive_macro() {
    let expander = TestExpander::from_definition(
        "countdown",
        r#"
        (syntax-rules ()
          ((countdown 0) (display "done"))
          ((countdown n)
           (begin
             (display n)
             (countdown (- n 1)))))
        "#,
    )
    .expect("Failed to create expander");

    // Base case - hygiene will rename display
    let result = expander
        .expand_to_string("(countdown 0)")
        .expect("Should expand");
    assert!(result.contains("\"done\""));

    // Recursive case
    let result = expander
        .expand_to_string("(countdown 3)")
        .expect("Should expand");
    assert!(result.contains("begin"));
    assert!(result.contains("3"));
    assert!(result.contains("countdown"));
}

#[test]
fn test_multiple_rules_macro() {
    let expander = TestExpander::from_definition(
        "maybe",
        r#"
        (syntax-rules ()
          ((maybe) #f)
          ((maybe x) x)
          ((maybe x y) (if x x y)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to("(maybe)", "#f")
        .expect("No args should give #f");

    expander
        .assert_expands_to("(maybe foo)", "foo")
        .expect("One arg should return itself");

    expander
        .assert_expands_to("(maybe test default)", "(if test test default)")
        .expect("Two args should use if");
}

#[test]
fn test_vector_patterns() {
    let expander = TestExpander::from_definition(
        "vec-swap",
        r#"
        (syntax-rules ()
          ((vec-swap #(a b))
           #(b a)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to("(vec-swap #(x y))", "#(y x)")
        .expect("Vector swap should work");
}

#[test]
fn test_dotted_tail_pattern() {
    let expander = TestExpander::from_definition(
        "cons-onto",
        r#"
        (syntax-rules ()
          ((cons-onto item . rest)
           (cons item 'rest)))
        "#,
    )
    .expect("Failed to create expander");

    // Hygiene will rename cons
    let result = expander
        .expand_to_string("(cons-onto 1 2 3)")
        .expect("Should expand");
    assert!(result.contains("1"));
    assert!(result.contains("'(2 3)"));
}

#[test]
#[ignore] // TODO: Re-enable when triple-nested ellipsis is fully supported
fn test_complex_nesting() {
    let expander = TestExpander::from_definition(
        "matrix",
        r#"
        (syntax-rules ()
          ((matrix ((item ...) ...) ...)
           (list (list (list item ...) ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(matrix ((1 2) (3 4)) ((5 6) (7 8)))",
            "(list (list (list 1 2) (list 3 4)) (list (list 5 6) (list 7 8)))",
        )
        .expect("Triple nested ellipsis should work");
}

// Comprehensive nested ellipsis tests

#[test]
fn test_flatten_pairs() {
    // Test 1: Basic nested ... - flatten nested lists
    let expander = TestExpander::from_definition(
        "flatten-pairs",
        r#"
        (syntax-rules ()
          ((flatten-pairs ((a b) ...))
           (a ... b ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to("(flatten-pairs ((1 2) (3 4) (5 6)))", "(1 3 5 2 4 6)")
        .expect("Should flatten pairs");
}

#[test]
#[ignore] // TODO: Re-enable when ... ... (ellipsis-ellipsis) Cartesian product is fully supported
fn test_cartesian_product() {
    // Test 2: ... ... (ellipsis-ellipsis) - Cartesian product pattern
    let expander = TestExpander::from_definition(
        "cartesian",
        r#"
        (syntax-rules ()
          ((cartesian (a ...) (b ...))
           ((list a b) ... ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(cartesian (1 2 3) (x y))",
            "((list 1 x) (list 1 y) (list 2 x) (list 2 y) (list 3 x) (list 3 y))",
        )
        .expect("Should create Cartesian product");
}

#[test]
#[ignore] // TODO: Re-enable when nested ellipsis with no variables at intermediate levels is supported
fn test_nested_map() {
    // Test 3: Nested ... with different depths
    let expander = TestExpander::from_definition(
        "nested-map",
        r#"
        (syntax-rules ()
          ((nested-map f ((x ...) ...))
           ((f x ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(nested-map list ((1 2 3) (4 5) (6 7 8 9)))",
            "((list 1 2 3) (list 4 5) (list 6 7 8 9))",
        )
        .expect("Should map over nested lists");
}

#[test]
#[ignore] // TODO: Re-enable when triple-nested ellipsis is fully supported
fn test_matrix_flatten() {
    // Test 4: Triple nesting - matrix-like structure
    let expander = TestExpander::from_definition(
        "matrix-flatten",
        r#"
        (syntax-rules ()
          ((matrix-flatten (((a ...) ...) ...))
           ((a ... ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(matrix-flatten (((1 2) (3 4)) ((5 6) (7 8))))",
            "((1 2 3 4) (5 6 7 8))",
        )
        .expect("Should flatten matrix structure");
}

#[test]
#[ignore] // TODO: Re-enable when ... ... (ellipsis-ellipsis) Cartesian product is fully supported
fn test_cross_sum() {
    // Test 5: ... ... with intermediate computation
    let expander = TestExpander::from_definition(
        "cross-sum",
        r#"
        (syntax-rules ()
          ((cross-sum (a ...) (b ...))
           ((+ a b) ... ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(cross-sum (1 2) (10 20 30))",
            "((+ 1 10) (+ 1 20) (+ 1 30) (+ 2 10) (+ 2 20) (+ 2 30))",
        )
        .expect("Should create cross product with +");
}

#[test]
fn test_interleave() {
    // Test 6: Interleaving with nested ...
    let expander = TestExpander::from_definition(
        "interleave",
        r#"
        (syntax-rules ()
          ((interleave ((a b) ...))
           (a ... b ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to("(interleave ((1 'x) (2 'y) (3 'z)))", "(1 2 3 'x 'y 'z)")
        .expect("Should interleave pairs");
}

#[test]
#[ignore] // TODO: Re-enable when ... ... (ellipsis-ellipsis) Cartesian product is fully supported
fn test_make_setters() {
    // Test 7: Nested ... with literals
    let expander = TestExpander::from_definition(
        "make-setters",
        r#"
        (syntax-rules ()
          ((make-setters (name field ...) ...)
           ((define (name val) (set! field val)) ... ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(make-setters (set-x! x) (set-y! y z))",
            "((define (set-x! val) (set! x val)) (define (set-y! val) (set! y val)) (define (set-y! val) (set! z val)))",
        )
        .expect("Should create setter definitions");
}

#[test]
#[ignore] // TODO: Re-enable when ... ... (ellipsis-ellipsis) Cartesian product is fully supported
fn test_pair_product() {
    // Test 8: Complex ... ... with pair structure
    let expander = TestExpander::from_definition(
        "pair-product",
        r#"
        (syntax-rules ()
          ((pair-product ((a . b) ...) ((c . d) ...))
           (((a . c) (b . d)) ... ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(pair-product ((1 . 2) (3 . 4)) ((x . y) (p . q)))",
            "(((1 . x) (2 . y)) ((1 . p) (2 . q)) ((3 . x) (4 . y)) ((3 . p) (4 . q)))",
        )
        .expect("Should create pair product");
}

#[test]
#[ignore] // TODO: Re-enable when nested ellipsis with no variables at intermediate levels is supported
fn test_prefix_all() {
    // Test 9: Asymmetric nesting
    let expander = TestExpander::from_definition(
        "prefix-all",
        r#"
        (syntax-rules ()
          ((prefix-all prefix (elem ...) ...)
           ((prefix elem ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(prefix-all list (1 2) (a b c) (x))",
            "((list 1 2) (list a b c) (list x))",
        )
        .expect("Should prefix all groups");
}

#[test]
#[ignore] // TODO: Re-enable when triple-nested ellipsis is fully supported
fn test_deep_nest() {
    // Test 10: Deep nesting with three levels
    let expander = TestExpander::from_definition(
        "deep-nest",
        r#"
        (syntax-rules ()
          ((deep-nest ((((x ...) ...) ...) ...))
           (((x ...) ...) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(deep-nest ((((1 2) (3)) ((4 5 6))) (((7) (8 9)))))",
            "(((1 2) (3)) ((4 5 6)) ((7) (8 9)))",
        )
        .expect("Should handle deep nesting");
}

#[test]
fn test_broadcast() {
    // Test 11: ... ... with single outer element
    let expander = TestExpander::from_definition(
        "broadcast",
        r#"
        (syntax-rules ()
          ((broadcast x (y ...))
           ((list x y) ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(broadcast 'a (1 2 3))",
            "((list 'a 1) (list 'a 2) (list 'a 3))",
        )
        .expect("Should broadcast single element");
}

#[test]
#[ignore] // TODO: Re-enable when ... ... (ellipsis-ellipsis) Cartesian product is fully supported
fn test_alternate() {
    // Test 12: Alternating pattern depths
    let expander = TestExpander::from_definition(
        "alternate",
        r#"
        (syntax-rules ()
          ((alternate (a ...) ((b c) ...))
           ((list a b c) ... ...)))
        "#,
    )
    .expect("Failed to create expander");

    expander
        .assert_expands_to(
            "(alternate (1 2) ((x y) (p q) (m n)))",
            "((list 1 x y) (list 1 p q) (list 1 m n) (list 2 x y) (list 2 p q) (list 2 m n))",
        )
        .expect("Should alternate pattern depths");
}
