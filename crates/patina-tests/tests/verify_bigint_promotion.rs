//! Test to verify BigInt promotion happens correctly
//!
//! This test demonstrates and verifies the overflow detection and
//! automatic promotion from i64 to BigInt.

use patina_interpreter::{TreeWalkInterpreter, Value};

#[test]
fn test_overflow_promotion_step_by_step() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Step 1: Start with a small integer (stays as i64)
    let result = interp.eval_str("42").unwrap();
    assert!(matches!(result, Value::Integer(42)));
    println!("✅ Small integer stays as i64: {:?}", result);

    // Step 2: i64::MAX is still i64
    let result = interp.eval_str("9223372036854775807").unwrap();
    assert!(matches!(result, Value::Integer(9223372036854775807)));
    println!("✅ i64::MAX stays as i64: {:?}", result);

    // Step 3: i64::MAX + 1 MUST promote to BigInt
    let result = interp.eval_str("(+ 9223372036854775807 1)").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ Overflow detected! Promoted to BigInteger: {}", n);
            assert_eq!(n.to_string(), "9223372036854775808");
        }
        Value::Integer(_) => {
            panic!("❌ FAIL: Should have promoted to BigInteger but stayed as Integer!");
        }
        other => {
            panic!("❌ FAIL: Unexpected type: {:?}", other);
        }
    }

    // Step 4: Once promoted, stays as BigInt
    let result = interp.eval_str("(+ (+ 9223372036854775807 1) 1)").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ Stays as BigInteger after promotion: {}", n);
            assert_eq!(n.to_string(), "9223372036854775809");
        }
        _ => panic!("Should still be BigInteger"),
    }

    // Step 5: BigInt can go way beyond i64
    let result = interp.eval_str("(* 1000000000000 1000000000000)").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ Large multiplication promotes to BigInteger: {}", n);
            assert_eq!(n.to_string(), "1000000000000000000000000");
        }
        _ => panic!("Large multiplication should promote to BigInteger"),
    }
}

#[test]
fn test_promotion_in_factorial() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Define factorial function
    interp
        .eval_str(
            r#"
        (define (factorial n)
          (if (= n 0)
              1
              (* n (factorial (- n 1)))))
    "#,
        )
        .unwrap();

    // Small factorial stays as i64
    let result = interp.eval_str("(factorial 10)").unwrap();
    match &result {
        Value::Integer(n) => {
            println!("✅ factorial(10) = {} (fits in i64)", n);
            assert_eq!(*n, 3628800);
        }
        _ => panic!("Small factorial should be i64"),
    }

    // Large factorial promotes to BigInt
    let result = interp.eval_str("(factorial 25)").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ factorial(25) = {} (promoted to BigInt)", n);
            assert_eq!(n.to_string(), "15511210043330985984000000");
        }
        Value::Integer(n) => {
            panic!("❌ FAIL: factorial(25) = {} stayed as i64 (WRONG!)", n);
        }
        _ => panic!("factorial(25) should be BigInteger"),
    }
}

#[test]
fn test_promotion_boundaries() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    println!("\n=== Testing i64 Boundaries ===");
    println!("i64::MAX = 9223372036854775807");
    println!("i64::MIN = -9223372036854775808");

    // Test positive overflow
    let result = interp.eval_str("(+ 9223372036854775807 1)").unwrap();
    assert!(matches!(result, Value::BigInteger(_)));
    println!("✅ i64::MAX + 1 promotes to BigInt");

    // Test negative overflow
    let result = interp.eval_str("(- -9223372036854775808 1)").unwrap();
    assert!(matches!(result, Value::BigInteger(_)));
    println!("✅ i64::MIN - 1 promotes to BigInt");

    // Test multiplication overflow
    let result = interp.eval_str("(* 10000000000 10000000000)").unwrap();
    assert!(matches!(result, Value::BigInteger(_)));
    println!("✅ Large multiplication promotes to BigInt");

    // Test that small operations stay as i64
    let result = interp.eval_str("(+ 1 2 3 4 5)").unwrap();
    assert!(matches!(result, Value::Integer(15)));
    println!("✅ Small operations stay as i64");
}

#[test]
fn test_mixed_operations() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    println!("\n=== Testing Mixed i64/BigInt Operations ===");

    // BigInt + i64 = BigInt
    let result = interp
        .eval_str("(+ (+ 9223372036854775807 1) 100)")
        .unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ BigInt + i64 = BigInt: {}", n);
            assert_eq!(n.to_string(), "9223372036854775908");
        }
        _ => panic!("Should be BigInteger"),
    }

    // i64 + BigInt = BigInt
    let result = interp
        .eval_str("(+ 100 (+ 9223372036854775807 1))")
        .unwrap();
    assert!(matches!(result, Value::BigInteger(_)));
    println!("✅ i64 + BigInt = BigInt");

    // Large literal (>i64::MAX) gets parsed as BigInt
    // 10000000000000000000 > i64::MAX (9223372036854775807)
    let result = interp.eval_str("10000000000000000000").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ Large literal parsed as BigInt: {}", n);
            assert_eq!(n.to_string(), "10000000000000000000");
        }
        _ => panic!("Large literal should be BigInteger, got {:?}", result),
    }

    // Adding BigInt literals produces BigInt
    let result = interp
        .eval_str("(+ 10000000000000000000 10000000000000000000)")
        .unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ BigInt + BigInt = BigInt: {}", n);
            assert_eq!(n.to_string(), "20000000000000000000");
        }
        _ => panic!("BigInt arithmetic should stay BigInt, got {:?}", result),
    }
}

// TODO: Remove #[cfg_attr] once tail call optimization is implemented (see issue #XX)
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn test_fibonacci_demonstrates_promotion() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    println!("\n=== Fibonacci Demonstrates Gradual Promotion ===");

    // Define iterative fibonacci
    interp
        .eval_str(
            r#"
        (define (fib n)
          (define (fib-iter a b count)
            (if (= count 0)
                a
                (let-values (((next-a next-b) (values b (+ a b))))
                  (fib-iter next-a next-b (- count 1)))))
          (fib-iter 0 1 n))
    "#,
        )
        .unwrap();

    // Small fibonacci stays as i64
    let result = interp.eval_str("(fib 50)").unwrap();
    println!("fib(50) = {:?}", result);
    // fib(50) = 12586269025 - still fits in i64

    // Large fibonacci promotes to BigInt
    let result = interp.eval_str("(fib 100)").unwrap();
    match &result {
        Value::BigInteger(n) => {
            println!("✅ fib(100) = {} (promoted to BigInt)", n);
            assert_eq!(n.to_string(), "354224848179261915075");
        }
        Value::Integer(n) => {
            panic!("❌ fib(100) = {} should have promoted to BigInt!", n);
        }
        _ => panic!("Unexpected type"),
    }

    // The promotion happens somewhere between fib(50) and fib(100)
    // Let's find exactly where...
    println!("\n=== Finding Promotion Point ===");

    for i in [90, 91, 92, 93, 94] {
        let result = interp.eval_str(&format!("(fib {})", i)).unwrap();
        match result {
            Value::Integer(n) => println!("fib({}) = {} (still i64)", i, n),
            Value::BigInteger(n) => println!("fib({}) = {} (PROMOTED to BigInt!)", i, n),
            _ => {}
        }
    }
}
