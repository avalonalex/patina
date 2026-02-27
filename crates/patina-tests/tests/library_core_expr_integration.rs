//! Test how library loading interacts with CoreExpr pipeline
//!
//! This test explores why enabling CoreExpr in backend.rs breaks chibi tests.

use patina_interpreter::Interpreter;
use patina_tree_walker::TreeWalker;

#[test]
fn test_simple_expression_flow() {
    // Trace exactly what happens for a simple expression
    let backend = TreeWalker::new();
    let interp = Interpreter::new(backend);

    let test_code = "(+ 1 2)";
    println!("\n=== Evaluating: {} ===", test_code);

    let result = interp.eval_str(test_code).unwrap();
    assert_eq!(result.as_fixnum(), Some(3));
}

#[test]
fn test_macro_expression_flow() {
    // What happens with a macro expression?
    let backend = TreeWalker::new();
    let interp = Interpreter::new(backend);

    let test_code = "(or #f #t)";
    println!("\n=== Evaluating macro: {} ===", test_code);

    let result = interp.eval_str(test_code).unwrap();
    assert_eq!(result, patina_core::TaggedValue::TRUE);
}

#[test]
fn test_define_with_macro_body() {
    // What happens when we define a function with macros in the body?
    let backend = TreeWalker::new();
    let interp = Interpreter::new(backend);

    let define_code = "(define (test-or x) (or (= x 1) (= x 2)))";
    println!("\n=== Defining function with macros: {} ===", define_code);

    interp.eval_str(define_code).unwrap();

    // Now call the function
    let call_code = "(test-or 1)";
    println!("\n=== Calling function: {} ===", call_code);
    let call_result = interp.eval_str(call_code).unwrap();
    assert_eq!(call_result, patina_core::TaggedValue::TRUE);
}

#[test]
fn test_interpreter_api_with_macros() {
    // Test through the Interpreter API (which uses Backend)
    let backend = TreeWalker::new();
    let interp = Interpreter::new(backend);

    let code = r#"
        (define (f x)
          (cond
            ((= x 1) 'one)
            ((= x 2) 'two)
            (else 'other)))
        (f 1)
    "#;

    println!("\n=== Testing complex macro code ===");
    let result = interp.eval_program(code).unwrap();
    let heap = interp.backend().evaluator().global_env.heap();
    let heap_ref = heap.borrow();
    let name = heap_ref.get_symbol_name(result);
    assert_eq!(name, Some("one"));
}
