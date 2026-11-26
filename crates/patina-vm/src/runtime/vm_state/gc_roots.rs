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
//! continuations, are reported as slots, where they live (#682): words of
//! the register stack (#908). Everything
//! else is pinned until a stage makes it heap data: the extents' records and
//! the code's constants at 4e, the environments at 4b; and the scratch
//! arguments and the pending escape, empty or unset at every safe point.
//!
//! The live register stack is the one part of the state its derived trace
//! leaves out: [`trace_frames`] walks the frames instead, reports each
//! frame's code and closure and the registers its map keeps, and clears the
//! rest, in one pass (#906). A capture clears its copy by the same walk
//! ([`retire_snapshot`]), and a snapshot's trace then reports its every
//! word: the headers' other words are fixnums, which a visitor passes over.
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

use crate::types::{CallFrame, CodeObjectId, FrameStack, Reg};
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
/// in one pass, each frame's code is reported by id (once for a run of
/// frames in the same code) and its closure where it lives, each of its registers that the map at its pc keeps is reported
/// where it lives, a run at a time, and every other register of its window
/// is overwritten with [`RETIRED`]. This is the register stack's whole
/// trace: `ExecutionState`'s derived trace skips the field for it, and
/// `VmState`'s `trace_roots` calls it. Until #906 a pass of its own retired
/// the registers before each collection, and the derived trace then
/// reported the file whole; until #908 the frames were a vector of their
/// own, whose derived trace reported their closures and code.
///
/// **Clearing, not skipping.** A register the walk does not report holds no
/// reference once it returns, so nothing that reads the register stack
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
/// What [`scan_frames`] does at each frame holds here: hand-built code
/// keeps every register, and a frame at an unmapped pc panics in a check
/// build and keeps every register in a plain release one. No word lies
/// outside every frame (#714): a frame is its header and its window, and
/// the walk goes from one to the next by the distance the header records,
/// which a debug build checks against the buffer first
/// (`FrameStack::assert_frames_tile`).
///
/// The census reads the stack's shape here, as the collection starts.
pub(in crate::runtime) fn trace_frames(stack: &FrameStack, visitor: &mut dyn SlotVisitor) {
    let (frames, registers) = stack.census_shape();
    patina_core::census::stack_shape(frames, registers);
    if cfg!(debug_assertions) {
        stack.assert_frames_tile();
    }
    let words = stack.words();
    // The code id last reported: frames that follow one another in the same
    // code, as a recursion's do, report it once.
    let mut reported = None;
    scan_frames(stack, |scanned| match scanned {
        Scanned::Frame(frame) => {
            let id = frame.code.id.0;
            if reported != Some(id) {
                reported = Some(id);
                visitor.code(id);
            }
            // The closure, the word below the frame's first register.
            visitor.slots(&words[frame.register_base - 1..frame.register_base]);
        }
        Scanned::Live(run) => visitor.slots(run),
    });
}

/// A capture's pass over its copy (#906, GC_PRD §13): the walk a collection
/// makes of the live frames ([`trace_frames`]), over the snapshot's frames
/// and with nothing to report, so that each register its frame's map calls
/// dead is overwritten with [`RETIRED`] in the copy; and with it `hole`,
/// the register of the snapshot's top frame that invoking the continuation
/// delivers into. The snapshot's own trace then reports its every word, so
/// no snapshot or tracer view carries a pointer nothing traces.
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
pub(in crate::runtime) fn retire_snapshot(stack: &FrameStack, hole: Option<Reg>) {
    scan_frames(stack, |_scanned| {});
    if let (Some(top), Some(hole)) = (stack.top(), hole)
        && hole != CallFrame::NO_RETURN
    {
        stack.words()[top.register_base + hole as usize].set(RETIRED);
    }
}

/// What [`scan_frames`] hands its caller: a frame, as the walk comes to
/// it, and then each run of that frame's registers its map keeps.
enum Scanned<'a> {
    Frame(&'a CallFrame<'a>),
    Live(&'a [Cell<TaggedValue>]),
}

/// The walk [`trace_frames`] and [`retire_snapshot`] share: for each frame
/// of `stack`, top first, hand `each` the frame, then the registers of its
/// window that the map at its pc keeps, a run at a time, and overwrite the
/// rest with [`RETIRED`] ([`scan_window`]).
///
/// - **Hand-built code** has no maps, and keeps every register.
/// - **A frame at a pc its code has no map for,** one no frame can be
///   suspended at (#905, GC_PRD §11.1 invariants 2 and 5), means the
///   compiler missed a suspension point: a check build panics there
///   ([`unmapped_pc`]), and a plain release build keeps every register of
///   the frame.
///
/// A frame is taken apart by name, so that a new field has to be placed.
#[inline(always)]
fn scan_frames(stack: &FrameStack, mut each: impl FnMut(Scanned<'_>)) {
    let words = stack.words();
    for frame in stack.frames() {
        each(Scanned::Frame(&frame));
        let CallFrame {
            pc,
            register_base,
            num_regs,
            // The frame's closure, a word of the stack: the caller's to
            // report for the live stack, and a snapshot's own trace's.
            closure: _,
            // A register number, in the caller's window.
            return_reg: _,
            code,
        } = frame;
        let window = &words[register_base..register_base + num_regs as usize];
        // Hand-built code has no maps, and keeps every register.
        let Some(maps) = &code.register_roots else {
            each(Scanned::Live(window));
            continue;
        };
        let Some(roots) = maps.at(pc) else {
            if GC_CHECK {
                unmapped_pc(&frame);
            }
            each(Scanned::Live(window));
            continue;
        };
        scan_window(
            window,
            roots,
            switches::dropped(roots, num_regs),
            &mut |run| each(Scanned::Live(run)),
        );
    }
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
fn unmapped_pc(frame: &CallFrame<'_>) -> ! {
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
    /// Then the register stack, which the derived trace leaves to the walk
    /// of the frames (#906, #908, [`trace_frames`]): each frame's code,
    /// closure and live registers reported, and its dead registers cleared.
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

/// The walk's own arithmetic (#906): which registers a window's scan hands
/// on and which it clears, and what the walk does with a register outside
/// every window. What a collection then keeps and frees is
/// `trace_sentinel_tests.rs`'s.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::code_object::{Arity, CodeObject};
    use std::rc::Rc;

    /// A window of `len` registers, register `r` holding the fixnum `r`.
    fn window(len: usize) -> Vec<Cell<TaggedValue>> {
        (0..len as i64)
            .map(|reg| Cell::new(TaggedValue::fixnum(reg)))
            .collect()
    }

    /// The registers `scan_window` hands to `live`, by number, in the order
    /// it hands them, with the runs it handed them in.
    fn scan(
        window: &[Cell<TaggedValue>],
        roots: &[u64],
        dropped: Option<usize>,
    ) -> (Vec<usize>, Vec<std::ops::Range<usize>>) {
        let mut reported = Vec::new();
        let mut runs = Vec::new();
        let mut live = |run: &[Cell<TaggedValue>]| {
            // Where the run lies in the window: the walk reports each
            // register where it lives, not a copy.
            let start = window
                .iter()
                .position(|slot| std::ptr::eq(slot, &run[0]))
                .expect("a run of the window's own slots");
            assert!(std::ptr::eq(&window[start..start + run.len()], run));
            reported.extend(start..start + run.len());
            runs.push(start..start + run.len());
        };
        scan_window(window, roots, dropped, &mut live);
        (reported, runs)
    }

    /// Against the definition, register by register, over windows on both
    /// sides of each word boundary, maps narrower and wider than the window,
    /// and each register dropped in turn: `scan_window` hands on exactly
    /// the registers whose bit is set and that the control does not drop,
    /// each once and in order, in maximal runs, none empty; it leaves those
    /// holding what they held; and it overwrites every other register.
    #[test]
    fn a_windows_scan_reports_what_its_map_keeps_and_clears_the_rest() {
        // A fixed sequence of words, of every density.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut maps: Vec<Vec<u64>> = vec![
            vec![0],
            vec![u64::MAX],
            vec![u64::MAX, u64::MAX, u64::MAX],
            vec![1 << 63, 1],
            vec![!(1 << 63), !1],
        ];
        for words in 1..=3 {
            for _ in 0..8 {
                maps.push((0..words).map(|_| next()).collect());
                maps.push((0..words).map(|_| next() & next()).collect());
                maps.push((0..words).map(|_| next() | next()).collect());
            }
        }
        for len in [0, 1, 2, 3, 17, 63, 64, 65, 127, 128, 129, 130, 200] {
            for roots in &maps {
                let kept = |reg: usize| {
                    roots
                        .get(reg / 64)
                        .is_some_and(|word| word & (1 << (reg % 64)) != 0)
                };
                for dropped in std::iter::once(None).chain((0..len).map(Some)) {
                    let registers = window(len);
                    let (reported, runs) = scan(&registers, roots, dropped);
                    let expected: Vec<usize> = (0..len)
                        .filter(|&reg| kept(reg) && dropped != Some(reg))
                        .collect();
                    assert_eq!(
                        reported, expected,
                        "{len} registers, {roots:x?}, {dropped:?}"
                    );
                    for pair in runs.windows(2) {
                        assert!(
                            pair[0].end < pair[1].start,
                            "runs {runs:?} are not maximal: {len} registers, {roots:x?}"
                        );
                    }
                    assert!(runs.iter().all(|run| !run.is_empty()));
                    for (reg, slot) in registers.iter().enumerate() {
                        let held = if expected.contains(&reg) {
                            TaggedValue::fixnum(reg as i64)
                        } else {
                            RETIRED
                        };
                        assert_eq!(slot.get(), held, "r{reg} of {len}, {roots:x?}, {dropped:?}");
                    }
                }
            }
        }
    }

    /// Hand-built code, which has no maps and keeps every register.
    fn code(num_regs: u16) -> Rc<CodeObject> {
        Rc::new(CodeObject {
            id: CodeObjectId::label(),
            name: None,
            links: patina_core::LinkTable::empty(),
            live_closures: Cell::new(0),
            instructions: Vec::new(),
            constants: Vec::new(),
            num_regs,
            arity: Arity::Fixed(0),
            source_map: Vec::new(),
            register_roots: None,
        })
    }

    /// A collection's walk reports, for every frame, its code by id, its
    /// closure and its registers, each where it lives, and nothing else:
    /// the header's other words are not values, and no word lies outside a
    /// frame (#714, #908).
    #[test]
    fn the_walk_reports_each_frames_code_closure_and_registers() {
        #[derive(Default)]
        struct Seen {
            slots: Vec<i64>,
            codes: Vec<u64>,
        }
        impl SlotVisitor for Seen {
            fn slot(&mut self, slot: &Cell<TaggedValue>) {
                self.slots.push(slot.get().as_fixnum().unwrap());
            }
            fn pinned(&mut self, _value: TaggedValue) {}
            fn env(&mut self, _env: &Rc<patina_core::environment::Environment>) {}
            fn continuation(&mut self, _k: &Rc<patina_core::CpsContinuation>) {}
            fn cont_env(&mut self, _cont_env: &patina_core::cont_value::ContEnv) {}
            fn expr_literals(&mut self, _expr: &patina_core::CpsExpr) {}
            fn code(&mut self, id: u64) {
                self.codes.push(id);
            }
        }
        // Two frames, of 4 and 3 registers. Each closure word and register
        // holds a fixnum of its own: 100 and 200 for the closures, and the
        // registers 0 to 6 from the bottom.
        let (lower, upper) = (code(4), code(3));
        let mut stack = FrameStack::default();
        let mut next = 0;
        for (code, closure) in [(&lower, 100), (&upper, 200)] {
            let n = code.num_regs;
            let base = stack.push(code, TaggedValue::fixnum(closure), 0, n, TaggedValue::NULL);
            for slot in &stack.words()[base..base + n as usize] {
                slot.set(TaggedValue::fixnum(next));
                next += 1;
            }
        }
        let mut seen = Seen::default();
        trace_frames(&stack, &mut seen);
        // The top frame first: its closure, then its registers.
        assert_eq!(seen.slots, [200, 4, 5, 6, 100, 0, 1, 2, 3]);
        assert_eq!(seen.codes, [upper.id.0, lower.id.0]);

        // A capture's pass reports nothing and, for hand-built code, clears
        // only the hole, in the top frame.
        retire_snapshot(&stack, Some(1));
        let words: Vec<_> = stack.words().iter().map(|slot| slot.get()).collect();
        assert_eq!(words[stack.base() + 1], RETIRED);
        assert_eq!(
            words.iter().filter(|&&word| word == RETIRED).count(),
            1,
            "only the hole is cleared"
        );
        retire_snapshot(&FrameStack::default(), Some(1));
    }
}
