use patina::Interpreter;

fn main() {
    let interp = Interpreter::new();

    println!("=== Lambda Implementation Test ===\n");

    // Test 1: Simple lambda
    println!("1. Simple lambda:");
    let result = interp.eval_str("((lambda (x) (* x 2)) 21)").unwrap();
    println!("   ((lambda (x) (* x 2)) 21) => {}", result);

    // Test 2: Multiple parameters
    println!("\n2. Multiple parameters:");
    let result = interp.eval_str("((lambda (x y) (+ x y)) 10 32)").unwrap();
    println!("   ((lambda (x y) (+ x y)) 10 32) => {}", result);

    // Test 3: Closure
    println!("\n3. Closure (captures environment):");
    interp
        .eval_str("(define make-multiplier (lambda (n) (lambda (x) (* x n))))")
        .unwrap();
    interp
        .eval_str("(define times3 (make-multiplier 3))")
        .unwrap();
    let result = interp.eval_str("(times3 14)").unwrap();
    println!("   (times3 14) => {}", result);

    // Test 4: Nested closures
    println!("\n4. Nested closures:");
    interp
        .eval_str("(define make-adder (lambda (a) (lambda (b) (+ a b))))")
        .unwrap();
    let result = interp.eval_str("((make-adder 10) 32)").unwrap();
    println!("   ((make-adder 10) 32) => {}", result);

    // Test 5: Variadic lambda
    println!("\n5. Variadic lambda (all args):");
    let result = interp.eval_str("((lambda args args) 1 2 3 4 5)").unwrap();
    println!("   ((lambda args args) 1 2 3 4 5) => {}", result);

    // Test 6: Variadic with fixed params
    println!("\n6. Variadic with fixed params:");
    let result = interp
        .eval_str("((lambda (x y . rest) rest) 1 2 3 4 5)")
        .unwrap();
    println!("   ((lambda (x y . rest) rest) 1 2 3 4 5) => {}", result);

    // Test 7: Higher-order function
    println!("\n7. Higher-order function:");
    interp
        .eval_str("(define twice (lambda (f) (lambda (x) (f (f x)))))")
        .unwrap();
    interp
        .eval_str("(define add1 (lambda (n) (+ n 1)))")
        .unwrap();
    let result = interp.eval_str("((twice add1) 10)").unwrap();
    println!("   ((twice add1) 10) => {}", result);

    println!("\n=== All lambda tests passed! ===");
}
