//! Test to verify BigInt promotion happens correctly
//!
//! This test demonstrates and verifies the overflow detection and
//! automatic promotion from i64 to BigInt.
//!
//! Note: With TaggedValue storage, integers are stored as fixnums (61-bit)
//! which means numbers larger than FIXNUM_MAX (~10^18) are automatically
//! promoted to BigInt even before arithmetic operations.

use patina_core::TaggedValue;
use patina_interpreter::TreeWalkInterpreter;

/// Evaluate code and return the TaggedValue result
fn eval(interp: &TreeWalkInterpreter, code: &str) -> TaggedValue {
    interp.eval_str(code).unwrap()
}

/// Check if a TaggedValue is a BigInt using the heap
fn is_bigint(interp: &TreeWalkInterpreter, tv: TaggedValue) -> bool {
    let heap = interp.evaluator().global_env.heap();
    heap.borrow().is_bigint(tv)
}

/// Get the BigInt string representation if it is one
fn bigint_str(interp: &TreeWalkInterpreter, tv: TaggedValue) -> Option<String> {
    let heap = interp.evaluator().global_env.heap();
    heap.borrow().get_bigint(tv).map(|n| n.to_string())
}

#[test]
fn test_overflow_promotion_step_by_step() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Step 1: Start with a small integer (stays as fixnum)
    let result = eval(&interp, "42");
    assert_eq!(result.as_fixnum(), Some(42));
    println!(
        "Small integer stays as fixnum: {}",
        interp.display_tagged(result)
    );

    // Step 2: Numbers larger than FIXNUM_MAX are automatically promoted to BigInt
    // with TaggedValue storage. FIXNUM_MAX is (1 << 60) - 1 = 1152921504606846975
    // i64::MAX is 9223372036854775807, which exceeds FIXNUM_MAX
    let result = eval(&interp, "9223372036854775807");
    assert!(
        is_bigint(&interp, result),
        "Expected BigInteger for i64::MAX with TaggedValue storage, got {}",
        interp.display_tagged(result)
    );
    println!(
        "i64::MAX promoted to BigInt (exceeds fixnum range): {}",
        interp.display_tagged(result)
    );

    // Step 3: i64::MAX + 1 MUST promote to BigInt
    let result = eval(&interp, "(+ 9223372036854775807 1)");
    let s = bigint_str(&interp, result);
    assert!(s.is_some(), "FAIL: Should have promoted to BigInteger!");
    let s = s.unwrap();
    println!("Overflow detected! Promoted to BigInteger: {}", s);
    assert_eq!(s, "9223372036854775808");

    // Step 4: Once promoted, stays as BigInt
    let result = eval(&interp, "(+ (+ 9223372036854775807 1) 1)");
    let s = bigint_str(&interp, result).expect("Should still be BigInteger");
    println!("Stays as BigInteger after promotion: {}", s);
    assert_eq!(s, "9223372036854775809");

    // Step 5: BigInt can go way beyond i64
    let result = eval(&interp, "(* 1000000000000 1000000000000)");
    let s = bigint_str(&interp, result).expect("Large multiplication should promote to BigInteger");
    println!("Large multiplication promotes to BigInteger: {}", s);
    assert_eq!(s, "1000000000000000000000000");
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

    // Small factorial stays as fixnum
    let result = eval(&interp, "(factorial 10)");
    let n = result
        .as_fixnum()
        .expect("Small factorial should be fixnum");
    println!("factorial(10) = {} (fits in fixnum)", n);
    assert_eq!(n, 3628800);

    // Large factorial promotes to BigInt
    let result = eval(&interp, "(factorial 25)");
    if let Some(n) = result.as_fixnum() {
        panic!("FAIL: factorial(25) = {} stayed as fixnum (WRONG!)", n);
    }
    let s = bigint_str(&interp, result).expect("factorial(25) should be BigInteger");
    println!("factorial(25) = {} (promoted to BigInt)", s);
    assert_eq!(s, "15511210043330985984000000");
}

#[test]
fn test_promotion_boundaries() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    println!("\n=== Testing i64 Boundaries ===");
    println!("i64::MAX = 9223372036854775807");
    println!("i64::MIN = -9223372036854775808");

    // Test positive overflow
    let result = eval(&interp, "(+ 9223372036854775807 1)");
    assert!(is_bigint(&interp, result));
    println!("i64::MAX + 1 promotes to BigInt");

    // Test negative overflow
    let result = eval(&interp, "(- -9223372036854775808 1)");
    assert!(is_bigint(&interp, result));
    println!("i64::MIN - 1 promotes to BigInt");

    // Test multiplication overflow
    let result = eval(&interp, "(* 10000000000 10000000000)");
    assert!(is_bigint(&interp, result));
    println!("Large multiplication promotes to BigInt");

    // Test that small operations stay as fixnum
    let result = eval(&interp, "(+ 1 2 3 4 5)");
    assert_eq!(result.as_fixnum(), Some(15));
    println!("Small operations stay as fixnum");
}

#[test]
fn test_mixed_operations() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    println!("\n=== Testing Mixed fixnum/BigInt Operations ===");

    // BigInt + fixnum = BigInt
    let result = eval(&interp, "(+ (+ 9223372036854775807 1) 100)");
    let s = bigint_str(&interp, result).expect("Should be BigInteger");
    println!("BigInt + fixnum = BigInt: {}", s);
    assert_eq!(s, "9223372036854775908");

    // fixnum + BigInt = BigInt
    let result = eval(&interp, "(+ 100 (+ 9223372036854775807 1))");
    assert!(is_bigint(&interp, result));
    println!("fixnum + BigInt = BigInt");

    // Large literal (>i64::MAX) gets parsed as BigInt
    // 10000000000000000000 > i64::MAX (9223372036854775807)
    let result = eval(&interp, "10000000000000000000");
    let s = bigint_str(&interp, result).unwrap_or_else(|| {
        panic!(
            "Large literal should be BigInteger, got {}",
            interp.display_tagged(result)
        )
    });
    println!("Large literal parsed as BigInt: {}", s);
    assert_eq!(s, "10000000000000000000");

    // Adding BigInt literals produces BigInt
    let result = eval(&interp, "(+ 10000000000000000000 10000000000000000000)");
    let s = bigint_str(&interp, result).unwrap_or_else(|| {
        panic!(
            "BigInt arithmetic should stay BigInt, got {}",
            interp.display_tagged(result)
        )
    });
    println!("BigInt + BigInt = BigInt: {}", s);
    assert_eq!(s, "20000000000000000000");
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

    // Small fibonacci stays as fixnum
    let result = eval(&interp, "(fib 50)");
    println!("fib(50) = {}", interp.display_tagged(result));
    // fib(50) = 12586269025 - still fits in fixnum

    // Large fibonacci promotes to BigInt
    let result = eval(&interp, "(fib 100)");
    if let Some(n) = result.as_fixnum() {
        panic!("fib(100) = {} should have promoted to BigInt!", n);
    }
    let s = bigint_str(&interp, result).expect("fib(100) should be BigInteger");
    println!("fib(100) = {} (promoted to BigInt)", s);
    assert_eq!(s, "354224848179261915075");

    // The promotion happens somewhere between fib(50) and fib(100)
    // Let's find exactly where...
    println!("\n=== Finding Promotion Point ===");

    for i in [90, 91, 92, 93, 94] {
        let result = eval(&interp, &format!("(fib {})", i));
        if let Some(n) = result.as_fixnum() {
            println!("fib({}) = {} (still fixnum)", i, n);
        } else if let Some(s) = bigint_str(&interp, result) {
            println!("fib({}) = {} (PROMOTED to BigInt!)", i, s);
        }
    }
}
