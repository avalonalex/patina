use patina::Interpreter;

#[test]
fn test_parse_complex_rectangular() {
    let interp = Interpreter::new();

    // Basic rectangular notation
    assert_eq!(interp.eval_str("3+4i").unwrap().to_string(), "3+4i");
    assert_eq!(interp.eval_str("5-2i").unwrap().to_string(), "5-2i");
    assert_eq!(interp.eval_str("-3+7i").unwrap().to_string(), "-3+7i");
    assert_eq!(interp.eval_str("-1-1i").unwrap().to_string(), "-1-i");
}

#[test]
fn test_parse_complex_pure_imaginary() {
    let interp = Interpreter::new();

    // Pure imaginary
    assert_eq!(interp.eval_str("+i").unwrap().to_string(), "+i");
    assert_eq!(interp.eval_str("-i").unwrap().to_string(), "-i");
    assert_eq!(interp.eval_str("+5i").unwrap().to_string(), "+5i");
    assert_eq!(interp.eval_str("-3i").unwrap().to_string(), "-3i");
}

#[test]
fn test_parse_complex_shorthand() {
    let interp = Interpreter::new();

    // Shorthand for imaginary part = 1
    assert_eq!(interp.eval_str("3+i").unwrap().to_string(), "3+i");
    assert_eq!(interp.eval_str("5-i").unwrap().to_string(), "5-i");
}

#[test]
fn test_parse_complex_with_floats() {
    let interp = Interpreter::new();

    // Inexact complex numbers
    assert_eq!(interp.eval_str("3.5+2.7i").unwrap().to_string(), "3.5+2.7i");
    assert_eq!(interp.eval_str("1.0-0.5i").unwrap().to_string(), "1-0.5i");
}

#[test]
fn test_complex_addition() {
    let interp = Interpreter::new();

    // (a+bi) + (c+di) = (a+c) + (b+d)i
    assert_eq!(
        interp.eval_str("(+ 1+2i 3+4i)").unwrap().to_string(),
        "4+6i"
    );
    assert_eq!(
        interp.eval_str("(+ 5+3i -2-1i)").unwrap().to_string(),
        "3+2i"
    );
    assert_eq!(
        interp.eval_str("(+ 1+1i 2+2i 3+3i)").unwrap().to_string(),
        "6+6i"
    );
}

#[test]
fn test_complex_addition_with_real() {
    let interp = Interpreter::new();

    // Complex + Real
    assert_eq!(interp.eval_str("(+ 3+4i 5)").unwrap().to_string(), "8+4i");
    assert_eq!(interp.eval_str("(+ 10 2+3i)").unwrap().to_string(), "12+3i");
}

#[test]
fn test_complex_subtraction() {
    let interp = Interpreter::new();

    // (a+bi) - (c+di) = (a-c) + (b-d)i
    assert_eq!(
        interp.eval_str("(- 5+7i 2+3i)").unwrap().to_string(),
        "3+4i"
    );
    assert_eq!(interp.eval_str("(- 10+5i 10+5i)").unwrap().to_string(), "0");
}

#[test]
fn test_complex_multiplication() {
    let interp = Interpreter::new();

    // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
    assert_eq!(interp.eval_str("(* 2+3i 1-1i)").unwrap().to_string(), "5+i");
    assert_eq!(interp.eval_str("(* 1+i 1-i)").unwrap().to_string(), "2");
    assert_eq!(interp.eval_str("(* +i +i)").unwrap().to_string(), "-1");
}

#[test]
fn test_complex_multiplication_with_real() {
    let interp = Interpreter::new();

    // Complex * Real
    assert_eq!(interp.eval_str("(* 2+3i 2)").unwrap().to_string(), "4+6i");
    assert_eq!(interp.eval_str("(* 3 1+2i)").unwrap().to_string(), "3+6i");
}

#[test]
fn test_complex_negation() {
    let interp = Interpreter::new();

    // Unary minus
    assert_eq!(interp.eval_str("(- 3+4i)").unwrap().to_string(), "-3-4i");
    assert_eq!(interp.eval_str("(- -5+2i)").unwrap().to_string(), "5-2i");
    assert_eq!(interp.eval_str("(- +i)").unwrap().to_string(), "-i");
}

#[test]
fn test_complex_zero() {
    let interp = Interpreter::new();

    // Operations that result in zero
    assert_eq!(interp.eval_str("(+ 3+4i -3-4i)").unwrap().to_string(), "0");
    assert_eq!(interp.eval_str("(- 5+7i 5+7i)").unwrap().to_string(), "0");
}

#[test]
fn test_parse_polar() {
    let interp = Interpreter::new();

    // Polar notation: r@theta
    // Note: Results are approximate due to floating point
    let result = interp.eval_str("1@0").unwrap().to_string();
    assert!(result.starts_with("1") || result.starts_with("0.99"));

    // 1@(π/2) ≈ i
    let result = interp.eval_str("1@1.5708").unwrap().to_string();
    assert!(result.contains("i"));
}

#[test]
fn test_complex_polynomial_identity() {
    let interp = Interpreter::new();

    // Verify polynomial identity: (z - w)(z + w) = z² - w²
    interp.eval_str("(define z 3+4i)").unwrap();
    interp.eval_str("(define w 1+2i)").unwrap();

    let lhs = interp.eval_str("(* (- z w) (+ z w))").unwrap().to_string();
    let rhs = interp.eval_str("(- (* z z) (* w w))").unwrap().to_string();

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_conjugate_property() {
    let interp = Interpreter::new();

    // Complex conjugate: (a + bi)(a - bi) = a² + b²
    // For 3+4i and 3-4i: should give 9 + 16 = 25
    let result = interp.eval_str("(* 3+4i 3-4i)").unwrap().to_string();
    assert_eq!(result, "25");
}

#[test]
fn test_i_squared() {
    let interp = Interpreter::new();

    // Fundamental property: i² = -1
    assert_eq!(interp.eval_str("(* +i +i)").unwrap().to_string(), "-1");
    assert_eq!(interp.eval_str("(* -i -i)").unwrap().to_string(), "-1");
}

#[test]
fn test_complex_fibonacci() {
    let interp = Interpreter::new();

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
    assert_eq!(
        interp
            .eval_str("(complex-fib 0 0+0i 0+1i)")
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        interp
            .eval_str("(complex-fib 1 0+0i 0+1i)")
            .unwrap()
            .to_string(),
        "+i"
    );
    assert_eq!(
        interp
            .eval_str("(complex-fib 3 0+0i 0+1i)")
            .unwrap()
            .to_string(),
        "+2i"
    );
    assert_eq!(
        interp
            .eval_str("(complex-fib 5 0+0i 0+1i)")
            .unwrap()
            .to_string(),
        "+5i"
    );

    // Test with mixed complex seeds
    assert_eq!(
        interp
            .eval_str("(complex-fib 2 1+1i 1-1i)")
            .unwrap()
            .to_string(),
        "2"
    );
    assert_eq!(
        interp
            .eval_str("(complex-fib 5 1+1i 1-1i)")
            .unwrap()
            .to_string(),
        "8-2i"
    );
}

#[test]
fn test_powers_of_i() {
    let interp = Interpreter::new();

    // i¹ = i
    assert_eq!(interp.eval_str("+i").unwrap().to_string(), "+i");

    // i² = -1
    assert_eq!(interp.eval_str("(* +i +i)").unwrap().to_string(), "-1");

    // i³ = i² * i = -i
    assert_eq!(
        interp.eval_str("(* (* +i +i) +i)").unwrap().to_string(),
        "-i"
    );

    // i⁴ = (i²)² = (-1)² = 1
    assert_eq!(
        interp
            .eval_str("(* (* +i +i) (* +i +i))")
            .unwrap()
            .to_string(),
        "1"
    );
}

#[test]
fn test_complex_iteration() {
    let interp = Interpreter::new();

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
    assert_eq!(
        interp
            .eval_str("(julia-iterate 2+0i 0+0i 1)")
            .unwrap()
            .to_string(),
        "4"
    );

    // 2² = 4, then 4² = 16
    assert_eq!(
        interp
            .eval_str("(julia-iterate 2+0i 0+0i 2)")
            .unwrap()
            .to_string(),
        "16"
    );

    // Pure imaginary iteration: (0+i)² = -1
    assert_eq!(
        interp
            .eval_str("(julia-iterate 0+1i 0+0i 1)")
            .unwrap()
            .to_string(),
        "-1"
    );
}

#[test]
fn test_cube_roots_of_unity_sum() {
    let interp = Interpreter::new();

    // The three cube roots of unity sum to 0
    // 1 + e^(2πi/3) + e^(4πi/3) = 0
    // Approximately: 1 + (-0.5+0.866i) + (-0.5-0.866i)
    let result = interp
        .eval_str("(+ 1 (+ -0.5+0.866i -0.5-0.866i))")
        .unwrap()
        .to_string();

    // Should be very close to 0 (allowing for floating point error)
    assert!(result.starts_with("0") || result.starts_with("-0.0"));
}

#[test]
fn test_demoivre_special_cases() {
    let interp = Interpreter::new();

    // (1+i)² = 1 + 2i - 1 = 2i
    assert_eq!(interp.eval_str("(* 1+1i 1+1i)").unwrap().to_string(), "+2i");

    // (1-i)² = 1 - 2i - 1 = -2i
    assert_eq!(interp.eval_str("(* 1-1i 1-1i)").unwrap().to_string(), "-2i");

    // (1+i)(1-i) = 1 - i² = 1 - (-1) = 2
    assert_eq!(interp.eval_str("(* 1+1i 1-1i)").unwrap().to_string(), "2");
}

#[test]
fn test_complex_distributive_law() {
    let interp = Interpreter::new();

    // Test: a(b + c) = ab + ac with complex numbers
    interp.eval_str("(define a 2+3i)").unwrap();
    interp.eval_str("(define b 1+1i)").unwrap();
    interp.eval_str("(define c 4-2i)").unwrap();

    let lhs = interp.eval_str("(* a (+ b c))").unwrap().to_string();
    let rhs = interp.eval_str("(+ (* a b) (* a c))").unwrap().to_string();

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_associative_law() {
    let interp = Interpreter::new();

    // Test: (ab)c = a(bc) with complex numbers
    interp.eval_str("(define a 1+2i)").unwrap();
    interp.eval_str("(define b 3-1i)").unwrap();
    interp.eval_str("(define c 2+2i)").unwrap();

    let lhs = interp.eval_str("(* (* a b) c)").unwrap().to_string();
    let rhs = interp.eval_str("(* a (* b c))").unwrap().to_string();

    assert_eq!(lhs, rhs);
}

#[test]
fn test_complex_with_exact_rationals() {
    let interp = Interpreter::new();

    // Complex numbers with rational components
    // Note: currently converts to float, but parsing works
    let result = interp.eval_str("1/2+3/4i").unwrap().to_string();
    assert!(result.contains("0.5") || result.contains("1/2"));
    assert!(result.contains("0.75") || result.contains("3/4"));
}

#[test]
fn test_nested_complex_operations() {
    let interp = Interpreter::new();

    // Deeply nested complex arithmetic
    let result = interp
        .eval_str("(* (+ 1+i 2+2i) (- 3+3i 1+1i))")
        .unwrap()
        .to_string();
    // (1+i + 2+2i) * (3+3i - 1-1i) = (3+3i) * (2+2i) = (6-6) + (6+6)i = 12i
    assert_eq!(result, "+12i");
}

#[test]
fn test_complex_zero_handling() {
    let interp = Interpreter::new();

    // 0 + complex = complex
    assert_eq!(interp.eval_str("(+ 0 3+4i)").unwrap().to_string(), "3+4i");

    // complex + 0 = complex
    assert_eq!(interp.eval_str("(+ 3+4i 0)").unwrap().to_string(), "3+4i");

    // 0 * complex = 0
    assert_eq!(interp.eval_str("(* 0 3+4i)").unwrap().to_string(), "0");

    // complex - complex = 0
    assert_eq!(interp.eval_str("(- 3+4i 3+4i)").unwrap().to_string(), "0");
}
