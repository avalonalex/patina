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
//!
//! The last test here is no control but the collect capability's rule at a
//! backend's entry (#702): a run beneath the carrier's driver, held further
//! out, is nested and never collects.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

mod common;

use common::{tree_walker_interpreter, vm_interpreter};
use patina_core::{AssertNoGc, GcDriver};
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

/// The collect capability (#702): only the run that holds the carrier's
/// driver collects. With the driver held further out, as an outer driver
/// entry holds it, a run is nested: its `(gc)` is posted and counted, not
/// run. The first run after the driver goes back collects.
#[test]
fn a_run_beneath_a_held_driver_never_collects() {
    fn run<B: Backend>(interp: Interpreter<B>) {
        let heap = interp.global_env().heap().clone();
        let counts = || {
            let h = heap.borrow();
            (h.gc_collections(), h.gc_deferred_collections())
        };
        interp
            .eval_program_owned("(import (scheme base) (patina debug))")
            .expect("import");
        let driver = GcDriver::acquire(&heap).expect("free between evaluations");
        let (collections, deferred) = counts();
        interp.eval_program_owned("(gc)").expect("evaluate");
        assert_eq!(counts(), (collections, deferred + 1), "posted and counted");
        drop(driver);
        interp.eval_program_owned("(gc)").expect("evaluate");
        let (after, deferred_after) = counts();
        assert!(after > collections, "collected once the driver went back");
        assert_eq!(deferred_after, deferred + 1, "nothing more deferred");
    }
    run(vm_interpreter());
    run(tree_walker_interpreter());
}
