//! A host keeps a value through a handle, on both backends (#605).
//!
//! Every `eval_*` method answered a bare `TaggedValue`, which names a slot and
//! roots nothing: a value a host kept across a later evaluation that collected
//! read whatever came to occupy its slot, a use after free in a debug build
//! and another object in a release one. The `eval_*_owned` methods answer an
//! `Owned` handle, which the heap roots until the handle is dropped, and the
//! loop behind every program entry holds the last form's value the same way,
//! so a program that carries on past errors still has the last value it got.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// Defines `churn`, which allocates enough to collect on its own.
const PRELUDE: &str = "(import (scheme base) (patina debug))
    (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))";

/// Collects, by allocation and then by `(gc)`, and allocates into the slots
/// that frees, so that a freed value reads as another.
const CHURN: &str = "(churn 200000) (gc) (define keep (list 'a 'b 'c 'd))";

fn collections<B: Backend>(interp: &Interpreter<B>) -> u64 {
    interp.global_env().heap().borrow().gc_collections()
}

fn handles<B: Backend>(interp: &Interpreter<B>) -> usize {
    interp.global_env().heap().borrow().handle_count()
}

/// The message of the panic `f` raises.
fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("a panic");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .unwrap_or_default(),
    }
}

/// #605's first program: a value kept across a program that collects.
fn kept_across_a_collection<B: Backend>(interp: Interpreter<B>) -> String {
    interp.eval_program_owned(PRELUDE).unwrap();
    let held = interp
        .eval_str_owned("(list 'held (vector 1 2 3))")
        .unwrap();
    let before = collections(&interp);
    interp.eval_program_owned(CHURN).unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    interp.display_tagged(&held)
}

#[test]
fn a_kept_value_survives_a_later_program_that_collects() {
    assert_eq!(
        kept_across_a_collection(vm_interpreter()),
        "(held #(1 2 3))"
    );
    assert_eq!(
        kept_across_a_collection(tree_walker_interpreter()),
        "(held #(1 2 3))"
    );
}

/// #605's second program: the last value of a program that carries on past
/// an error, where the failing form collects.
fn last_value_after_a_failing_form<B: Backend>(interp: Interpreter<B>) -> [String; 2] {
    interp.eval_program_owned(PRELUDE).unwrap();
    let program = "(list 'first 1 2) (begin (churn 200000) (gc) (car 5))";
    let before = collections(&interp);
    let owned = interp.eval_program_resilient_owned(program);
    assert!(collections(&interp) > before, "the program did not collect");
    let owned = interp.display_tagged(&owned);
    #[allow(deprecated)]
    let bare = interp.eval_program_resilient(program);
    [owned, interp.display_tagged(bare)]
}

#[test]
fn a_program_that_carries_on_keeps_its_last_value() {
    let expected = ["(first 1 2)", "(first 1 2)"];
    assert_eq!(last_value_after_a_failing_form(vm_interpreter()), expected);
    assert_eq!(
        last_value_after_a_failing_form(tree_walker_interpreter()),
        expected
    );
}

/// A global read through `lookup`, kept after the program rebinds it.
fn global_kept_after_rebinding<B: Backend>(interp: Interpreter<B>) -> String {
    interp
        .eval_program_owned(&format!(
            "{PRELUDE} (define global (list 'global (vector 4 5 6)))"
        ))
        .unwrap();
    assert!(interp.lookup("no-such-global").is_none());
    let held = interp.lookup("global").expect("`global` is bound");
    interp
        .eval_program_owned(&format!("(set! global #f) {CHURN}"))
        .unwrap();
    interp.display_tagged(&held)
}

#[test]
fn a_global_read_by_lookup_survives_its_rebinding() {
    let expected = "(global #(4 5 6))";
    assert_eq!(global_kept_after_rebinding(vm_interpreter()), expected);
    assert_eq!(
        global_kept_after_rebinding(tree_walker_interpreter()),
        expected
    );
}

/// A clone is a handle of its own: it keeps the value after the original is
/// dropped.
fn kept_by_a_clone<B: Backend>(interp: Interpreter<B>) -> String {
    interp.eval_program_owned(PRELUDE).unwrap();
    let held = interp.eval_str_owned("(list 'cloned \"text\")").unwrap();
    let copy = held.clone();
    drop(held);
    interp.eval_program_owned(CHURN).unwrap();
    interp.display_tagged(&copy)
}

#[test]
fn a_clone_keeps_the_value_after_the_original_is_dropped() {
    let expected = "(cloned \"text\")";
    assert_eq!(kept_by_a_clone(vm_interpreter()), expected);
    assert_eq!(kept_by_a_clone(tree_walker_interpreter()), expected);
}

/// The only handles alive are the ones the host holds: each entry point lets
/// go of the one it held its running value in, whether the program finished,
/// failed or carried on.
fn handles_left_behind<B: Backend>(interp: Interpreter<B>) {
    interp.eval_program_owned("(import (scheme base))").unwrap();
    assert_eq!(handles(&interp), 0);
    let held = interp.eval_program_owned("1 2 3").unwrap();
    assert_eq!(handles(&interp), 1);
    drop(interp.eval_program_resilient_owned("1 (car 5) 2"));
    let (result, _) = interp.eval_program_with_source_name_owned("1 (car 5)", "stops.scm");
    assert!(result.is_err());
    assert!(interp.eval_program_owned("1 (").is_err());
    assert!(interp.eval_str_owned("(car 5)").is_err());
    #[allow(deprecated)]
    let bare = interp.eval_program("4 5").unwrap();
    assert_eq!(bare.as_fixnum(), Some(5));
    assert_eq!(handles(&interp), 1);
    assert_eq!(interp.raw_value(&held).as_fixnum(), Some(3));
    drop(held);
    assert_eq!(handles(&interp), 0);
}

#[test]
fn evaluation_leaves_no_handle_behind() {
    handles_left_behind(vm_interpreter());
    handles_left_behind(tree_walker_interpreter());
}

#[test]
fn a_handle_used_with_another_interpreter_is_refused() {
    let vm = vm_interpreter();
    let tree_walker = tree_walker_interpreter();
    let held = vm.eval_str_owned("(list 1 2)").unwrap();
    let message = panic_message(|| {
        tree_walker.display_tagged(&held);
    });
    assert!(message.contains("used with heap"), "{message}");
    let other_vm = vm_interpreter();
    let message = panic_message(|| {
        other_vm.raw_value(&held);
    });
    assert!(message.contains("used with heap"), "{message}");
    // Refused, not spoiled: the interpreter that made it still reads it.
    assert_eq!(vm.display_tagged(&held), "(1 2)");
}

/// A handle kept past its interpreter keeps nothing alive, and dropping it,
/// or a clone of it, does nothing.
fn kept_past_its_interpreter<B: Backend>(interp: Interpreter<B>) {
    let held = interp.eval_str_owned("(list 'held 1 2)").unwrap();
    let copy = held.clone();
    let heap = Rc::downgrade(interp.global_env().heap());
    drop(interp);
    assert!(heap.upgrade().is_none(), "a handle kept the heap alive");
    drop(held);
    let again = copy.clone();
    drop(copy);
    drop(again);
}

#[test]
fn a_handle_kept_past_its_interpreter_holds_nothing() {
    kept_past_its_interpreter(vm_interpreter());
    kept_past_its_interpreter(tree_walker_interpreter());
}
