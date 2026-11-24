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
//! continuation's frames report theirs only when its payload is traced. A
//! table this traces marks the records its unit links, and with them the
//! placeholders and weak read-through records nothing else marks, and the
//! namespace the unit was linked in at load (`VmState::load_unit`). A check
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
//! The live registers are the one part of the state its derived trace
//! leaves out: [`trace_frames`] walks the frames instead, reports the
//! registers each frame's map keeps and clears the rest, in one pass
//! (#906). A capture clears its copy by the same walk ([`retire_snapshot`]),
//! and a snapshot's trace then reports its whole register file.
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

use crate::types::{CallFrame, CodeObjectId, Reg};
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

/// A collection's walk of the live frames (#906, GC_PRD §11.1 invariant 3):
/// in one pass, each frame's registers that the map at its pc keeps are
/// reported to `visitor` where they live, a run at a time, and every other
/// register of its window is overwritten with [`RETIRED`]. This is the
/// register file's whole trace: `ExecutionState`'s derived trace skips the
/// field for it, and `VmState`'s `trace_roots` calls it. Until #906 a pass
/// of its own retired the registers before each collection, and the derived
/// trace then reported the file whole.
///
/// **Clearing, not skipping.** A register the walk does not report holds no
/// reference once it returns, so nothing that reads the register file
/// afterwards, a capture's copy or the step tracer or the reader a wrong map
/// leaves, finds a pointer to an object this collection freed: a wrong map
/// retains garbage or panics at the read of a `DEAD_SLOT` (#625), and never
/// exposes a freed referent. The writes go through `&self`: a register is a
/// `Cell`, the slot the visitor is shown (#682).
///
/// It runs only inside a collection, so only where one may run: with the
/// driver, at depth 0 (#702). A nested loop collects nothing, and so clears
/// nothing: the frames beneath it are suspended in the middle of an
/// instruction, which may read its operands again when it resumes.
///
/// What [`scan_windows`] does at each frame holds here: hand-built code
/// keeps every register, a frame at an unmapped pc panics in a check build
/// and keeps every register in a plain release one, and a register outside
/// every frame's window, which #714 left none of, is reported with the
/// rest, never passed over, and refused by a debug build.
///
/// The frames' closures and code ids are not this walk's: each frame's own
/// derived trace reports them (`ExecutionState.frames`).
///
/// The census reads the stack's shape here, as the collection starts.
pub(in crate::runtime) fn trace_frames(
    registers: &[Cell<TaggedValue>],
    frames: &[CallFrame],
    visitor: &mut dyn SlotVisitor,
) {
    patina_core::census::stack_shape(frames.len(), registers.len());
    let outside = scan_windows(registers, frames, 0, |run| visitor.slots(run));
    debug_assert!(
        outside.is_none(),
        "register {outside:?} of {} lies outside every frame's window (#714, \
         PRD/GC_PRD.md §11.1, invariant 1); the walk reported it with the rest",
        registers.len()
    );
}

/// A capture's pass over its copy (#906, GC_PRD §13): the walk a collection
/// makes of the live frames ([`trace_frames`]), over the snapshot's frames
/// and with nothing to report, so that each register its frame's map calls
/// dead is overwritten with [`RETIRED`] in the copy; and with it `hole`,
/// the register of the snapshot's top frame that invoking the continuation
/// delivers into. The snapshot's own trace then reports its whole register
/// file, so no snapshot or tracer view carries a pointer nothing traces.
/// A delimited snapshot's frames keep their absolute bases and its file is
/// only its slice, which starts at `base_at_capture`.
///
/// **The hole.** The capturing call never returns a value into it, and an
/// arrival writes it before the frame runs again (`step_wind_jump`,
/// `append_delimited`), so it is dead by construction, though the frame's
/// map keeps it: a call's destination is a root from the call on. Whatever
/// it still holds would be retained for the continuation's whole life, and
/// chains: `guard`'s expansion is `((call/cc …))`, so the register holds the
/// thunk the previous `guard` delivered, which closes over its own
/// continuation, whose snapshot holds the one before. A loop catching one
/// raise an iteration retained nine heap objects an iteration for the life
/// of its frame (296 MB at 160k). [`CallFrame::NO_RETURN`] is the hole of a
/// capture made as a primitive's callback, which is in no window (#714),
/// and there is nothing to clear.
///
/// Out of line and cold, as the pass it replaces was, so that the capture
/// paths keep their shape.
#[cold]
#[inline(never)]
pub(in crate::runtime) fn retire_snapshot(
    registers: &[Cell<TaggedValue>],
    frames: &[CallFrame],
    base_at_capture: usize,
    hole: Option<Reg>,
) {
    // A register outside every window is left as it is: the snapshot's
    // trace reports the whole file, and the live stack's checks are what
    // refuse one (`trace_frames`, `debug_assert_windows_tile`).
    scan_windows(registers, frames, base_at_capture, |_run| {});
    if let (Some(top), Some(hole)) = (frames.last(), hole)
        && hole != CallFrame::NO_RETURN
    {
        registers[top.register_base - base_at_capture + hole as usize].set(RETIRED);
    }
}

/// The walk [`trace_frames`] and [`retire_snapshot`] share: for each of
/// `frames`, bottom first, hand `live` the registers of its window that the
/// map at its pc keeps, a run at a time, and overwrite the rest with
/// [`RETIRED`] ([`scan_window`]). `registers` is the file the windows lie
/// in, from `base_at_capture`: 0 for the live stack and a full snapshot,
/// and the base of a delimited snapshot's first frame.
///
/// - **Hand-built code** has no maps, and keeps every register.
/// - **A frame at a pc its code has no map for,** one no frame can be
///   suspended at (#905, GC_PRD §11.1 invariants 2 and 5), means the
///   compiler missed a suspension point: a check build panics there
///   ([`unmapped_pc`]), and a plain release build keeps every register of
///   the frame.
/// - **A register outside every frame's window,** between two windows or
///   past the top one, is handed to `live` with the rest, whole: one
///   compare a frame, and no word passed over. #714 left none, and this
///   answers the first one it met, for the collection's walk to refuse in a
///   debug build.
///
/// A frame is taken apart by name, so that a new field has to be placed.
#[inline(always)]
fn scan_windows(
    registers: &[Cell<TaggedValue>],
    frames: &[CallFrame],
    base_at_capture: usize,
    mut live: impl FnMut(&[Cell<TaggedValue>]),
) -> Option<usize> {
    let mut outside = None;
    // Where the window below ends, which is where this one starts.
    let mut end = 0;
    for frame in frames {
        let CallFrame {
            pc,
            register_base,
            num_regs,
            // The frame's closure, a slot, which the frame's own trace
            // reports (`ExecutionState.frames`, a snapshot's `frames`).
            closure: _,
            // A register number, in the caller's window.
            return_reg: _,
            code,
        } = frame;
        let base = *register_base - base_at_capture;
        if base != end {
            // A window that starts below the one under it ends leaves no
            // gap to report, and the slice is empty.
            let gap = registers.get(end..base).unwrap_or_default();
            live(gap);
            outside = outside.or(Some(end.min(base)));
        }
        end = base + *num_regs as usize;
        let window = &registers[base..end];
        // Hand-built code has no maps, and keeps every register.
        let Some(maps) = &code.register_roots else {
            live(window);
            continue;
        };
        let Some(roots) = maps.at(*pc) else {
            if GC_CHECK {
                unmapped_pc(frame);
            }
            live(window);
            continue;
        };
        scan_window(
            window,
            roots,
            switches::dropped(roots, *num_regs),
            &mut live,
        );
    }
    if let Some(past) = registers.get(end..)
        && !past.is_empty()
    {
        live(past);
        outside = outside.or(Some(end));
    }
    outside
}

/// One window against `roots`, the map of its frame's pc: hand `live` each
/// run of registers the map keeps, and overwrite every other register with
/// [`RETIRED`]. Register `r` is bit `r % 64` of word `r / 64`, and a
/// register past the map's words is dead: a tail call can reuse a window
/// larger than the new code needs, and nothing but this clears what the
/// earlier occupant left past the new code's count. `dropped` is a register
/// a wrong-map control has the walk treat as dead although the map keeps it
/// (`test_support::DropHighestLive`, #625); `None` outside the controls.
#[inline(always)]
fn scan_window(
    window: &[Cell<TaggedValue>],
    roots: &[u64],
    dropped: Option<usize>,
    live: &mut impl FnMut(&[Cell<TaggedValue>]),
) {
    // Where the run of kept registers now open starts.
    let mut run = 0;
    for (word, registers) in window.chunks(64).enumerate() {
        let kept = roots.get(word).copied().unwrap_or(0);
        for (bit, register) in registers.iter().enumerate() {
            let reg = word * 64 + bit;
            if kept & (1 << bit) == 0 || dropped == Some(reg) {
                if run < reg {
                    live(&window[run..reg]);
                }
                register.set(RETIRED);
                run = reg + 1;
            }
        }
    }
    if run < window.len() {
        live(&window[run..]);
    }
}

/// A check build's refusal of a frame found at a pc with no map (#905).
#[cold]
#[inline(never)]
fn unmapped_pc(frame: &CallFrame) -> ! {
    let after = (frame.pc.checked_sub(1)).and_then(|pc| frame.code.instructions.get(pc));
    panic!(
        "a frame of code {} ({:?}) is at pc {}, after {after:?}, which has no \
         register map: no frame can be suspended there (#905)",
        frame.code.id, frame.code.name, frame.pc
    )
}

/// The positive controls' switches, read here and at the ends of the load
/// windows (`push_top_frame`, `set_suspension`), each off unless
/// `test-support` is compiled in and a control turned it on
/// (`test_support`). Without the feature each is a constant, and the test
/// that reads it folds away.
#[cfg(not(feature = "test-support"))]
pub(in crate::runtime) mod switches {
    use crate::types::CodeObject;
    use std::rc::Rc;

    /// The register a wrong-map control drops from `roots` (#625,
    /// `test_support::DropHighestLive`): `None`, so the comparison in
    /// `scan_window` folds away.
    #[inline(always)]
    pub(in crate::runtime) fn dropped(_roots: &[u64], _num_regs: u16) -> Option<usize> {
        None
    }

    /// Whether a collection passes over a code id that names `code`, and
    /// leaves its unit's link table untraced (#836,
    /// `test_support::SkipCodeIds`): never.
    #[inline(always)]
    pub(in crate::runtime) fn skip_code_id(_code: &Rc<CodeObject>) -> bool {
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

    /// Whether a collection passes over the entries of a table it has
    /// marked traced (#836, `test_support::SkipLinkEntries`): never.
    #[inline(always)]
    pub(in crate::runtime) fn skip_link_entries() -> bool {
        false
    }
}

#[cfg(feature = "test-support")]
pub(in crate::runtime) use crate::test_support as switches;

impl RootProvider for VmState {
    /// `VmState`'s derived trace (#686), whose fields say what each holds,
    /// in a new epoch: `trace_roots` runs once, first, in each collection.
    /// Then the register file, which the derived trace leaves to the walk
    /// of the frames (#906, [`trace_frames`]): each frame's live registers
    /// reported, and its dead ones cleared.
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        self.trace_epoch.set(self.trace_epoch.get() + 1);
        Trace::trace(self, visitor);
        self.execution.trace_frames(visitor);
    }

    fn trace_code_ids(&self, ids: &[u64], visitor: &mut dyn SlotVisitor) {
        // Each id was reported by something running its code: a marked
        // closure, a frame (live or in a traced continuation), the running
        // code. Generation-checked, so an id whose unit was let go, or
        // another machine's, finds nothing; the unit's members share one
        // table, traced once this epoch. Its entries are slots, and the
        // namespace it was linked in goes to `visitor.env`.
        let epoch = self.trace_epoch.get();
        for &id in ids {
            let Some(code) = loaded(&self.code_store, CodeObjectId(id)) else {
                continue;
            };
            // A positive control's switch passes over the ids of the code
            // one kind of keeper keeps, before their table is marked traced
            // this epoch, so that `release_dead_code`'s check sees those
            // units' tables untraced (#836).
            if switches::skip_code_id(code) {
                continue;
            }
            if code.links.trace_once(epoch) {
                continue;
            }
            // A positive control's switch passes over the entries of a
            // table marked traced, so that the placeholders it links go
            // unmarked while `release_dead_code`'s check stays quiet (#836).
            if switches::skip_link_entries() {
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

/// What `test_support::collect_with_a_frame_at_an_unmapped_pc` does (#905):
/// on a fresh machine, push a frame of code whose map index has no map at
/// the frame's pc, the pc after a `LoadImmediate`, which is what a compiler
/// that missed a suspension point would leave; then collect, as a library
/// load's collection does, outside any run (`collect_for_loader`), whose
/// walk of the frames meets it. A check build panics there, naming
/// the frame ([`unmapped_pc`]). A plain release build keeps every register
/// of the frame, though the code's one map calls them all dead; this
/// answers whether both still hold what they held.
#[cfg(feature = "test-support")]
pub(crate) fn collect_with_a_frame_at_an_unmapped_pc() -> bool {
    use crate::types::code_object::{Arity, CodeObject, RegisterMaps};
    use crate::types::instruction::Instruction;
    use std::rc::Rc;

    let mut state = VmState::on_a_fresh_heap();
    let instructions = vec![
        Instruction::LoadImmediate {
            dst: 0,
            val: TaggedValue::fixnum(1),
        },
        Instruction::Move { dst: 1, src: 0 },
        Instruction::Return { val: 1 },
    ];
    let never = RegisterMaps::NEVER;
    // One map, of no live register, at every pc but 1.
    let index = vec![0, never, 0, 0].into_boxed_slice();
    let id = state.load(CodeObject {
        id: CodeObjectId::label(),
        name: Some(Rc::from("unmapped-pc-control")),
        links: patina_core::LinkTable::empty(),
        live_closures: Cell::new(0),
        instructions,
        constants: Vec::new(),
        num_regs: 2,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
        register_roots: Some(RegisterMaps::from_parts(index, Rc::from([0]), 1)),
    });
    let code = state.code_object(id).expect("the control's code is loaded");
    let base = state.execution.push_frame(code, CallFrame::NO_CLOSURE, 0);
    let held = [TaggedValue::fixnum(1), TaggedValue::fixnum(2)];
    for (reg, value) in (0..).zip(held) {
        state.execution.set_top(base, reg, value);
    }
    state.execution.set_pc(1);
    super::collect_for_loader(&mut state);
    (0..).zip(held).all(|(reg, value)| state.reg(reg) == value)
}
