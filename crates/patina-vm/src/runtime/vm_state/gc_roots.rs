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
//! A third is each unit's link table (#836), which the code store never
//! traces (`CodeObject::links`): a frame's code (`FrameCode`), the running
//! code (`RunningCode`) and a marked closure report their code's id, the
//! collector queues each id once, and `trace_code_ids` traces the table of
//! the unit each one names, once a collection (`LinkTable::trace_once`, by
//! `trace_epoch`), in the same fixpoint as the continuation ids, since a
//! continuation's frames report theirs only when its payload is traced. The
//! VM does not link its units at load yet, so a table this traces has every
//! entry `UNBOUND` and no namespace, and only the tests link one. A check
//! build asserts after each collection that the table of every unit it kept
//! was traced (`VmState::release_dead_code`, with `LinkTable::traced_in`):
//! whatever keeps a unit can run its code, and so reports its id.
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
//! tracing — see `NoGcScope` and §7. The environments a library's load set
//! aside were too, until they moved onto a traced stack (#677), which went
//! when the load stopped swapping the machine's globals (#823).

use patina_core::{GC_CHECK, GcVisitor, RootProvider, SlotVisitor, Trace};
use rustc_hash::FxHashMap;

use crate::types::{CallFrame, CodeObjectId};
use patina_core::TaggedValue;
use std::cell::Cell;

use super::{VmState, loaded};

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
        let dropped = switches::dropped(roots, frame.num_regs);
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

/// The positive controls' switches, read here and at the ends of the load
/// windows (`execute`, `push_frame_in`, `set_suspension`), each off unless
/// `test-support` is compiled in and a control turned it on
/// (`test_support`). Without the feature each is a constant, and the test
/// that reads it folds away.
#[cfg(not(feature = "test-support"))]
pub(in crate::runtime) mod switches {
    /// The register a wrong-map control drops from `roots` (#625,
    /// `test_support::DropHighestLive`): `None`, so the comparison in
    /// `retire_registers` folds away.
    #[inline(always)]
    pub(in crate::runtime) fn dropped(_roots: &[u64], _num_regs: u16) -> Option<usize> {
        None
    }

    /// Whether a collection traces no unit's link table (#836,
    /// `test_support::SkipCodeIds`): never.
    #[inline(always)]
    pub(in crate::runtime) fn skip_code_ids() -> bool {
        false
    }

    /// Whether the machine checks for an open window just before a loaded
    /// unit's frame is pushed (#836, `test_support::PollInWindow`): never.
    #[inline(always)]
    pub(in crate::runtime) fn poll_before_frame_push() -> bool {
        false
    }

    /// Whether it checks just before an `eval`'s closure is written into
    /// `CALLEE` (#836, `test_support::PollInWindow`): never.
    #[inline(always)]
    pub(in crate::runtime) fn poll_before_callee_write() -> bool {
        false
    }
}

#[cfg(feature = "test-support")]
pub(in crate::runtime) use crate::test_support as switches;

impl RootProvider for VmState {
    /// `VmState`'s derived trace (#686), whose fields say what each holds,
    /// in a new epoch: `trace_roots` runs once, first, in each collection.
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        self.trace_epoch.set(self.trace_epoch.get() + 1);
        Trace::trace(self, visitor);
    }

    fn trace_code_ids(&self, ids: &[u64], visitor: &mut dyn SlotVisitor) {
        // Each id was reported by something running its code: a marked
        // closure, a frame (live or in a traced continuation), the running
        // code. Generation-checked, so an id whose unit was let go, or
        // another machine's, finds nothing; the unit's members share one
        // table, traced once this epoch. Its entries are slots, and the
        // namespace it was linked in goes to `visitor.env`.
        //
        // A positive control's switch skips every table, before any is
        // marked traced this epoch, so that `release_dead_code`'s check sees
        // the kept units' tables untraced (#836).
        if switches::skip_code_ids() {
            return;
        }
        let epoch = self.trace_epoch.get();
        for &id in ids {
            let Some(code) = loaded(&self.code_store, CodeObjectId(id)) else {
                continue;
            };
            if code.links.trace_once(epoch) {
                continue;
            }
            code.links.trace(visitor);
        }
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
