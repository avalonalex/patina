//! The coupled components in VM_RUNTIME §5.6, and the id of the code the
//! running dispatch loop runs: the register stack, whose frames and
//! registers are one buffer of words since GC stage 4d.7b
//! (`types::frame_stack`, #908), and the three extent stacks. Only this
//! module may push, pop, cut or replace the live stack's frames, or replace
//! a dynamic-state snapshot. Transfer policy (wind travel, re-entry, error
//! routing) stays in `control`.

use super::control::abort_step;
use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject};
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::frame_stack::{Buffer, Frames, LiveStack, NoRoom};
use crate::types::{CallFrame, Depth, FrameStack};
use patina_core::heap::Heap;
use patina_core::{Carrier, CheckCell, GC_CHECK, SlotVisitor, TaggedValue, Trace};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

/// Its trace is every component and the running code's id, a root of
/// every collection through `VmState`'s: derived (#686) but for the
/// register stack, which the walk of the frames reports
/// ([`ExecutionState::trace_frames`], #906). The sentinel test
/// `every_vm_state_root_is_traced` pins each component,
/// `a_frame_s_live_registers_are_kept_and_its_dead_ones_cleared` the walk
/// under a map, and `the_running_code_keeps_its_units_link_table` the
/// running code.
#[derive(Trace)]
pub(super) struct ExecutionState {
    /// The frames and their registers, interleaved (#908), in a
    /// reservation of address space the size of the stack's cap, which
    /// never moves (7c). Its words are
    /// slots, so that the root provider reports each register where it
    /// lives, and clears one, through `&self` (#682). Not the derived
    /// trace's: a collection walks the frames, reports each frame's code
    /// and closure and the registers its map keeps at its pc, a run at a
    /// time, and overwrites the completed expression temporaries the map
    /// leaves out (#423, #906). A continuation snapshot is cleared the same
    /// way at capture, and its trace reports its every word.
    #[trace(
        skip,
        reason = "reported by the walk of the frames, which `VmState`'s `trace_roots` runs after the derived trace: each frame's code, closure and live registers, the rest cleared (`gc_roots::trace_frames`, #906)"
    )]
    stack: LiveStack,
    prompt_stack: Vec<PromptFrame>,
    dynamic_winds: Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
    /// The code the innermost running dispatch loop runs, by id (#836).
    running: RunningCode,
    /// Which top frame awaits a value, for the check builds' assertions on
    /// the register writes (#904).
    #[trace(skip, reason = "a stack depth, which only a check build writes")]
    awaiting: Awaiting,
    /// The value [`ExecutionState::return_into`] was last handed for
    /// [`CallFrame::NO_RETURN`], which writes no register (#714): a callee
    /// that answered here and now a call that asked for no destination
    /// (`run_apply_proc`'s), which `call_any` takes back at once
    /// ([`ExecutionState::take_answer`]), with no safe point between. A
    /// value a frame returned into nothing (a continuation that re-entered
    /// a callback after its primitive returned) stays until the next one
    /// replaces it, or the stack goes (`clear`). Pinned, as
    /// `VmState::pending_escape` is.
    answer: Cell<Option<TaggedValue>>,
}

/// A check build's record of the top frame that awaits a value (#904): the
/// depth of the stack whose top frame something has returned into, or
/// [`Depth::BASE`] when none does. [`ExecutionState::return_into`] writes
/// that frame, and [`ExecutionState::set_top`] may not until the frame
/// starts its own work again.
///
/// Set where the top frame comes to await a value: where the stack shrinks
/// under it or is reinstated (`pop_frame`, `truncate_to_prompt`, `restore`,
/// `append_delimited`), and where it calls
/// a callee that may answer here and now, which a callback's frames may
/// have run above it before then ([`ExecutionState::mark_awaiting`]).
/// Cleared where the top frame starts its own work (`dispatch_frame`,
/// `restart_stub`) and where the stack goes (`clear`). A push leaves it, and
/// every shrink sets it, so it is `BASE` or no deeper than the stack, and
/// equal to the stack's depth only while its top frame awaits a value.
///
/// A [`CheckCell`], since the VM cannot name core's `gc-check` feature in a
/// `#[cfg]`: zero-sized in a plain release build, so `ExecutionState`, and
/// `VmState` around it, keep that build's layout. A word held in every
/// build reordered `VmState` and split the dispatch loop's paired loads
/// (#904's measurement).
struct Awaiting(CheckCell<Depth>);

impl Default for Awaiting {
    fn default() -> Self {
        Awaiting(CheckCell::new(Depth::BASE))
    }
}

// A plain build keeps no word for it.
const _: () = assert!(GC_CHECK || std::mem::size_of::<Awaiting>() == 0);

/// What a push writes into every register of the window it initializes that
/// no argument or operand of its own fills (#714, `PRD/GC_PRD.md` §11.1,
/// frame invariant 1): a fresh window whole (`push_frame`, `push_abort_stub`),
/// the registers a reused window grows by, and a stub's next round past the
/// registers it keeps (`restart_stub`). A plain release build writes `NULL`,
/// which `push_frame` always wrote. A check build writes `DEAD_SLOT`, which
/// the machine refuses to read (`VmState::reg_at`, #625), so a read of a
/// register the activation never wrote panics there, rather than answering
/// what an earlier occupant of the window left.
///
/// A tail call's reused window (`tail_replace`, `restart_closure`) is filled
/// past its arguments in a check build only ([`fill_reused`]).
const FILL: TaggedValue = if GC_CHECK {
    TaggedValue::DEAD_SLOT
} else {
    TaggedValue::NULL
};

/// [`FILL`], through an opaque copy (`std::hint::black_box`), so that a
/// fill stays a run of inline stores. With the constant, LLVM turns each
/// fill loop, and `Vec::resize`'s, into a call of `memset_pattern16` on
/// macOS, which cost a tail call into a window of 16 registers some 55
/// instructions, and every push the same; measured 2026-11-22 (#714).
#[inline(always)]
fn fill_value() -> TaggedValue {
    std::hint::black_box(FILL)
}

/// Write [`FILL`] into every register of `window`: one store each, with no
/// test per register.
#[inline(always)]
fn fill(window: &[Cell<TaggedValue>]) {
    let value = fill_value();
    for slot in window {
        slot.set(value);
    }
}

/// [`fill`], in a check build only: for the registers past the arguments in
/// the window a tail call reuses (`tail_replace`, `restart_closure`). The
/// owner's ruling of 2026-11-26 on #714's Q-4d3-1, option (b): the stores
/// cost loop- and tail-call-heavy code 2.5–4% of its instructions (measured
/// 2026-11-25), and a plain release build is sound without them.
///
/// In a plain release build such a register keeps the earlier occupant's
/// word until the new activation writes it, or until the next collection
/// or capture, whose walk of the frames clears every register the frame's
/// map calls dead (`PRD/GC_PRD.md` §11.1, invariant 3). The word is never a
/// freed object's: nothing is freed between the reuse and that collection,
/// and the collection clears the register, or traces it where the frame's
/// pc has no map. Correct code reads no register it has not written, which
/// the check builds hold every test to, since there the fill is `DEAD_SLOT`.
/// What it gives up is the answer a compiler's unwritten read gets in a
/// plain release build: the earlier occupant's word, where it was `NULL`.
///
/// The caller passes the window's bounds and not a slice, so that a plain
/// release build does not check them either.
#[inline(always)]
fn fill_reused(words: &[Cell<TaggedValue>], from: usize, to: usize) {
    if GC_CHECK {
        fill(&words[from..to]);
    }
}

/// The registers a call's arguments occupy in a window of code of `arity`:
/// the fixed parameters, and for a variadic one the register of its rest
/// list after them, which `store_args_in_window` writes. A reused window is
/// filled from here up, in a check build.
#[inline(always)]
fn argument_slots(arity: Arity) -> usize {
    match arity {
        Arity::Fixed(n) => n as usize,
        Arity::Variadic(n) => n as usize + 1,
    }
}

/// The code the innermost running dispatch loop's `cur_code` holds (#836),
/// by id: written when `cur_code` changes (where a loop starts or an escape
/// lands, `VmState::current_code`, and where a dispatch refreshes it,
/// [`ExecutionState::dispatch_frame`]), and put back by
/// `run_loop_until_outcome` when a nested loop ends. A tail call gives the
/// frame to its callee (`tail_replace`), or pops it to deliver a tail-called
/// primitive's, parameter's or control primitive's value, and then polls,
/// at the callee's entry or on the call's return, before any dispatch
/// refreshes `cur_code`: the code it left is then held only by `cur_code`
/// and the code store, and a collection there still traces that code's
/// unit's table, since this reports the id as a frame's code does. Written
/// once per change of `cur_code`, never per instruction, and not on a call
/// or return that keeps the same code. `RETIRED_VM_CLOSURE_CODE`, which
/// marking passes over, while no loop runs.
pub(super) struct RunningCode(Cell<u64>);

impl Default for RunningCode {
    fn default() -> Self {
        RunningCode(Cell::new(Heap::RETIRED_VM_CLOSURE_CODE))
    }
}

/// Its one field by name (`scripts/check_gc_trace_names.py`).
impl Trace for RunningCode {
    fn trace<V: SlotVisitor + ?Sized>(&self, visitor: &mut V) {
        let RunningCode(id) = self;
        visitor.code(id.get());
    }
}

impl ExecutionState {
    /// A state with nothing on it, whose register stack may reach `cap`
    /// bytes: its reservation is made here, of that size or of as much of
    /// it as the system gives, down to `floor` bytes
    /// ([`Self::stack_capacity`] says which).
    pub(super) fn with_stack_cap(cap: usize, floor: usize, account: Rc<Carrier>) -> Self {
        ExecutionState {
            stack: LiveStack::reserved_on(cap, floor, account),
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            exception_handlers: Vec::new(),
            running: RunningCode::default(),
            awaiting: Awaiting::default(),
            answer: Cell::new(None),
        }
    }

    /// Give the register stack a reservation for a cap of `cap` bytes in
    /// place of the one it has, for a test that wants a small cap. The
    /// stack holds no frame.
    #[cfg(test)]
    pub(super) fn reserve_stack(&mut self, cap: usize, floor: usize, account: Rc<Carrier>) {
        assert!(
            self.stack.is_empty(),
            "a stack with frames keeps its reservation"
        );
        self.stack = LiveStack::reserved_on(cap, floor, account);
    }

    /// An observation of the register stack, which a collection is (#909,
    /// `PRD/GC_PRD.md` §17.1): its pages above what it has reached since
    /// the observation before go back to the system
    /// (`LiveStack::observe`). The machine calls it after every
    /// collection, outside the pause.
    pub(super) fn observe_stack(&mut self) {
        self.stack.observe();
    }

    /// The bytes of the register stack's reservation that are committed.
    #[cfg(test)]
    pub(super) fn stack_committed(&self) -> usize {
        self.stack.committed_bytes()
    }

    /// The bytes the register stack's reservation holds: the cap it was
    /// asked for rounded up to a page, or less where the system refused
    /// that.
    pub(super) fn stack_capacity(&self) -> usize {
        self.stack.capacity_bytes()
    }

    /// The address a stack `bytes` deep has its top at: what the poll word
    /// holds for a limit of `bytes` (`PRD/GC_PRD.md` §12). The stack's base
    /// never moves, so the address holds for the machine's life. A limit
    /// past every address is the largest one.
    #[inline]
    pub(super) fn stack_address(&self, bytes: usize) -> usize {
        self.stack.base_address().saturating_add(bytes)
    }

    /// Build a state from its components, for tests that fill each with a
    /// value nothing else holds, with no running code. A struct literal, so
    /// a new component fails to compile here too. The stack is copied into
    /// a reservation of its own.
    #[cfg(test)]
    pub(super) fn from_parts(
        stack: FrameStack,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Self {
        let mut live = LiveStack::reserved(64 << 20, 2 << 20);
        live.copy_from(&stack).expect("a test's stack fits");
        ExecutionState {
            stack: live,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
            running: RunningCode::default(),
            awaiting: Awaiting::default(),
            answer: Cell::new(None),
        }
    }

    /// The id of the code the innermost running dispatch loop runs
    /// ([`RunningCode`]).
    #[inline(always)]
    pub(super) fn running(&self) -> u64 {
        self.running.0.get()
    }

    /// Record that the running dispatch loop now runs the code `id`.
    #[inline(always)]
    pub(super) fn set_running(&self, id: u64) {
        self.running.0.set(id);
    }

    /// The register stack's words, lent for reading ([`Registers`]): a
    /// frame's registers are the words from its `register_base`. The writes
    /// are this module's: `set_top`, `return_into` and a push's (#904).
    #[inline(always)]
    pub(super) fn registers(&self) -> Registers<'_> {
        Registers(self.stack.words())
    }
    /// The register stack itself, for the tests that read a window or a
    /// header's word where it lives.
    #[cfg(test)]
    pub(super) fn stack(&self) -> &LiveStack {
        &self.stack
    }
    /// The frames, top first, each as a view of its header.
    #[inline(always)]
    pub(super) fn frames(&self) -> Frames<'_> {
        self.stack.frames()
    }
    /// The top frame, as a view of its header.
    #[inline(always)]
    pub(super) fn top_frame(&self) -> Option<CallFrame<'_>> {
        self.stack.top()
    }
    /// How many frames the stack holds, which the step tracer reports
    /// while one is attached. Nothing in the machine's control reads it: a
    /// depth is a byte offset ([`Self::depth`]). It is a walk of the
    /// frames (`FrameStack::frame_count`).
    pub(super) fn frame_count(&self) -> usize {
        self.stack.frame_count()
    }
    /// Whether the stack holds no frame.
    #[inline(always)]
    pub(super) fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
    /// Whether a frame lies below the top one.
    #[inline(always)]
    pub(super) fn has_caller(&self) -> bool {
        self.stack.caller_base().is_some()
    }
    /// The closure the top frame runs, if it runs one, for the dispatch
    /// loop, which has the frame's `base` in hand (`LoadClosure`): the
    /// read is one range check.
    #[inline(always)]
    pub(super) fn closure_of_top(&self, base: usize) -> Option<TaggedValue> {
        debug_assert_eq!(base, self.top_base());
        Some(self.stack.closure_at(base)).filter(|closure| closure.is_object())
    }
    /// The top frame's code.
    #[inline(always)]
    pub(super) fn top_code(&self) -> Option<&Rc<CodeObject>> {
        self.stack.top_code()
    }
    #[inline(always)]
    pub(super) fn prompts(&self) -> &[PromptFrame] {
        &self.prompt_stack
    }
    #[inline(always)]
    pub(super) fn winds(&self) -> &[DynamicWindRecord] {
        &self.dynamic_winds
    }
    #[inline(always)]
    pub(super) fn handlers(&self) -> &[ExceptionHandler] {
        &self.exception_handlers
    }

    /// How deep the register stack is: the one reading of its depth (#903).
    /// Prompts, handlers and delimited captures record it, and the dispatch
    /// loops stop at it. The stack's top as a byte offset from its base
    /// (`PRD/GC_PRD.md` §11.1) since GC stage 4d.7b (#908); a count of
    /// frames before.
    #[inline(always)]
    pub(super) fn depth(&self) -> Depth {
        Depth::of_bytes(self.stack.bytes())
    }

    /// What the stack holds in bytes (#907, `PRD/GC_PRD.md` §11.1): 40 a
    /// frame and 8 a register, which is its buffer's length. The cap and
    /// the soft limit are counted in it.
    #[inline(always)]
    pub(super) fn top_bytes(&self) -> usize {
        self.stack.bytes()
    }

    /// The address of the stack's top, which the frame-entry check
    /// compares with the poll word (`PRD/GC_PRD.md` §12; #908's 7c).
    #[inline(always)]
    pub(super) fn top_address(&self) -> usize {
        self.stack.top_address()
    }

    /// The depth the top frame was pushed at: where its header starts, or
    /// [`Depth::BASE`] with no frame.
    #[inline(always)]
    pub(super) fn top_frame_depth(&self) -> Depth {
        Depth::of_bytes(self.stack.top_frame_bytes())
    }

    // ── Register writes (#904) ──────────────────────────────────────────
    //
    // The only ways to write a live register: the top frame's own write
    // (`set_top`), a write into a frame something has returned into or one
    // below the top (`return_into`), and a push's initializing writes
    // (`push_frame`, `push_abort_stub`, the fill of a reused window in
    // `restart_stub` and, in a check build, in `tail_replace` and
    // `restart_closure`, #714, and the arguments the call paths copy into a
    // callee's window, `init_arg` and `init_args`).
    // `inline(always)`, as the register helpers they replace were:
    // `dispatch_one_instruction` is too large for LLVM to inline them
    // otherwise (PRD §1.6 finding 1).

    /// The top frame's window base.
    #[inline(always)]
    pub(super) fn top_base(&self) -> usize {
        let base = self.stack.base();
        assert!(base != 0, "no active frame");
        base
    }

    /// Assert, in a debug build, that no word lies above the top frame's
    /// window (#714, `PRD/GC_PRD.md` §11.1, invariant 1): the only place a
    /// word outside every frame could arise, since a push puts its frame at
    /// the end of the stack and a pop cuts the stack at the frame's header.
    /// Checked at each push, before it extends the stack, at each pop, and
    /// where `return_into` relies on it.
    #[inline(always)]
    fn debug_assert_file_ends_at_top(&self) {
        debug_assert_eq!(
            self.stack.words().len(),
            self.stack.top_end(),
            "a word lies past the top frame's window, outside every frame \
             (#714, PRD/GC_PRD.md §11.1, invariant 1)"
        );
    }

    /// Assert, in a debug build, that every word lies within a frame
    /// (#714, `PRD/GC_PRD.md` §11.1, invariant 1): the frames tile the
    /// stack from its base, each header beginning where the window below
    /// ends, and the top window ends the stack. O(depth), so checked where
    /// words were kept outside every window until 4d.3: after a wind step
    /// (`ResumeWindJump`) and after a higher-order primitive's callback
    /// (`run_apply_proc`).
    pub(super) fn debug_assert_windows_tile(&self) {
        if cfg!(debug_assertions) {
            self.stack.assert_frames_tile();
        }
    }

    /// Record, in a check build, that the top frame awaits a value
    /// ([`Awaiting`]): it has called a callee that may answer here and now,
    /// through [`Self::return_into`], or something above it has just
    /// returned or a transfer has reinstated it. With no frame, none does.
    #[inline(always)]
    pub(super) fn mark_awaiting(&self) {
        if GC_CHECK {
            self.awaiting.0.set(self.depth());
        }
    }

    /// Record, in a check build, that the top frame starts its own work.
    #[inline(always)]
    fn clear_awaiting(&self) {
        if GC_CHECK {
            self.awaiting.0.set(Depth::BASE);
        }
    }

    /// Whether the top frame awaits a value. Kept only by a check build: a
    /// plain one answers `false`.
    fn top_awaits(&self) -> bool {
        self.awaiting
            .0
            .get()
            .is_some_and(|awaiting| awaiting != Depth::BASE && awaiting == self.depth())
    }

    /// The one way to write a register of a frame something has returned
    /// into, or of a frame below the top (`PRD/GC_PRD.md` §11.1): a value
    /// delivered after a pop (`Return`, a tail call's delivery), a callee's
    /// answer once its callback's frames have run above the caller, a jump's
    /// or a composable invoke's arrival into the frame it reinstated, and the
    /// register `deliver_before_collecting` clears in the frame a stub
    /// returns into.
    ///
    /// At 4d it writes and nothing more. Stage 7 adds the stack watermark
    /// here: it lowers the watermark below `frame` first, and sets the
    /// thread's `ran_since_gc`, so that a minor collection that scans only
    /// the frames above the watermark sees every reference written below it.
    ///
    /// A check build asserts that `frame` is below the top, or is a top frame
    /// that awaits a value ([`Awaiting`]).
    ///
    /// **The no-destination form** (#714): `reg` is [`CallFrame::NO_RETURN`]
    /// for the value of a call that asked for none, a higher-order
    /// primitive's callback (`run_apply_proc`). It writes no register: the
    /// value goes to [`Self::take_answer`], for a callee that answered here
    /// and now, and otherwise nowhere. It is the same choke point, asserted
    /// alike, since the frame something returned into awaits its value even
    /// when the value goes nowhere, and stage 7 lowers the watermark for the
    /// activation as for a write.
    #[inline(always)]
    pub(super) fn return_into(&mut self, frame: ReturnFrame, reg: u16, value: TaggedValue) {
        let base = match frame {
            ReturnFrame::Top => self.top_base(),
            ReturnFrame::TopAt(base) => {
                debug_assert_eq!(base, self.top_base());
                base
            }
            ReturnFrame::Caller => self.stack.caller_base().expect("no frame below the top"),
        };
        if GC_CHECK && frame != ReturnFrame::Caller && !self.top_awaits() {
            returned_into_a_running_frame(reg);
        }
        self.debug_assert_file_ends_at_top();
        // The top frame's window ends the register stack (`PRD/GC_PRD.md`
        // §11.1, invariant 1, asserted at each push), and no window reaches
        // `NO_RETURN`, so the bounds check every write makes is what tells
        // the no-destination form apart, at no cost on a `Return`: its cold
        // arm takes the place of the panic's. The frame below the top is
        // tested by name, since its base plus `NO_RETURN` can fall in the
        // top frame.
        match self.stack.words().get(base + reg as usize) {
            Some(slot) if frame != ReturnFrame::Caller || reg != CallFrame::NO_RETURN => {
                slot.set(value)
            }
            _ => self.no_destination(reg, value),
        }
    }

    /// The no-destination form of [`Self::return_into`], a write past the
    /// end of the register stack: hand `value` to [`Self::take_answer`], or
    /// panic, as indexing would have, for any register but
    /// [`CallFrame::NO_RETURN`].
    #[cold]
    #[inline(never)]
    fn no_destination(&self, reg: u16, value: TaggedValue) {
        assert_eq!(
            reg,
            CallFrame::NO_RETURN,
            "return_into wrote r{reg}, past the end of the register stack"
        );
        self.answer.set(Some(value));
    }

    /// The value a callee answered here and now to a call whose destination
    /// was [`CallFrame::NO_RETURN`] (#714): what [`Self::return_into`]'s
    /// no-destination form was last handed, taken, so that nothing stays.
    /// For `call_any` and the tail path of a control primitive, which read
    /// a register here for any other destination.
    pub(super) fn take_answer(&self) -> TaggedValue {
        self.answer
            .take()
            .expect("a callee that answered here and now answers through return_into")
    }

    /// The top frame's own write, during its own instruction: `reg` of the
    /// window at `base`, the base the dispatch prologue hoisted, or the one
    /// `push_frame` answered for a stub's operands. Only an instruction arm
    /// that cannot push or pop a frame before the write may pass a hoisted
    /// base; the debug assert holds it to the top frame's. A check build
    /// asserts that the top frame awaits no value ([`Awaiting`]): a write
    /// into one is [`Self::return_into`]'s.
    #[inline(always)]
    pub(super) fn set_top(&mut self, base: usize, reg: u16, value: TaggedValue) {
        debug_assert_eq!(base, self.top_base());
        if GC_CHECK && self.top_awaits() {
            wrote_an_awaiting_frame("set_top", reg as usize);
        }
        self.stack.words()[base + reg as usize].set(value);
    }

    /// A push's initializing write: argument `index` into the window at
    /// `base`, of the frame just pushed (`push_frame`) or given to a callee
    /// in place (`tail_replace`, `restart_closure`).
    #[inline(always)]
    pub(super) fn init_arg(&mut self, base: usize, index: usize, value: TaggedValue) {
        debug_assert_eq!(base, self.top_base());
        if GC_CHECK && self.top_awaits() {
            wrote_an_awaiting_frame("init_arg", index);
        }
        self.stack.words()[base + index].set(value);
    }

    /// [`Self::init_arg`] for the first `slots` registers of that window,
    /// from `values`, as many as both have, under one range check.
    #[inline(always)]
    pub(super) fn init_args(&mut self, base: usize, slots: usize, values: &[TaggedValue]) {
        debug_assert_eq!(base, self.top_base());
        if GC_CHECK && self.top_awaits() {
            wrote_an_awaiting_frame("init_args", 0);
        }
        for (slot, &value) in self.stack.words()[base..base + slots].iter().zip(values) {
            slot.set(value);
        }
    }

    /// A copy of the frames below `depth`, as a stack of its own: what a
    /// stack cut back to it keeps, for the landing an abort describes
    /// before it travels there.
    pub(super) fn stack_below(&self, depth: Depth) -> FrameStack {
        self.stack.below(depth.bytes())
    }

    /// A copy of the frames at and above `depth`, as a stack of its own:
    /// what a capture from it takes.
    pub(super) fn stack_above(&self, depth: Depth) -> FrameStack {
        self.stack.above(depth.bytes())
    }

    /// Enter one loaded code object with its matching window, every register
    /// of it [`FILL`]ed (#714). Caller fills operands before the next safe
    /// point. Dynamic extents are unchanged. Refused, with nothing written,
    /// where the frame would end past the stack's reservation
    /// (`VmState::push_frame` answers that as the cap).
    pub(super) fn push_frame(
        &mut self,
        code: Rc<CodeObject>,
        closure: TaggedValue,
        return_reg: u16,
    ) -> Result<usize, NoRoom> {
        self.push_closure_frame(&code, closure, return_reg)
    }

    /// [`Self::push_frame`], in line: for the closure call paths, where the
    /// push is most of what a call does. The stubs' pushes call the one
    /// copy above. The code is borrowed, from the store on a call's path
    /// (`VmState::loaded_code_and_execution`): the stack takes a reference
    /// of its own only where it has none for the frame's run
    /// (`FrameStack`'s comment).
    #[inline(always)]
    pub(super) fn push_closure_frame(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        return_reg: u16,
    ) -> Result<usize, NoRoom> {
        self.debug_assert_file_ends_at_top();
        self.stack
            .push(code, closure, return_reg, code.num_regs, fill_value())
    }

    /// Remove a frame and its window together, as every frame goes (a wind
    /// step's frame too, since #714: it stays for its whole travel, and
    /// arrival replaces it). Extents stay live until value delivery: a
    /// tail-called raise must still find its caller's handler.
    #[inline(always)]
    pub(super) fn pop_frame(&mut self) -> Popped {
        let return_reg = self.stack.pop();
        self.debug_assert_file_ends_at_top();
        self.mark_awaiting();
        Popped { return_reg }
    }

    /// Let go of the code the stack holds for no frame: the store calls
    /// this before it asks whether a frame holds a code object
    /// (`FrameStack::drop_spent`).
    pub(super) fn drop_spent_code(&mut self) {
        self.stack.drop_spent();
    }

    /// Reuse the top window for another closure, a tail call's push: the
    /// window grows to the new code's registers if it needs more, the
    /// registers it grows by [`FILL`]ed. In a check build every register of
    /// the new code past its arguments' is filled too (#714,
    /// [`fill_reused`]), so a read of one the new activation never wrote
    /// panics; in a plain release build those keep what the replaced
    /// activation left, until the new one writes them or a collection or
    /// capture clears them. The caller writes the arguments
    /// (`store_args_in_window`), into the registers the fill leaves alone.
    /// A window larger than the new code needs keeps its spare registers,
    /// which the new code cannot name and the walk of the frames clears at
    /// every collection and capture. Refused, with the frame as it was,
    /// where a window that must grow would end past the stack's
    /// reservation.
    #[inline(always)]
    pub(super) fn tail_replace(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
    ) -> Result<usize, NoRoom> {
        if GC_CHECK && self.top_awaits() {
            wrote_an_awaiting_frame("tail_replace", 0);
        }
        let base = self.top_base();
        self.stack.replace_top(code, closure, fill_value())?;
        fill_reused(
            self.stack.words(),
            base + argument_slots(code.arity),
            base + code.num_regs as usize,
        );
        Ok(base)
    }

    /// Give the top window to another activation of its own code, a self
    /// tail call's push. In a check build every register of the code past
    /// its arguments' is [`FILL`]ed (#714, [`fill_reused`]); a plain
    /// release build writes none of them. The caller has written the
    /// arguments, which the fill leaves alone.
    pub(super) fn restart_closure(&mut self, closure: TaggedValue) {
        if GC_CHECK {
            if self.top_awaits() {
                wrote_an_awaiting_frame("restart_closure", 0);
            }
            let base = self.top_base();
            let code = self.stack.top_code().expect("the top frame's code");
            fill_reused(
                self.stack.words(),
                base + argument_slots(code.arity),
                base + code.num_regs as usize,
            );
        }
        self.stack.restart_top(closure);
    }

    /// Start the top stub frame's next round in `code`, at pc 0, as its own
    /// work: the registers below `keep` carry over from the round before,
    /// and the rest of `code`'s are [`FILL`]ed (#714), for the round to
    /// write its operands into with `set_top`. Answers the window's base.
    /// Resume variants share one window size, and only their instruction
    /// stream changes (`resume_primitive`, which keeps the primitive's
    /// index); a wind step restarts in its own code (`push_wind_step`, which
    /// keeps the jump's target and value).
    pub(super) fn restart_stub(&mut self, code: Rc<CodeObject>, keep: u16) -> usize {
        self.clear_awaiting();
        let base = self.top_base();
        debug_assert!(base + code.num_regs as usize <= self.stack.top_end());
        debug_assert!(keep <= code.num_regs);
        fill(&self.stack.words()[base + keep as usize..base + code.num_regs as usize]);
        self.stack.recode_top(&code);
        base
    }

    /// One instruction's prologue: the top frame's pc, advanced past the
    /// instruction, and its window's base, with `cur_code` made the top
    /// frame's code where it is not. Three loads from the stack's own
    /// fields and a compare (`FrameStack::dispatch`).
    #[inline(always)]
    pub(super) fn dispatch_frame(&mut self, cur_code: &mut Rc<CodeObject>) -> (usize, usize) {
        let (pc, switched) = self.stack.dispatch(cur_code);
        if switched {
            self.running.0.set(cur_code.id.0);
        }
        self.clear_awaiting();
        (pc, self.stack.base())
    }

    #[inline(always)]
    pub(super) fn set_pc(&mut self, pc: usize) {
        self.stack.set_pc(pc);
    }
    pub(super) fn set_return_reg(&mut self, dst: u16) {
        self.top_base();
        self.stack.set_top_return_reg(dst);
    }

    pub(super) fn push_prompt(
        &mut self,
        tag: TaggedValue,
        handler: TaggedValue,
        dst: u16,
    ) -> usize {
        let index = self.prompt_stack.len();
        self.prompt_stack.push(PromptFrame {
            tag,
            handler,
            dst,
            stack_depth: self.depth(),
            dynamic_wind_depth: self.dynamic_winds.len(),
            exception_handler_depth: self.exception_handlers.len(),
        });
        index
    }
    pub(super) fn close_prompts(&mut self, index: usize) {
        self.prompt_stack.truncate(index);
    }
    pub(super) fn push_handler(&mut self, handler: TaggedValue) -> usize {
        let index = self.exception_handlers.len();
        self.exception_handlers.push(ExceptionHandler {
            handler,
            stack_depth: self.depth(),
        });
        index
    }
    pub(super) fn restore_handler(&mut self, handler: ExceptionHandler) {
        self.exception_handlers.push(handler);
    }
    pub(super) fn pop_handler(&mut self) -> Option<ExceptionHandler> {
        self.exception_handlers.pop()
    }
    pub(super) fn close_handlers(&mut self, index: usize) {
        self.exception_handlers.truncate(index);
    }
    pub(super) fn push_wind(&mut self, record: DynamicWindRecord) {
        self.dynamic_winds.push(record);
    }
    pub(super) fn pop_wind(&mut self) -> Option<DynamicWindRecord> {
        self.dynamic_winds.pop()
    }

    /// Only after value delivery or full-jump arrival, never a tail-call pop.
    pub(super) fn pop_resolved_prompts(&mut self) {
        while self
            .prompt_stack
            .last()
            .is_some_and(|p| p.stack_depth >= self.depth())
        {
            self.prompt_stack.pop();
        }
    }
    pub(super) fn pop_resolved_handlers(&mut self) {
        while self
            .exception_handlers
            .last()
            .is_some_and(|h| h.stack_depth >= self.depth())
        {
            self.exception_handlers.pop();
        }
    }
    /// Make `handlers` the live handler stack for a wind step's thunk, each
    /// recorded depth clamped to the depth the step's stub frame, on top, was
    /// pushed at: the depth the thunk is called above. The stub stays for
    /// the whole travel (#714), so that depth is the same at every step.
    pub(super) fn install_thunk_handlers(&mut self, handlers: &[ExceptionHandler]) {
        let depth = self.top_frame_depth();
        self.exception_handlers.clear();
        self.exception_handlers
            .extend(handlers.iter().map(|h| ExceptionHandler {
                handler: h.handler,
                stack_depth: h.stack_depth.min(depth),
            }));
    }

    /// Capture every dynamic component together; handle allocation clears
    /// the copy's dead temporaries and its hole, `deliver_reg`, and installs
    /// the weak-store payload before a safe point. `stack_reserve_open` is
    /// the machine's, which the snapshot carries (#907).
    pub(super) fn capture_full(
        &self,
        deliver_reg: u16,
        reentry: Rc<[u64]>,
        stack_reserve_open: bool,
    ) -> VmContinuation {
        let (frames, registers) = self.stack.census_shape();
        patina_core::census::capture_full(frames, registers);
        VmContinuation {
            stack: self.stack.copy(),
            dynamic_winds: self.dynamic_winds.clone(),
            prompt_stack: self.prompt_stack.clone(),
            exception_handlers: self.exception_handlers.clone(),
            deliver_reg,
            exit_status: None,
            abort_landing: false,
            reentry,
            stack_reserve_open,
        }
    }

    /// Full arrival replaces every component. The control layer decides
    /// which Rust re-entry boundaries it leaves and when to deliver the value.
    /// Refused, with every component as it was, where the snapshot's stack
    /// does not fit the reservation: none is, since a snapshot is a copy of
    /// this stack or of part of it with one stub's frame more, which the
    /// machine's cap admitted.
    pub(super) fn restore(&mut self, cc: &VmContinuation) -> Result<(), NoRoom> {
        let (frames, registers) = cc.stack.census_shape();
        patina_core::census::restore_full(frames, registers);
        // Into the reservation, at its base: an arrival moves no frame
        // that stays, and the stack's address does not change.
        self.stack.copy_from(&cc.stack)?;
        self.dynamic_winds = cc.dynamic_winds.clone();
        self.prompt_stack = cc.prompt_stack.clone();
        self.exception_handlers = cc.exception_handlers.clone();
        self.mark_awaiting();
        Ok(())
    }

    /// Abort fast path, only when there is no wind thunk left to run. The
    /// caller installs the handler stub before yielding to the dispatch loop.
    pub(super) fn truncate_to_prompt(&mut self, prompt_idx: usize) {
        let prompt = &self.prompt_stack[prompt_idx];
        debug_assert_eq!(
            self.dynamic_winds.len(),
            prompt.dynamic_wind_depth.min(self.dynamic_winds.len())
        );
        let landing_depth = prompt.stack_depth.min(self.depth());
        self.stack.truncate(landing_depth.bytes());
        self.exception_handlers.truncate(
            prompt
                .exception_handler_depth
                .min(self.exception_handlers.len()),
        );
        self.prompt_stack.truncate(prompt_idx);
        self.mark_awaiting();
    }

    pub(super) fn push_abort_stub(
        &mut self,
        code: Rc<CodeObject>,
        return_reg: u16,
        handler: TaggedValue,
        val: TaggedValue,
        cont: TaggedValue,
    ) -> Result<(), NoRoom> {
        self.debug_assert_file_ends_at_top();
        push_abort_stub(&mut self.stack, code, return_reg, handler, val, cont)
    }

    /// Abandon an uncaught form: drop every component without running winds.
    pub(super) fn clear(&mut self) {
        self.stack.clear();
        self.prompt_stack.clear();
        self.dynamic_winds.clear();
        self.exception_handlers.clear();
        self.clear_awaiting();
        self.answer.set(None);
    }

    /// The register stack's trace (#906): a collection's walk of the
    /// frames, which reports each frame's code, closure and live registers
    /// to `visitor` and clears the rest (`gc_roots::trace_frames`). Called
    /// by `VmState`'s `trace_roots` and by nothing else, so only inside a
    /// collection.
    pub(super) fn trace_frames(&self, visitor: &mut dyn SlotVisitor) {
        super::vm_state::gc_roots::trace_frames(&self.stack, visitor);
    }

    /// Re-entry thunks have already pushed the captured winds. Relocate and
    /// append the other four components together, then deliver into the hole.
    /// The inner refusal is the stack's, with no component changed: the
    /// captured frames would end past its reservation, which the caller's
    /// check of the cap rules out (`finish_delimited_invoke`).
    pub(super) fn append_delimited(
        &mut self,
        dc: &VmDelimitedContinuation,
        value: TaggedValue,
        dst: u16,
    ) -> Result<Result<(), NoRoom>, VmError> {
        // Checked, because arbitrary user code has run between the invoke and
        // here — that is what the stub frames are for — and this assumes every
        // captured record is still on top. Unchecked it would wrap in release and
        // hand `relocate_count` a base near 2^64, producing carried prompts with
        // garbage wind depths: silent corruption of some later abort's travel
        // rather than a diagnosis here.
        let wind_base = self
            .dynamic_winds
            .len()
            .checked_sub(dc.dynamic_winds.len())
            .ok_or_else(|| VmError::Runtime {
                message: "composable invoke: a re-entered extent left the wind stack".into(),
            })?;
        // The captured frames go onto the end of the live stack as they
        // are: each records a distance to its caller, so only the outermost
        // one's is written, for the frame it now sits above (#908).
        let outermost = self.depth();
        if let Err(refused) = self.stack.append(&dc.stack) {
            return Ok(Err(refused));
        }

        // The dynamic environment the captured frames ran in comes back with them:
        // the prompts they established and the handlers they installed. Every
        // depth in it was recorded against a stack at capture and has to be moved
        // onto the live one — and a `PromptFrame` records **three**, one per stack
        // it delimits. Relocating only `stack_depth` left the other two indexing
        // the invoke site's stacks, so an abort to a carried prompt ran an
        // enclosing `after` thunk early and uninstalled handlers enclosing the
        // invoke.
        let handler_base = self.exception_handlers.len();
        for p in dc.prompt_stack.iter() {
            let stack_depth = p.stack_depth.relocate(dc.depth_at_capture, outermost);
            debug_assert!(
                stack_depth >= outermost,
                "a carried prompt is inside the capture"
            );
            self.prompt_stack.push(PromptFrame {
                stack_depth,
                dynamic_wind_depth: relocate_count(
                    p.dynamic_wind_depth,
                    dc.wind_depth_at_capture,
                    wind_base,
                ),
                exception_handler_depth: relocate_count(
                    p.exception_handler_depth,
                    dc.handler_depth_at_capture,
                    handler_base,
                ),
                // A carried prompt sitting at the outermost appended frame — its
                // `call-with-continuation-prompt` was tail-called, so it shares
                // that frame's depth — has no captured frame below it to deliver
                // into any more. Its result is this invoke's result, by the same
                // reasoning that re-points `return_reg` below.
                dst: if stack_depth == outermost { dst } else { p.dst },
                ..p.clone()
            });
        }
        self.exception_handlers
            .extend(dc.exception_handlers.iter().map(|h| ExceptionHandler {
                stack_depth: h.stack_depth.relocate(dc.depth_at_capture, outermost),
                ..h.clone()
            }));
        // Both stacks are swept by frame depth as the resumed frames return
        // (`pop_resolved_extents`), which is when they stop applying — except at a
        // dispatch loop's own exit depth, where that sweep does nothing by design
        // and `run_loop_until_outcome`'s `handlers_at_entry` truncation is the
        // backstop for both handlers and prompts. A prompt a *full*
        // continuation's snapshot carries past its own body is swept on arrival
        // instead (issue #176, full arrival in `step_wind_jump`); a composable invoke has no
        // equivalent, because it appends to the live stacks rather than replacing
        // them, and the frames it appends are the ones the depth sweep follows.

        self.stack.set_return_reg_at(outermost.bytes(), dst);
        let deliver_reg = dc
            .deliver_reg
            .expect("the caller returns Identity when there is no hole");
        // Into the innermost appended frame, which the append reinstated;
        // `deliver_reg` is Some only for a non-empty capture.
        self.mark_awaiting();
        self.return_into(ReturnFrame::Top, deliver_reg, value);
        Ok(Ok(()))
    }
}

/// The register stack's words, lent for reading: what
/// [`ExecutionState::registers`] answers. A frame's registers are the words
/// from its `register_base`; its header is the five below. No method
/// writes, so a holder of `&VmState` cannot write a register (#904); the
/// step tracer reads through it.
#[derive(Clone, Copy)]
pub(crate) struct Registers<'a>(&'a [Cell<TaggedValue>]);

impl<'a> Registers<'a> {
    #[inline(always)]
    pub(crate) fn len(self) -> usize {
        self.0.len()
    }

    /// The register at `index`, panicking past the end as indexing does.
    #[inline(always)]
    pub(crate) fn at(self, index: usize) -> TaggedValue {
        self.0[index].get()
    }

    /// The register at `index`, if the file reaches it.
    #[inline(always)]
    pub(crate) fn get(self, index: usize) -> Option<TaggedValue> {
        self.0.get(index).map(Cell::get)
    }

    /// The registers in `range`, in order.
    pub(crate) fn values(self, range: Range<usize>) -> impl Iterator<Item = TaggedValue> + 'a {
        self.0[range].iter().map(Cell::get)
    }

    /// Every register, in order.
    pub(crate) fn iter(self) -> impl Iterator<Item = TaggedValue> + 'a {
        self.0.iter().map(Cell::get)
    }
}

/// What a pop answers of the frame it removed: the register of its caller
/// that the frame returns into.
pub(super) struct Popped {
    pub(super) return_reg: u16,
}

/// The frame an [`ExecutionState::return_into`] writes into (#904).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReturnFrame {
    /// The top frame, which something has just returned into: the frame
    /// above it popped, a jump or a composable invoke reinstated it, or a
    /// callee it called answered here and now.
    Top,
    /// [`ReturnFrame::Top`], whose window base the caller holds already:
    /// the base the dispatch prologue hoisted, which a primitive's call
    /// keeps across the callback it may make.
    TopAt(usize),
    /// The frame below the top: the one the top frame returns into.
    Caller,
}

/// Move a count recorded against one wind or handler stack onto another:
/// `value` sat `from` records up that stack, and the same position is `to`
/// records up this one. Saturating rather than wrapping, as
/// [`Depth::relocate`] is for frame depths, so a count recorded below `from`
/// lands at `to` rather than near 2^64.
fn relocate_count(value: usize, from: usize, to: usize) -> usize {
    (to + value).saturating_sub(from)
}

/// The panic of a write into a top frame that awaits a value made by its own
/// write path (#904): something has returned into the frame since its last
/// instruction began, so the write is [`ExecutionState::return_into`]'s,
/// where stage 7 lowers the stack watermark.
#[cold]
#[inline(never)]
fn wrote_an_awaiting_frame(path: &str, index: usize) -> ! {
    panic!(
        "{path} wrote register {index} of a top frame that awaits a value: something \
         returned into it, or it called a callee that answered, since its instruction \
         began, so the write goes through `return_into` (#904, PRD/GC_PRD.md §11.1)"
    )
}

/// The panic of a [`ExecutionState::return_into`] into a top frame that
/// awaits no value (#904): nothing has returned into it since its
/// instruction began, so the write is its own, `set_top`'s.
#[cold]
#[inline(never)]
fn returned_into_a_running_frame(reg: u16) -> ! {
    panic!(
        "return_into wrote r{reg} of a top frame that awaits no value: nothing returned \
         into it, and it called nothing that answered, since its instruction began, so the \
         write is its own and goes through `set_top` (#904)"
    )
}

/// Put the frame an abort lands in on top of `stack`.
///
/// One description of the landing frame, used by both of `abort_to_prompt`'s
/// paths — the one that cuts the live machine back in place and the one that
/// describes the same machine as a jump target. They differ in how they get
/// there and must not differ in where they end up.
///
/// # State contract
///
/// Requires loaded abort-handler code. Appends one initialized frame to
/// `stack`, live or detached; no other dynamic components are touched.
pub(super) fn push_abort_stub<B: Buffer>(
    stack: &mut FrameStack<B>,
    code: Rc<CodeObject>,
    return_reg: u16,
    handler: TaggedValue,
    val: TaggedValue,
    cont: TaggedValue,
) -> Result<(), NoRoom> {
    let base = stack.push(
        &code,
        CallFrame::NO_CLOSURE,
        return_reg,
        abort_step::NUM_REGS,
        fill_value(),
    )?;
    let words = stack.words();
    words[base + abort_step::HANDLER as usize].set(handler);
    words[base + abort_step::VAL as usize].set(val);
    words[base + abort_step::CONT as usize].set(cont);
    Ok(())
}
