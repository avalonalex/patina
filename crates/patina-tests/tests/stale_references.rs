//! A stale value held by an embedding host panics in a check build (#621).
//!
//! What `eval_program` returns is a bare `TaggedValue` that no root provider
//! sees, so a later collecting evaluation frees it (#605). Before #621 the
//! host then read a swept vector as `#()`, a swept string as `""`, and a pair
//! whose slot had been reused as part of the slot's new tenant, in debug and
//! release alike; and a stale value it handed back to the interpreter was
//! absorbed by the next collection without a report. Each test here holds
//! such a value through today's bare-value API, on both backends, and expects
//! the panic of the check that sees it.
//!
//! These are positive controls for the checks, not tests of the behaviour a
//! host should get. #605 is to root host values with handles while keeping
//! the bare-value methods (deprecated), so these shapes stay valid after it.
//!
//! They run in every check build: every debug `cargo test`, and a release
//! one with this crate's `gc-check` feature, which the release GC lane
//! enables. A build without the checks reports them ignored.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

mod common;

use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::Interpreter;
use patina_runtime::Backend;

/// A collecting evaluation: `(gc)` raises the pending flag, and the safe
/// point at the next form collects.
const COLLECT: &str = "(import (patina debug)) (gc) 0";

/// Hold what `make` evaluates to across a collecting evaluation, then read it.
fn read_after_a_collection<B: Backend>(interp: Interpreter<B>, make: &str) {
    let held = interp.eval_program(make).expect("make the value");
    interp.eval_program(COLLECT).expect("collect");
    interp.display_tagged(held);
}

/// A pair for the host to hold, with 20,000 pairs of garbage allocated after
/// it. Sweep frees slots in index order and allocation reuses the last freed
/// first, so the padding sits above the held pair on the free list: the next
/// programs' own garbage (the reader's lists) takes padding slots, never the
/// held pair's. Without it the held slot went to a parsed form, and the next
/// collection freed it again.
const HELD_PAIR: &str = "(let ((held (cons 1 2))) (make-list 20000 0) held)";

/// Hold a pair across a collection that frees it, then across an evaluation
/// that keeps 300,000 new pairs live — more than the arena has free slots, so
/// one of them takes the held pair's — then read it. Before #621 this read
/// the new tenant, part of `keep`, in every build.
fn read_after_its_slot_is_reused<B: Backend>(interp: Interpreter<B>) {
    let held = interp.eval_program(HELD_PAIR).expect("make the pair");
    interp.eval_program(COLLECT).expect("collect");
    interp
        .eval_program("(define keep (make-list 300000 0)) 0")
        .expect("reuse the freed slots");
    interp.display_tagged(held);
}

/// Hold a pair across a collection that frees it, hand it back by binding it
/// in the global environment, and collect: marking reaches the free slot.
/// Before #621 that collection absorbed it without a report.
fn bind_and_collect<B: Backend>(interp: Interpreter<B>) {
    let held = interp.eval_program(HELD_PAIR).expect("make the pair");
    interp.eval_program(COLLECT).expect("collect");
    interp.global_env().define("stale", held);
    let _ = interp.eval_program("(gc) 0");
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
    held_vector_vm,
    "use-after-free: vector slot",
    read_after_a_collection(vm_interpreter(), "(make-vector 3 7)")
);
control!(
    held_vector_tree_walker,
    "use-after-free: vector slot",
    read_after_a_collection(tree_walker_interpreter(), "(make-vector 3 7)")
);

control!(
    held_string_vm,
    "use-after-free: string slot",
    read_after_a_collection(vm_interpreter(), "(make-string 3 #\\z)")
);
control!(
    held_string_tree_walker,
    "use-after-free: string slot",
    read_after_a_collection(tree_walker_interpreter(), "(make-string 3 #\\z)")
);

control!(
    held_bytevector_vm,
    "use-after-free: object slot",
    read_after_a_collection(vm_interpreter(), "(make-bytevector 3 7)")
);
control!(
    held_bytevector_tree_walker,
    "use-after-free: object slot",
    read_after_a_collection(tree_walker_interpreter(), "(make-bytevector 3 7)")
);

control!(
    held_pair_whose_slot_is_reused_vm,
    "stale pair reference, slot",
    read_after_its_slot_is_reused(vm_interpreter())
);
control!(
    held_pair_whose_slot_is_reused_tree_walker,
    "stale pair reference, slot",
    read_after_its_slot_is_reused(tree_walker_interpreter())
);

control!(
    stale_pair_bound_in_the_global_environment_vm,
    "is free, but marking reached it",
    bind_and_collect(vm_interpreter())
);
control!(
    stale_pair_bound_in_the_global_environment_tree_walker,
    "is free, but marking reached it",
    bind_and_collect(tree_walker_interpreter())
);
