//! The five coupled components in VM_RUNTIME §5.6, and the id of the code
//! the running dispatch loop runs. Only this module may resize/rebase live
//! register windows or replace a dynamic-state snapshot. Transfer policy
//! (wind travel, re-entry, error routing) stays in `control`.

use super::control::abort_step;
use crate::error::VmError;
use crate::types::code_object::CodeObject;
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::{CallFrame, Depth, FrameCode};
use patina_core::heap::Heap;
use patina_core::{GC_CHECK, SlotVisitor, TaggedValue, Trace};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::slice::SliceIndex;

/// Its trace (derived, #686) is all five components and the running code's
/// id, a root of every collection through `VmState`'s. The sentinel test
/// `every_vm_state_root_is_traced` pins each component, and
/// `the_running_code_keeps_its_units_link_table` the running code.
#[derive(Default, Trace)]
pub(super) struct ExecutionState {
    /// Slots, so that the root provider reports each register where it
    /// lives, through `&self` (#682): the whole register file in one call
    /// (`Trace::trace_slice`), after the safe point has retired completed
    /// expression temporaries (#423). Continuation snapshots are retired at
    /// capture, so they obey the same full-vector contract.
    registers: Vec<Cell<TaggedValue>>,
    frames: Vec<CallFrame>,
    prompt_stack: Vec<PromptFrame>,
    dynamic_winds: Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
    /// The code the innermost running dispatch loop runs, by id (#836).
    running: RunningCode,
    /// Which top frame awaits a value, for the check builds' assertions on
    /// the register writes (#904).
    #[trace(skip, reason = "a frame depth, which only a check build writes")]
    awaiting: Awaiting,
}

/// A check build's record of the top frame that awaits a value (#904): the
/// depth of the stack whose top frame something has returned into, or
/// [`Depth::BASE`] when none does. [`ExecutionState::return_into`] writes
/// that frame, and [`ExecutionState::set_top`] may not until the frame
/// starts its own work again.
///
/// Set where the top frame comes to await a value: where the stack shrinks
/// under it or is reinstated (`pop_frame`, `finish_wind_step`,
/// `truncate_to_prompt`, `restore`, `append_delimited`), and where it calls
/// a callee that may answer here and now, which a callback's frames may
/// have run above it before then ([`ExecutionState::mark_awaiting`]).
/// Cleared where the top frame starts its own work (`dispatch_frame`,
/// `restart_stub`) and where the stack goes (`clear`). A push leaves it, and
/// every shrink sets it, so it is `BASE` or no deeper than the stack, and
/// equal to the stack's depth only while its top frame awaits a value.
///
/// In every build, since the VM cannot name core's `gc-check` feature, but
/// written and read only under `GC_CHECK`: a plain release build keeps the
/// word and never touches it.
struct Awaiting(Cell<Depth>);

impl Default for Awaiting {
    fn default() -> Self {
        Awaiting(Cell::new(Depth::BASE))
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
    /// Build a state from its five components, for tests that fill each with
    /// a value nothing else holds, with no running code. A struct literal,
    /// so a new component fails to compile here too.
    #[cfg(test)]
    pub(super) fn from_parts(
        registers: Vec<Cell<TaggedValue>>,
        frames: Vec<CallFrame>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Self {
        ExecutionState {
            registers,
            frames,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
            running: RunningCode::default(),
            awaiting: Awaiting::default(),
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

    /// The register file, lent for reading ([`Registers`]). The writes are
    /// this module's: `set_top`, `return_into` and a push's (#904).
    #[inline(always)]
    pub(super) fn registers(&self) -> Registers<'_> {
        Registers(&self.registers)
    }
    /// The register file's slots, for the one test that reports each where
    /// it lives (`every_slot_the_vm_reports_is_where_its_value_lives`). The
    /// collector reaches them through the derived trace, and retirement
    /// through [`Self::retire_registers`], both inside this module.
    #[cfg(test)]
    pub(super) fn register_cells(&self) -> &[Cell<TaggedValue>] {
        &self.registers
    }
    #[inline(always)]
    pub(super) fn frames(&self) -> &[CallFrame] {
        &self.frames
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

    /// How deep the frame stack is: the one reading of its depth (#903).
    /// Prompts, handlers and delimited captures record it, and the dispatch
    /// loops stop at it. A count of frames until GC stage 4d.7 (#908), which
    /// makes it a byte offset from the register stack's base
    /// (`PRD/GC_PRD.md` §11.1) by changing what [`Depth`] holds.
    #[inline(always)]
    pub(super) fn depth(&self) -> Depth {
        Depth::of_frames(self.frames.len())
    }

    /// The depth the top frame was pushed at: [`Self::depth`] less that
    /// frame, or [`Depth::BASE`] with no frame. An accessor rather than a
    /// [`Depth`] method because once depths are byte offsets (4d.7) it is the
    /// top frame's own offset, which a depth alone does not give.
    #[inline(always)]
    pub(super) fn top_frame_depth(&self) -> Depth {
        self.depth().lowered_by(1)
    }

    // ── Register writes (#904) ──────────────────────────────────────────
    //
    // The only ways to write a live register: the top frame's own write
    // (`set_top`), a write into a frame something has returned into or one
    // below the top (`return_into`), and a push's initializing writes
    // (`push_frame`, `push_abort_stub`, and the arguments the call paths
    // copy into a callee's window, `init_arg` and `init_args`).
    // `inline(always)`, as the register helpers they replace were:
    // `dispatch_one_instruction` is too large for LLVM to inline them
    // otherwise (PRD §1.6 finding 1).

    /// The top frame's window base.
    #[inline(always)]
    fn top_base(&self) -> usize {
        self.frames.last().expect("no active frame").register_base
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

    /// Whether the top frame awaits a value. Kept only by a check build.
    fn top_awaits(&self) -> bool {
        let awaiting = self.awaiting.0.get();
        awaiting != Depth::BASE && awaiting == self.depth()
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
    #[inline(always)]
    pub(super) fn return_into(&mut self, frame: ReturnFrame, reg: u16, value: TaggedValue) {
        let base = match frame {
            ReturnFrame::Top => self.top_base(),
            ReturnFrame::TopAt(base) => {
                debug_assert_eq!(base, self.top_base());
                base
            }
            ReturnFrame::Caller => {
                let [.., caller, _] = self.frames.as_slice() else {
                    panic!("no frame below the top");
                };
                caller.register_base
            }
        };
        if GC_CHECK && frame != ReturnFrame::Caller && !self.top_awaits() {
            returned_into_a_running_frame(reg);
        }
        self.registers[base + reg as usize].set(value);
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
        self.registers[base + reg as usize].set(value);
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
        self.registers[base + index].set(value);
    }

    /// [`Self::init_arg`] for the first `slots` registers of that window,
    /// from `values`, as many as both have, under one range check.
    #[inline(always)]
    pub(super) fn init_args(&mut self, base: usize, slots: usize, values: &[TaggedValue]) {
        debug_assert_eq!(base, self.top_base());
        if GC_CHECK && self.top_awaits() {
            wrote_an_awaiting_frame("init_args", 0);
        }
        for (slot, &value) in self.registers[base..base + slots].iter().zip(values) {
            slot.set(value);
        }
    }

    /// The frames below `depth`: the ones a stack cut back to it keeps.
    pub(super) fn frames_below(&self, depth: Depth) -> &[CallFrame] {
        &self.frames[..depth.frame_count()]
    }

    /// The frames at and above `depth`: the ones a capture from it copies.
    pub(super) fn frames_above(&self, depth: Depth) -> &[CallFrame] {
        &self.frames[depth.frame_count()..]
    }

    /// Enter one loaded code object with its matching window. Caller fills
    /// operands before the next safe point. Dynamic extents are unchanged.
    pub(super) fn push_frame(
        &mut self,
        code: Rc<CodeObject>,
        closure: TaggedValue,
        return_reg: u16,
    ) -> usize {
        let base = self.registers.len();
        let num_regs = code.num_regs;
        self.registers
            .resize(base + num_regs as usize, Cell::new(TaggedValue::NULL));
        self.frames.push(CallFrame {
            pc: 0,
            register_base: base,
            num_regs,
            closure: Cell::new(closure),
            return_reg,
            code: FrameCode(code),
        });
        base
    }

    /// Remove a frame and its window together. Extents stay live until value
    /// delivery: a tail-called raise must still find its caller's handler.
    pub(super) fn pop_frame(&mut self) -> CallFrame {
        let frame = self.frames.pop().expect("pop with empty frame stack");
        self.registers.truncate(frame.register_base);
        self.mark_awaiting();
        frame
    }

    /// A jump step is the one deliberate exception to paired removal: its
    /// operands remain roots until the next stub holds them or arrival replaces
    /// the register file. This preserves the existing weak-store root contract.
    pub(super) fn finish_wind_step(&mut self) {
        self.frames
            .pop()
            .expect("ResumeWindJump runs in its own frame");
        self.mark_awaiting();
    }

    /// Reuse the top window for another closure. Keep excess capacity as the
    /// existing tail-call path does; the GC retirement map clears dead slots.
    pub(super) fn tail_replace(&mut self, code: Rc<CodeObject>, closure: TaggedValue) -> usize {
        let frame = self
            .frames
            .last_mut()
            .expect("tail call with empty frame stack");
        if code.num_regs > frame.num_regs {
            let extra = code.num_regs - frame.num_regs;
            self.registers.resize(
                self.registers.len() + extra as usize,
                Cell::new(TaggedValue::NULL),
            );
            frame.num_regs = code.num_regs;
        }
        frame.pc = 0;
        frame.closure.set(closure);
        frame.code = FrameCode(code);
        frame.register_base
    }

    pub(super) fn restart_closure(&mut self, closure: TaggedValue) {
        let frame = self
            .frames
            .last_mut()
            .expect("tail call with empty frame stack");
        frame.pc = 0;
        frame.closure.set(closure);
    }

    /// Resume variants share one window size; only their instruction stream
    /// changes. The stub starts its next round as its own work, so its
    /// operands are written with `set_top` after this.
    pub(super) fn restart_stub(&mut self, code: Rc<CodeObject>) {
        let frame = self.frames.last_mut().expect("the stub frame");
        debug_assert!(code.num_regs <= frame.num_regs);
        frame.code = FrameCode(code);
        frame.pc = 0;
        self.clear_awaiting();
    }

    #[inline(always)]
    pub(super) fn dispatch_frame(&mut self, cur_code: &mut Rc<CodeObject>) -> (usize, usize) {
        let frame = self.frames.last_mut().expect("empty frame stack");
        if !Rc::ptr_eq(cur_code, &frame.code) {
            *cur_code = Rc::clone(&frame.code);
            self.running.0.set(frame.code.id.0);
        }
        let pc = frame.pc;
        frame.pc = pc + 1;
        let base = frame.register_base;
        self.clear_awaiting();
        (pc, base)
    }

    #[inline(always)]
    pub(super) fn set_pc(&mut self, pc: usize) {
        self.frames.last_mut().expect("empty frame stack").pc = pc;
    }
    pub(super) fn set_return_reg(&mut self, dst: u16) {
        self.frames.last_mut().expect("the stub frame").return_reg = dst;
    }

    /// Reserve a scratch destination without changing the frame's live window.
    pub(super) fn scratch_return_reg(&mut self) -> u16 {
        let return_reg = self.frames.last().map(|f| f.num_regs).unwrap_or(0);
        if let Some(f) = self.frames.last() {
            let needed = f.register_base + return_reg as usize + 1;
            if self.registers.len() < needed {
                self.registers
                    .resize(needed, Cell::new(TaggedValue::UNSPECIFIED));
            }
        }
        return_reg
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
    pub(super) fn install_thunk_handlers(&mut self, handlers: &[ExceptionHandler]) {
        let depth = self.depth();
        self.exception_handlers.clear();
        self.exception_handlers
            .extend(handlers.iter().map(|h| ExceptionHandler {
                handler: h.handler,
                stack_depth: h.stack_depth.min(depth),
            }));
    }

    /// Capture every dynamic component together; handle allocation retires
    /// dead temporaries and installs the weak-store payload before a safe point.
    pub(super) fn capture_full(&self, deliver_reg: u16, reentry: Rc<[u64]>) -> VmContinuation {
        patina_core::census::capture_full(self.depth().frame_count(), self.registers.len());
        VmContinuation {
            frames: self.frames.clone(),
            dynamic_winds: self.dynamic_winds.clone(),
            prompt_stack: self.prompt_stack.clone(),
            exception_handlers: self.exception_handlers.clone(),
            registers: self.registers.clone(),
            deliver_reg,
            exit_status: None,
            abort_landing: false,
            reentry,
        }
    }

    /// Full arrival replaces all five components. The control layer decides
    /// which Rust re-entry boundaries it leaves and when to deliver the value.
    pub(super) fn restore(&mut self, cc: &VmContinuation) {
        patina_core::census::restore_full(cc.depth().frame_count(), cc.registers.len());
        self.registers = cc.registers.clone();
        self.frames = cc.frames.clone();
        self.dynamic_winds = cc.dynamic_winds.clone();
        self.prompt_stack = cc.prompt_stack.clone();
        self.exception_handlers = cc.exception_handlers.clone();
        self.mark_awaiting();
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
        let registers_end = self
            .frames_below(landing_depth)
            .last()
            .map_or(0, |f| f.register_base + f.num_regs as usize);
        self.frames.truncate(landing_depth.frame_count());
        self.registers.truncate(registers_end);
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
    ) {
        push_abort_stub(
            &mut self.frames,
            &mut self.registers,
            code,
            return_reg,
            handler,
            val,
            cont,
        );
    }

    /// Abandon an uncaught form: drop all five components without running winds.
    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.registers.clear();
        self.prompt_stack.clear();
        self.dynamic_winds.clear();
        self.exception_handlers.clear();
        self.clear_awaiting();
    }

    pub(super) fn retire_registers(&mut self) {
        super::vm_state::gc_roots::retire_registers(&mut self.registers, &self.frames, 0);
    }

    /// Re-entry thunks have already pushed the captured winds. Relocate and
    /// append the other four components together, then deliver into the hole.
    pub(super) fn append_delimited(
        &mut self,
        dc: &VmDelimitedContinuation,
        value: TaggedValue,
        dst: u16,
    ) -> Result<(), VmError> {
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
        // Relocate the captured register windows onto the end of the live array.
        let shift = self.registers.len().wrapping_sub(dc.base_at_capture);
        self.registers.extend_from_slice(&dc.registers);
        let outermost = self.depth();
        self.frames.extend(dc.frames.iter().cloned());
        for f in &mut self.frames[outermost.frame_count()..] {
            f.register_base = f.register_base.wrapping_add(shift);
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

        self.frames[outermost.frame_count()].return_reg = dst;
        let deliver_reg = dc
            .deliver_reg
            .expect("the caller returns Identity when there is no hole");
        // Into the innermost appended frame, which the append reinstated;
        // `deliver_reg` is Some only for a non-empty capture.
        self.mark_awaiting();
        self.return_into(ReturnFrame::Top, deliver_reg, value);
        Ok(())
    }
}

/// The register file, lent for reading: what [`ExecutionState::registers`]
/// answers. No method writes, so a holder of `&VmState` cannot write a
/// register (#904); the step tracer and the snapshot copies read through it.
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

    /// A copy of the registers `range` takes: a snapshot's own file, which
    /// the capture and retirement may then write, as they write a
    /// continuation's, never the machine's.
    pub(crate) fn copy<R>(self, range: R) -> Vec<Cell<TaggedValue>>
    where
        R: SliceIndex<[Cell<TaggedValue>], Output = [Cell<TaggedValue>]>,
    {
        self.0[range].to_vec()
    }
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

/// Put the frame an abort lands in on top of `frames`, with its window on the
/// end of `registers`.
///
/// One description of the landing frame, used by both of `abort_to_prompt`'s
/// paths — the one that cuts the live machine back in place and the one that
/// describes the same machine as a jump target. They differ in how they get
/// there and must not differ in where they end up.
///
/// # State contract
///
/// Requires a matched frame/register prefix and loaded abort-handler code.
/// Appends one initialized frame/window to those buffers, live or detached;
/// no other dynamic components are touched.
pub(super) fn push_abort_stub(
    frames: &mut Vec<CallFrame>,
    registers: &mut Vec<Cell<TaggedValue>>,
    code: Rc<CodeObject>,
    return_reg: u16,
    handler: TaggedValue,
    val: TaggedValue,
    cont: TaggedValue,
) {
    let base = registers.len();
    registers.resize(
        base + abort_step::NUM_REGS as usize,
        Cell::new(TaggedValue::NULL),
    );
    registers[base + abort_step::HANDLER as usize].set(handler);
    registers[base + abort_step::VAL as usize].set(val);
    registers[base + abort_step::CONT as usize].set(cont);
    frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: abort_step::NUM_REGS,
        closure: Cell::new(CallFrame::NO_CLOSURE),
        return_reg,
        code: FrameCode(code),
    });
}
