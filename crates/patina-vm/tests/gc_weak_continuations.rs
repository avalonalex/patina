//! Weak continuation side tables (`docs/GC_DESIGN.md` §9.5).
//!
//! The stores on `VmState` must behave like weak tables keyed by their
//! `VmContinuationRef` / `VmDelimitedContinuationRef` heap objects: an entry
//! whose ref object survives marking keeps its payload (and everything the
//! payload's snapshot pins) alive; an entry whose ref died is pruned, and the
//! heap values only its snapshot pinned become reclaimable in the same
//! collection.
//!
//! These tests drive `MarkSweepCollector` directly against a hand-built
//! `VmState` so they can assert on store contents and arena free lists —
//! the Scheme-level behavior is covered in `patina-tests/tests/gc_vm.rs`.

use patina_core::environment::Environment;
use patina_core::tagged_value::TaggedValue;
use patina_core::{Collector, MarkSweepCollector};
use patina_vm::runtime::VmState;
use patina_vm::types::{VmContinuation, VmDelimitedContinuation};
use std::rc::Rc;

fn fresh_state() -> VmState {
    VmState::new(Rc::new(Environment::new()))
}

/// A full continuation whose register snapshot pins `payload`.
fn continuation_pinning(payload: TaggedValue) -> VmContinuation {
    VmContinuation {
        frames: vec![],
        dynamic_winds: vec![],
        prompt_stack: vec![],
        exception_handlers: vec![],
        registers: vec![payload],
        deliver_reg: 0,
    }
}

fn delimited_continuation_pinning(payload: TaggedValue) -> VmDelimitedContinuation {
    VmDelimitedContinuation {
        frames: vec![],
        dynamic_winds: vec![],
        registers: vec![payload],
        base_at_capture: 0,
    }
}

fn collect(state: &VmState) {
    let mut heap = state.heap.borrow_mut();
    MarkSweepCollector::new().collect(&mut heap, &[state]);
}

#[test]
fn dead_entry_pruned_live_entry_survives() {
    let mut state = fresh_state();

    let live_pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
    let dead_pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);

    let live_ref = state.alloc_vm_continuation(continuation_pinning(live_pin));
    let _dead_ref = state.alloc_vm_continuation(continuation_pinning(dead_pin));
    // Only the live ref is rooted (register file); the dead ref is dropped.
    state.registers.push(live_ref);
    assert_eq!(state.continuation_store.borrow().len(), 2);

    collect(&state);

    // Dead entry pruned; the pair only its snapshot pinned is reclaimed
    // (the sole free pair — the live snapshot's pin survived).
    assert_eq!(state.continuation_store.borrow().len(), 1);
    assert!(state.get_vm_continuation(live_ref).is_some());
    let heap = state.heap.borrow();
    assert_eq!(heap.stats().free_pairs, 1);
    assert_eq!(heap.car(live_pin), TaggedValue::fixnum(1));
}

#[test]
fn continuation_reachable_only_through_live_payload_survives() {
    // k_inner's ref exists only inside k_outer's register snapshot: the weak
    // fixpoint must trace k_outer's payload (its ref is rooted) and thereby
    // prove k_inner live — one round of discovery per chain link.
    let mut state = fresh_state();

    let inner_pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(7), TaggedValue::NULL);
    let inner_ref = state.alloc_vm_continuation(continuation_pinning(inner_pin));
    let outer_ref = state.alloc_vm_continuation(continuation_pinning(inner_ref));
    state.registers.push(outer_ref);

    collect(&state);

    assert_eq!(state.continuation_store.borrow().len(), 2);
    assert!(state.get_vm_continuation(inner_ref).is_some());
    let heap = state.heap.borrow();
    assert_eq!(heap.stats().free_pairs, 0);
    assert_eq!(heap.car(inner_pin), TaggedValue::fixnum(7));
}

#[test]
fn dead_chain_fully_pruned_in_one_collection() {
    // A chain of captures where each snapshot pins the previous ref — the
    // ctak shape. Once the head ref is dropped, no entry may keep another
    // alive through the side table: the whole chain must go in ONE
    // collection, not one link per collection.
    let mut state = fresh_state();

    let mut prev = TaggedValue::NULL;
    for _ in 0..10 {
        prev = state.alloc_vm_continuation(continuation_pinning(prev));
    }
    assert_eq!(state.continuation_store.borrow().len(), 10);

    collect(&state);

    assert_eq!(state.continuation_store.borrow().len(), 0);
}

#[test]
fn delimited_entries_weak_too() {
    let mut state = fresh_state();

    let live_pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL);
    let dead_pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(4), TaggedValue::NULL);

    let live_ref = state.alloc_vm_delimited_continuation(delimited_continuation_pinning(live_pin));
    let _dead_ref = state.alloc_vm_delimited_continuation(delimited_continuation_pinning(dead_pin));
    state.registers.push(live_ref);

    collect(&state);

    assert_eq!(state.delimited_continuation_store.borrow().len(), 1);
    assert!(state.get_vm_delimited_continuation(live_ref).is_some());
    let heap = state.heap.borrow();
    assert_eq!(heap.stats().free_pairs, 1);
    assert_eq!(heap.car(live_pin), TaggedValue::fixnum(3));
}

#[test]
fn full_and_delimited_refs_cross_reference() {
    // A rooted full continuation whose snapshot pins a delimited ref: the
    // fixpoint must cross store boundaries in both directions.
    let mut state = fresh_state();

    let pin = state
        .heap
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(9), TaggedValue::NULL);
    let delim_ref = state.alloc_vm_delimited_continuation(delimited_continuation_pinning(pin));
    let full_ref = state.alloc_vm_continuation(continuation_pinning(delim_ref));
    state.registers.push(full_ref);

    collect(&state);

    assert_eq!(state.continuation_store.borrow().len(), 1);
    assert_eq!(state.delimited_continuation_store.borrow().len(), 1);
    let heap = state.heap.borrow();
    assert_eq!(heap.car(pin), TaggedValue::fixnum(9));
}

#[test]
fn store_stays_bounded_under_capture_churn() {
    // The §9.5 blowup in miniature: repeated capture-and-drop cycles with
    // periodic collections must not grow the store or the pair arena.
    let mut state = fresh_state();

    for round in 0..10 {
        for i in 0..100 {
            let pin = state
                .heap
                .borrow_mut()
                .alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
            let _ = state.alloc_vm_continuation(continuation_pinning(pin));
        }
        collect(&state);
        assert_eq!(
            state.continuation_store.borrow().len(),
            0,
            "store leaked entries after round {round}"
        );
    }
    // Every round's snapshot pins were reclaimed: the arena never grows past
    // one round's worth of pairs.
    assert!(state.heap.borrow().stats().pairs <= 100);
}
