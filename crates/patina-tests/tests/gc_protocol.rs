//! A GC poll inside an `AssertNoGc` window panics, on both backends (#624).
//!
//! An `AssertNoGc` scope covers a window whose soundness rests on reaching no
//! safe point: a weak-table entry and its handle not yet both reachable, a
//! transfer's operands held in Rust locals until the next write. Every poll
//! site checks that no scope is open, before its safe point and on every
//! iteration, whether or not the loop could collect there. Each test here
//! opens a window and then evaluates, so the backend's own loop reaches a poll
//! inside it, and expects that check's panic.
//!
//! These are positive controls for the check, which is compiled into check
//! builds only: every debug `cargo test`, and a release one with this crate's
//! `gc-check` feature, which the release GC lane enables. A build without the
//! check reports them ignored. The core-level controls for the rest of the
//! protocol (the depth at collection, a holder's extent, the defer balance)
//! are `patina-core`'s `heap::gc::tests::protocol`.

mod common;

use common::{tree_walker_interpreter, vm_interpreter};
use patina_core::AssertNoGc;
use patina_interpreter::Interpreter;
use patina_runtime::Backend;

/// Evaluate a program while a window is open. The heap is the interpreter's
/// own, so the window is on the counter its dispatch loop polls.
fn evaluate_inside_a_window<B: Backend>(interp: Interpreter<B>) {
    let env = interp.global_env();
    let _window = AssertNoGc::new(env.heap());
    let _ = interp.eval_program("(+ 1 2)");
}

/// A control: a test that must panic with `$expected` in a check build.
macro_rules! control {
    ($name:ident, $expected:literal, $body:expr) => {
        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = $expected)]
        fn $name() {
            $body;
        }
    };
}

control!(
    a_poll_inside_a_window_panics_vm,
    "GC poll inside an AssertNoGc scope",
    evaluate_inside_a_window(vm_interpreter())
);
control!(
    a_poll_inside_a_window_panics_tree_walker,
    "GC poll inside an AssertNoGc scope",
    evaluate_inside_a_window(tree_walker_interpreter())
);

/// The same evaluation after the window has closed polls freely: what the
/// controls see is the open window, not the evaluation.
#[test]
fn a_poll_after_the_window_closes_passes() {
    fn run<B: Backend>(interp: Interpreter<B>) {
        let env = interp.global_env();
        drop(AssertNoGc::new(env.heap()));
        let value = interp.eval_program("(+ 1 2)").expect("evaluate");
        assert_eq!(value, patina_core::TaggedValue::fixnum(3));
    }
    run(vm_interpreter());
    run(tree_walker_interpreter());
}
