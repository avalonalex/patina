//! A GC poll inside an `AssertNoGc` window panics, on both backends (#624).
//!
//! An `AssertNoGc` scope covers a window whose soundness rests on reaching no
//! safe point: a weak-table entry and its handle not yet both reachable, a
//! transfer's operands held in Rust locals until the next write. Every poll
//! site checks that no scope is open, before its safe point and on every
//! iteration, whether or not the loop could collect there. The first two
//! tests here open a window and then evaluate, so the backend's own loop
//! reaches a poll inside it, and expect that check's panic. In the third a
//! host primitive opens one inside a VM instruction that polls before it
//! ends: the loop checked before the instruction began, so only the poll's
//! own check can see the window (#864).
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
#![expect(
    clippy::disallowed_macros,
    reason = "OPENED: the heap and interrupt handle of the test's interpreter, for a host \
              primitive of the test, and the window it leaves open (#864)"
)]

mod common;

use common::{tree_walker_interpreter, vm_interpreter};
use patina_core::{AssertNoGc, Cx, GcDriver, InterruptHandle, SharedHeap, Value, event};
use patina_interpreter::{Arity, EvalError, Interpreter, PrimClass, PrimSpec};
use patina_runtime::Backend;
use std::cell::RefCell;

/// Evaluate a program while a window is open. The heap is the interpreter's
/// own, so the window is on the counter its dispatch loop polls.
fn evaluate_inside_a_window<B: Backend>(interp: Interpreter<B>) {
    let _window = AssertNoGc::new(interp.backend().heap());
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

thread_local! {
    /// What `(open-a-window!)` opens its window on and posts its interrupt
    /// to: the heap and the interrupt handle of the test's interpreter; and
    /// the window, open from the primitive's call until the test lets go of
    /// it all.
    static OPENED: RefCell<Option<Opened>> = const { RefCell::new(None) };
}

struct Opened {
    heap: SharedHeap,
    interrupt: InterruptHandle,
    window: Option<AssertNoGc>,
}

/// Lets go of [`OPENED`], the window with it, however the test ends:
/// declared after the interpreter, it goes first in the unwind.
struct LetGo;

impl Drop for LetGo {
    fn drop(&mut self) {
        OPENED.take();
    }
}

/// `(open-a-window!)`: opens an `AssertNoGc` window on [`OPENED`]'s heap
/// and leaves it open, and posts an interrupt, so that the next poll has
/// something to do that is not a collection.
fn open_a_window<'gc>(_: &mut Cx<'gc>, _: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    OPENED.with_borrow_mut(|opened| {
        let opened = opened.as_mut().expect("the test names its interpreter");
        opened.window = Some(AssertNoGc::new(&opened.heap));
        assert!(
            opened.interrupt.post(event::SIGNAL),
            "the interpreter lives"
        );
    });
    Ok(Value::UNSPECIFIED)
}

/// Call `(open-a-window!)` with `apply`, a generic call, whose return is a
/// poll inside the `Apply` instruction (#700): the window opens after the
/// loop's check on that instruction's iteration, and the poll is the first
/// to meet it. A check build's poll panics there (#864). Where the poll
/// checks nothing, the poll raises the interrupt, which no handler takes,
/// and the run ends with it before the loop could check again: no panic.
fn poll_inside_an_instruction() {
    let mut interp = vm_interpreter();
    interp
        .register_primitive(
            &["test", "window"],
            "open-a-window!",
            PrimSpec {
                f: open_a_window,
                arity: Arity::Exact(0),
                class: PrimClass::Leaf,
            },
        )
        .expect("register");
    interp
        .eval_program_owned("(import (scheme base) (test window))")
        .expect("import");
    let _let_go = LetGo;
    OPENED.set(Some(Opened {
        heap: interp.backend().heap().clone(),
        interrupt: interp.interrupt_handle(),
        window: None,
    }));
    let _ = interp.eval_program_owned("(apply open-a-window! '())");
}

control!(
    a_poll_inside_an_instruction_inside_a_window_panics_vm,
    "GC poll inside an AssertNoGc scope",
    poll_inside_an_instruction()
);

/// The same evaluation after the window has closed polls freely: what the
/// controls see is the open window, not the evaluation.
#[test]
fn a_poll_after_the_window_closes_passes() {
    fn run<B: Backend>(interp: Interpreter<B>) {
        drop(AssertNoGc::new(interp.backend().heap()));
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
        let heap = interp.backend().heap().clone();
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
