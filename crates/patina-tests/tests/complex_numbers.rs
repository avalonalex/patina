use patina_interpreter::TreeWalkInterpreter;

fn ev(interp: &TreeWalkInterpreter, code: &str) -> String {
    interp.display_tagged(interp.eval_str(code).unwrap())
}

#[test]
fn test_parse_complex_rectangular() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Basic rectangular notation
    assert_eq!(ev(&interp, "3+4i"), "3+4i");
    assert_eq!(ev(&interp, "5-2i"), "5-2i");
    assert_eq!(ev(&interp, "-3+7i"), "-3+7i");
    assert_eq!(ev(&interp, "-1-1i"), "-1-i");
}

#[test]
fn test_parse_complex_pure_imaginary() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Pure imaginary
    assert_eq!(ev(&interp, "+i"), "+i");
    assert_eq!(ev(&interp, "-i"), "-i");
    assert_eq!(ev(&interp, "+5i"), "+5i");
    assert_eq!(ev(&interp, "-3i"), "-3i");
}

#[test]
fn test_parse_complex_shorthand() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Shorthand for imaginary part = 1
    assert_eq!(ev(&interp, "3+i"), "3+i");
    assert_eq!(ev(&interp, "5-i"), "5-i");
}

#[test]
fn test_parse_complex_with_floats() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Inexact complex numbers
    assert_eq!(ev(&interp, "3.5+2.7i"), "3.5+2.7i");
    // 1.0-0.5i preserves exactness: real part is inexact 1.0, imag is inexact -0.5
    assert_eq!(ev(&interp, "1.0-0.5i"), "1.0-0.5i");
}

#[test]
fn test_complex_addition() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // (a+bi) + (c+di) = (a+c) + (b+d)i
    assert_eq!(ev(&interp, "(+ 1+2i 3+4i)"), "4+6i");
    assert_eq!(ev(&interp, "(+ 5+3i -2-1i)"), "3+2i");
    assert_eq!(ev(&interp, "(+ 1+1i 2+2i 3+3i)"), "6+6i");
}

#[test]
fn test_complex_addition_with_real() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Complex + Real
    assert_eq!(ev(&interp, "(+ 3+4i 5)"), "8+4i");
    assert_eq!(ev(&interp, "(+ 10 2+3i)"), "12+3i");
}

#[test]
fn test_complex_subtraction() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // (a+bi) - (c+di) = (a-c) + (b-d)i
    assert_eq!(ev(&interp, "(- 5+7i 2+3i)"), "3+4i");
    assert_eq!(ev(&interp, "(- 10+5i 10+5i)"), "0");
}

#[test]
fn test_complex_multiplication() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
    assert_eq!(ev(&interp, "(* 2+3i 1-1i)"), "5+i");
    assert_eq!(ev(&interp, "(* 1+i 1-i)"), "2");
    assert_eq!(ev(&interp, "(* +i +i)"), "-1");
}

#[test]
fn test_complex_multiplication_with_real() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Complex * Real
    assert_eq!(ev(&interp, "(* 2+3i 2)"), "4+6i");
    assert_eq!(ev(&interp, "(* 3 1+2i)"), "3+6i");
}

#[test]
fn test_complex_negation() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Unary minus
    assert_eq!(ev(&interp, "(- 3+4i)"), "-3-4i");
    assert_eq!(ev(&interp, "(- -5+2i)"), "5-2i");
    assert_eq!(ev(&interp, "(- +i)"), "-i");
}

#[test]
fn test_complex_zero() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Operations that result in zero
    assert_eq!(ev(&interp, "(+ 3+4i -3-4i)"), "0");
    assert_eq!(ev(&interp, "(- 5+7i 5+7i)"), "0");
}

#[test]
fn test_parse_polar() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Polar notation: r@theta
    // Note: Results are approximate due to floating point
    let result = ev(&interp, "1@0");
    assert!(result.starts_with("1") || result.starts_with("0.99"));

    // 1@(π/2) ≈ i
    let result = ev(&interp, "1@1.5708");
    assert!(result.contains("i"));
}

#[test]
fn test_complex_polynomial_identity() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Verify polynomial identity: (z - w)(z + w) = z² - w²
    interp.eval_str("(define z 3+4i)").unwrap();
    interp.eval_str("(define w 1+2i)").unwrap();

    let lhs = ev(&interp, "(* (- z w) (+ z w))");
    let rhs = ev(&interp, "(- (* z z) (* w w))");

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_conjugate_property() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Complex conjugate: (a + bi)(a - bi) = a² + b²
    // For 3+4i and 3-4i: should give 9 + 16 = 25
    let result = ev(&interp, "(* 3+4i 3-4i)");
    assert_eq!(result, "25");
}

#[test]
fn test_i_squared() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Fundamental property: i² = -1
    assert_eq!(ev(&interp, "(* +i +i)"), "-1");
    assert_eq!(ev(&interp, "(* -i -i)"), "-1");
}

#[test]
fn test_complex_fibonacci() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define complex Fibonacci
    interp
        .eval_str(
            "(define (complex-fib n a b)
               (if (= n 0)
                   a
                   (if (= n 1)
                       b
                       (complex-fib (- n 1) b (+ a b)))))",
        )
        .unwrap();

    // Test with imaginary seeds: should give pure imaginary results
    assert_eq!(ev(&interp, "(complex-fib 0 0+0i 0+1i)"), "0");
    assert_eq!(ev(&interp, "(complex-fib 1 0+0i 0+1i)"), "+i");
    assert_eq!(ev(&interp, "(complex-fib 3 0+0i 0+1i)"), "+2i");
    assert_eq!(ev(&interp, "(complex-fib 5 0+0i 0+1i)"), "+5i");

    // Test with mixed complex seeds
    assert_eq!(ev(&interp, "(complex-fib 2 1+1i 1-1i)"), "2");
    assert_eq!(ev(&interp, "(complex-fib 5 1+1i 1-1i)"), "8-2i");
}

#[test]
fn test_powers_of_i() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // i¹ = i
    assert_eq!(ev(&interp, "+i"), "+i");

    // i² = -1
    assert_eq!(ev(&interp, "(* +i +i)"), "-1");

    // i³ = i² * i = -i
    assert_eq!(ev(&interp, "(* (* +i +i) +i)"), "-i");

    // i⁴ = (i²)² = (-1)² = 1
    assert_eq!(ev(&interp, "(* (* +i +i) (* +i +i))"), "1");
}

#[test]
fn test_complex_iteration() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Julia set style iteration: z_n+1 = z_n² + c
    interp
        .eval_str(
            "(define (julia-iterate z c iterations)
               (if (= iterations 0)
                   z
                   (julia-iterate (+ (* z z) c) c (- iterations 1))))",
        )
        .unwrap();

    // With c = 0, this is just repeated squaring
    // 2² = 4
    assert_eq!(ev(&interp, "(julia-iterate 2+0i 0+0i 1)"), "4");

    // 2² = 4, then 4² = 16
    assert_eq!(ev(&interp, "(julia-iterate 2+0i 0+0i 2)"), "16");

    // Pure imaginary iteration: (0+i)² = -1
    assert_eq!(ev(&interp, "(julia-iterate 0+1i 0+0i 1)"), "-1");
}

#[test]
fn test_cube_roots_of_unity_sum() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // The three cube roots of unity sum to 0
    // 1 + e^(2πi/3) + e^(4πi/3) = 0
    // Approximately: 1 + (-0.5+0.866i) + (-0.5-0.866i)
    let result = ev(&interp, "(+ 1 (+ -0.5+0.866i -0.5-0.866i))");

    // Should be very close to 0 (allowing for floating point error)
    assert!(result.starts_with("0") || result.starts_with("-0.0"));
}

#[test]
fn test_demoivre_special_cases() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // (1+i)² = 1 + 2i - 1 = 2i
    assert_eq!(ev(&interp, "(* 1+1i 1+1i)"), "+2i");

    // (1-i)² = 1 - 2i - 1 = -2i
    assert_eq!(ev(&interp, "(* 1-1i 1-1i)"), "-2i");

    // (1+i)(1-i) = 1 - i² = 1 - (-1) = 2
    assert_eq!(ev(&interp, "(* 1+1i 1-1i)"), "2");
}

#[test]
fn test_complex_distributive_law() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Test: a(b + c) = ab + ac with complex numbers
    interp.eval_str("(define a 2+3i)").unwrap();
    interp.eval_str("(define b 1+1i)").unwrap();
    interp.eval_str("(define c 4-2i)").unwrap();

    let lhs = ev(&interp, "(* a (+ b c))");
    let rhs = ev(&interp, "(+ (* a b) (* a c))");

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_associative_law() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Test: (ab)c = a(bc) with complex numbers
    interp.eval_str("(define a 1+2i)").unwrap();
    interp.eval_str("(define b 3-1i)").unwrap();
    interp.eval_str("(define c 2+2i)").unwrap();

    let lhs = ev(&interp, "(* (* a b) c)");
    let rhs = ev(&interp, "(* a (* b c))");

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_with_exact_rationals() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Complex numbers with rational components
    // Note: currently converts to float, but parsing works
    let result = ev(&interp, "1/2+3/4i");
    assert!(result.contains("0.5") || result.contains("1/2"));
    assert!(result.contains("0.75") || result.contains("3/4"));
}

#[test]
fn test_nested_complex_operations() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Deeply nested complex arithmetic
    let result = ev(&interp, "(* (+ 1+i 2+2i) (- 3+3i 1+1i))");
    // (1+i + 2+2i) * (3+3i - 1-1i) = (3+3i) * (2+2i) = (6-6) + (6+6)i = 12i
    assert_eq!(result, "+12i");
}

#[test]
fn test_complex_zero_handling() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // 0 + complex = complex
    assert_eq!(ev(&interp, "(+ 0 3+4i)"), "3+4i");

    // complex + 0 = complex
    assert_eq!(ev(&interp, "(+ 3+4i 0)"), "3+4i");

    // 0 * complex = 0
    assert_eq!(ev(&interp, "(* 0 3+4i)"), "0");

    // complex - complex = 0
    assert_eq!(ev(&interp, "(- 3+4i 3+4i)"), "0");
}
