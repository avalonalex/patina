//! A host calls a Scheme procedure, on both backends (#710).
//!
//! `Interpreter::call` applies the procedure a handle holds to the values
//! handles hold, as a program's application of it at the top level does. It
//! is a driver entry, as the `eval_*` methods are: the callee may collect
//! while it runs, with its arguments rooted only by their handles, and a
//! continuation captured inside it behaves as one captured in a form.
//! `Interpreter::with` makes the values a host passes.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_core::Value;
use patina_interpreter::{Backend, Interpreter};

fn collections<B: Backend>(interp: &Interpreter<B>) -> u64 {
    interp.heap().borrow().gc_collections()
}

/// Collects, by allocation and then by `(gc)`, before it reads its arguments.
const COLLECTING_CALLEE: &str = "(import (scheme base) (patina debug))
    (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
    (define (churn-then-sum numbers box)
      (churn 200000)
      (gc)
      (+ (apply + numbers) (vector-ref box 0)))";

/// The arguments are the host's, made in `with`: nothing but their handles
/// holds them, and the callee collects before it reads them.
fn a_call_that_collects<B: Backend>(mut interp: Interpreter<B>) -> String {
    interp.eval_program_owned(COLLECTING_CALLEE).unwrap();
    let callee = interp.lookup("churn-then-sum").unwrap();
    let (numbers, cell) = interp.with(|cx| {
        let numbers = cx.list_from_iter([Value::fixnum(1), Value::fixnum(2), Value::fixnum(3)]);
        let cell = cx.alloc_vector_fill(1, Value::fixnum(10));
        (cx.hold(numbers), cx.hold(cell))
    });
    let before = collections(&interp);
    let sum = interp.call(&callee, &[numbers, cell]).unwrap();
    assert!(collections(&interp) > before, "the call did not collect");
    interp.display_tagged(&sum)
}

#[test]
fn a_call_collects_with_its_arguments_held_by_their_handles() {
    assert_eq!(a_call_that_collects(vm_interpreter()), "16");
    assert_eq!(a_call_that_collects(tree_walker_interpreter()), "16");
}

/// Captures a continuation in its argument's computation and keeps it in `k`.
const CAPTURING_CALLEE: &str = "(import (scheme base))
    (define k #f)
    (define (capture x) (+ 1 (call/cc (lambda (c) (set! k c) x))))";

/// What the capture and a later `(k 10)` answer, the capture made by `call`.
fn captured_by_call<B: Backend>(mut interp: Interpreter<B>) -> (String, String) {
    interp.eval_program_owned(CAPTURING_CALLEE).unwrap();
    let capture = interp.lookup("capture").unwrap();
    let one = interp.with(|cx| cx.hold(Value::fixnum(1)));
    let first = interp.call(&capture, &[one]).unwrap();
    let again = interp.eval_str_owned("(k 10)").unwrap();
    (interp.display_tagged(&first), interp.display_tagged(&again))
}

/// The same, the capture made by a form.
fn captured_by_a_form<B: Backend>(interp: Interpreter<B>) -> (String, String) {
    interp.eval_program_owned(CAPTURING_CALLEE).unwrap();
    let first = interp.eval_str_owned("(capture 1)").unwrap();
    let again = interp.eval_str_owned("(k 10)").unwrap();
    (interp.display_tagged(&first), interp.display_tagged(&again))
}

#[test]
fn a_continuation_captured_in_a_call_behaves_as_one_captured_in_a_form() {
    let pair = |first: &str, again: &str| (first.to_string(), again.to_string());
    for (by_call, by_form) in [
        (
            captured_by_call(vm_interpreter()),
            captured_by_a_form(vm_interpreter()),
        ),
        (
            captured_by_call(tree_walker_interpreter()),
            captured_by_a_form(tree_walker_interpreter()),
        ),
    ] {
        assert_eq!(by_call, by_form);
        assert_eq!(by_call, pair("2", "11"));
    }
}

/// A program that rebinds `quote`, and so the spelling a call built as
/// source text would quote its arguments with.
fn a_call_with_quote_rebound<B: Backend>(mut interp: Interpreter<B>) -> String {
    interp
        .eval_program_owned(
            "(import (scheme base)) (define quote car) (define (pair a b) (list a b))",
        )
        .unwrap();
    let pair = interp.lookup("pair").unwrap();
    let (symbol, list) = interp.with(|cx| {
        let symbol = cx.intern_symbol("x");
        let list = cx.list_from_iter([Value::fixnum(1), Value::fixnum(2)]);
        (cx.hold(symbol), cx.hold(list))
    });
    let answer = interp.call(&pair, &[symbol, list]).unwrap();
    interp.display_tagged(&answer)
}

#[test]
fn a_call_consults_no_binding_of_the_program() {
    // The arguments arrive as themselves: a symbol is not looked up, and a
    // list is not applied, whatever `quote` is bound to.
    assert_eq!(a_call_with_quote_rebound(vm_interpreter()), "(x (1 2))");
    assert_eq!(
        a_call_with_quote_rebound(tree_walker_interpreter()),
        "(x (1 2))"
    );
}

/// What a call of a non-procedure, and of a procedure with too few
/// arguments, answers: the error each form would raise.
fn calls_that_fail<B: Backend>(mut interp: Interpreter<B>) -> (bool, bool, String) {
    interp
        .eval_program_owned("(import (scheme base)) (define (two a b) (+ a b))")
        .unwrap();
    let two = interp.lookup("two").unwrap();
    let five = interp.with(|cx| cx.hold(Value::fixnum(5)));
    let not_a_procedure = interp.call(&five, &[]).is_err();
    let too_few = interp.call(&two, std::slice::from_ref(&five)).is_err();
    // The interpreter carries on afterwards.
    let sum = interp.call(&two, &[five.clone(), five]).unwrap();
    (not_a_procedure, too_few, interp.display_tagged(&sum))
}

#[test]
fn a_call_that_fails_answers_an_error_and_the_interpreter_carries_on() {
    for answer in [
        calls_that_fail(vm_interpreter()),
        calls_that_fail(tree_walker_interpreter()),
    ] {
        assert_eq!(answer, (true, true, "10".to_string()));
    }
}
