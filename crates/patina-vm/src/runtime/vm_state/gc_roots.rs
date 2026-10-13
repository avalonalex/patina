//! GC root providers for the VM (`docs/GC_DESIGN.md` §5.2).
//!
//! Unlike the tree-walker, whose live state is a Rust local, the VM keeps
//! everything on `VmState` — so `impl RootProvider for VmState` covers almost the
//! whole root set. One member of it is invisible to a heap scan, and is the
//! reason this impl has to exist at all: **the continuation side tables.**
//! The heap holds only an opaque `VmContinuationRef { id, bytes }`, charged
//! the payload's size for the byte account (#606); the payloads live in
//! `continuation_store` and `delimited_continuation_store`. Nothing but this
//! impl reaches them — and they are **weak** (design §9.5, which has the
//! measurements and the full argument): a payload is traced only when its
//! ref object was itself marked (`trace_weak_ids`), and entries whose ref
//! died are pruned after marking (`sweep_weak`). Tracing them as strong
//! roots instead made every capture immortal — snapshots contain other
//! continuation refs, so the tables pinned themselves transitively.
//!
//! A frame's closure was a second member, a bare index (`ObjectIndex`) that
//! a pass over every `TaggedValue` would have stepped past, rooted through a
//! visitor method of its own. It is a value like the frame's registers now
//! (#682).
//!
//! The registers and the frames' closures, live and in captured
//! continuations, are reported as slots, where they live (#682). Everything
//! else is pinned until a stage makes it heap data: the extents' records and
//! the code's constants at 4e, the environments at 4b; and the scratch
//! arguments and the pending escape, empty or unset at every safe point.
//!
//! The weak protocol is sound because every store touch (capture, invoke)
//! is confined to one instruction dispatch and nested loops defer
//! collection — so at a collecting safe point an unmarked ref proves its
//! payload unreachable.
//!
//! Rust-stack temporaries (continuation-capture register clones, `mem::take`n
//! buffers, primitive argument vectors) are handled by deferral rather than
//! tracing — see `NoGcScope` and §7. The environments `with_globals` sets
//! aside were too, until they moved onto the traced `saved_globals` (#677).

use patina_core::{GC_CHECK, GcVisitor, RootProvider, SlotVisitor, Trace};
use rustc_hash::FxHashMap;

use crate::types::CallFrame;
use patina_core::TaggedValue;
use std::cell::Cell;

use super::VmState;

/// What retirement writes into a register its frame's map calls dead. In a
/// check build (`GC_CHECK`: debug, or release with `gc-check`) that is
/// `DEAD_SLOT`, which the VM refuses to read (`VmState::reg_at`,
/// `control::argument_reg`) and the heap refuses to store, so a map that
/// calls a live register dead panics at the read rather than handing the
/// program a legal `UNSPECIFIED` (#625, GC_PRD §11.1 invariant 3). A plain
/// release build writes `UNSPECIFIED`.
const RETIRED: TaggedValue = if GC_CHECK {
    TaggedValue::DEAD_SLOT
} else {
    TaggedValue::UNSPECIFIED
};

/// Clear finished compiler temporaries before collection or snapshotting.
/// Keeping the full-vector tracing contract means no captured snapshot or
/// tracer can later expose an untraced pointer to an already swept slot.
/// Delimited snapshots use absolute frame bases but store only their slice.
#[cold]
#[inline(never)]
pub(in crate::runtime) fn retire_registers(
    registers: &mut [Cell<TaggedValue>],
    frames: &[CallFrame],
    base_at_capture: usize,
) {
    for frame in frames {
        let Some(maps) = &frame.code.register_roots else {
            continue;
        };
        let Some(roots) = maps.get(frame.pc) else {
            continue;
        };
        let dropped = wrong_maps::dropped(roots, frame.num_regs);
        let base = frame.register_base - base_at_capture;
        let window = &mut registers[base..base + frame.num_regs as usize];
        for (reg, value) in window.iter_mut().enumerate() {
            // A tail call can reuse a window larger than the new code needs.
            let live = roots.get(reg / 64).copied().unwrap_or(0) & (1 << (reg % 64)) != 0;
            if !live || dropped == Some(reg) {
                value.set(RETIRED);
            }
        }
    }
}

/// The register a wrong-map control drops from `roots` (#625): `None`
/// unless `test-support` is compiled in and its switch is on
/// (`test_support::DropHighestLive`). Without the feature this is a
/// constant `None`, and the comparison in `retire_registers` folds away.
#[cfg(not(feature = "test-support"))]
mod wrong_maps {
    #[inline(always)]
    pub(super) fn dropped(_roots: &[u64], _num_regs: u16) -> Option<usize> {
        None
    }
}

#[cfg(feature = "test-support")]
use crate::test_support as wrong_maps;

impl RootProvider for VmState {
    /// `VmState`'s derived trace (#686), whose fields say what each holds.
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        Trace::trace(self, visitor);
    }

    fn trace_weak_ids(&self, ids: &[u64], visitor: &mut dyn SlotVisitor) {
        // Each id was proven live by marking; an id keys at most one entry
        // across both stores (heap-minted), and one not found here belongs
        // to a dead temporary VmState — nothing to trace.
        let conts = self.continuation_store.borrow();
        let delims = self.delimited_continuation_store.borrow();
        for id in ids {
            if let Some(continuation) = conts.get(id) {
                continuation.as_ref().trace(visitor);
            } else if let Some(continuation) = delims.get(id) {
                continuation.as_ref().trace(visitor);
            }
        }
    }

    fn sweep_weak(&self, visitor: &GcVisitor<'_>) {
        // Drop every entry whose ref object did not survive marking. The
        // payloads live outside the arenas (Rc'd register/frame snapshots),
        // so dropping them here touches no heap state; the dead ref objects
        // themselves are reclaimed by the sweep that follows.
        prune_store(&self.continuation_store, visitor);
        prune_store(&self.delimited_continuation_store, visitor);
    }
}

/// Retain only live entries, and give back bucket capacity after a churn
/// spike — `retain` never shrinks, and a table that once held thousands of
/// dead captures would otherwise be walked at full width by every future
/// collection (the §9.5 monotonic-residue problem in miniature).
fn prune_store<T>(store: &std::cell::RefCell<FxHashMap<u64, T>>, visitor: &GcVisitor<'_>) {
    let mut store = store.borrow_mut();
    store.retain(|&id, _| visitor.weak_continuation_id_is_live(id));
    if store.len() * 8 < store.capacity() {
        store.shrink_to_fit();
    }
}
