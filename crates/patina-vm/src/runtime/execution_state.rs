//! The five coupled components in VM_RUNTIME §5.6. Only this module may
//! resize/rebase live register windows or replace a dynamic-state snapshot.
//! Transfer policy (wind travel, re-entry, error routing) stays in `control`.

use super::control::abort_step;
use super::vm_state::gc_roots::{trace_frames, trace_handlers, trace_prompts, trace_winds};
use crate::error::VmError;
use crate::types::CallFrame;
use crate::types::code_object::CodeObject;
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use patina_core::tagged_value::ObjectIndex;
use patina_core::{GcVisitor, TaggedValue};
use std::rc::Rc;

#[derive(Default)]
pub(super) struct ExecutionState {
    registers: Vec<TaggedValue>,
    frames: Vec<CallFrame>,
    prompt_stack: Vec<PromptFrame>,
    dynamic_winds: Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
}

impl ExecutionState {
    /// GC roots: all five components, every field named (#623), so a new one
    /// does not compile here until it is traced. `VmState`'s provider calls
    /// this; the sentinel test `every_vm_state_root_is_traced` pins each.
    pub(super) fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        let ExecutionState {
            registers,
            frames,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } = self;
        // The whole register file, after the safe point has retired completed
        // expression temporaries (#423). Continuation snapshots are retired at
        // capture, so they obey the same full-vector contract.
        visitor.visit_slice(registers);
        trace_frames(frames, visitor);
        trace_prompts(prompt_stack, visitor);
        trace_winds(dynamic_winds, visitor);
        trace_handlers(exception_handlers, visitor);
    }

    /// Build a state from its five components, for tests that fill each with
    /// a value nothing else holds. A struct literal, so a new component fails
    /// to compile here too.
    #[cfg(test)]
    pub(super) fn from_parts(
        registers: Vec<TaggedValue>,
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
        }
    }

    #[inline(always)]
    pub(super) fn registers(&self) -> &[TaggedValue] {
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

    /// Instruction/argument writes may replace slots, never resize a window.
    #[inline(always)]
    pub(super) fn registers_mut(&mut self) -> &mut [TaggedValue] {
        &mut self.registers
    }

    /// Enter one loaded code object with its matching window. Caller fills
    /// operands before the next safe point. Dynamic extents are unchanged.
    pub(super) fn push_frame(
        &mut self,
        code: Rc<CodeObject>,
        closure: Option<ObjectIndex>,
        return_reg: u16,
    ) -> usize {
        let base = self.registers.len();
        let num_regs = code.num_regs;
        self.registers
            .resize(base + num_regs as usize, TaggedValue::NULL);
        self.frames.push(CallFrame {
            pc: 0,
            register_base: base,
            num_regs,
            closure,
            return_reg,
            code,
        });
        base
    }

    /// Remove a frame and its window together. Extents stay live until value
    /// delivery: a tail-called raise must still find its caller's handler.
    pub(super) fn pop_frame(&mut self) -> CallFrame {
        let frame = self.frames.pop().expect("pop with empty frame stack");
        self.registers.truncate(frame.register_base);
        frame
    }

    /// A jump step is the one deliberate exception to paired removal: its
    /// operands remain roots until the next stub holds them or arrival replaces
    /// the register file. This preserves the existing weak-store root contract.
    pub(super) fn finish_wind_step(&mut self) {
        self.frames
            .pop()
            .expect("ResumeWindJump runs in its own frame");
    }

    /// Reuse the top window for another closure. Keep excess capacity as the
    /// existing tail-call path does; the GC retirement map clears dead slots.
    pub(super) fn tail_replace(
        &mut self,
        code: Rc<CodeObject>,
        closure: Option<ObjectIndex>,
    ) -> usize {
        let frame = self
            .frames
            .last_mut()
            .expect("tail call with empty frame stack");
        if code.num_regs > frame.num_regs {
            let extra = code.num_regs - frame.num_regs;
            self.registers
                .resize(self.registers.len() + extra as usize, TaggedValue::NULL);
            frame.num_regs = code.num_regs;
        }
        frame.pc = 0;
        frame.closure = closure;
        frame.code = code;
        frame.register_base
    }

    pub(super) fn restart_closure(&mut self, closure: Option<ObjectIndex>) {
        let frame = self
            .frames
            .last_mut()
            .expect("tail call with empty frame stack");
        frame.pc = 0;
        frame.closure = closure;
    }

    /// Resume variants share one window size; only their instruction stream changes.
    pub(super) fn restart_stub(&mut self, code: Rc<CodeObject>) {
        let frame = self.frames.last_mut().expect("the stub frame");
        debug_assert!(code.num_regs <= frame.num_regs);
        frame.code = code;
        frame.pc = 0;
    }

    #[inline(always)]
    pub(super) fn dispatch_frame(&mut self, cur_code: &mut Rc<CodeObject>) -> (usize, usize) {
        let frame = self.frames.last_mut().expect("empty frame stack");
        if !Rc::ptr_eq(cur_code, &frame.code) {
            *cur_code = frame.code.clone();
        }
        let pc = frame.pc;
        frame.pc = pc + 1;
        (pc, frame.register_base)
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
                self.registers.resize(needed, TaggedValue::UNSPECIFIED);
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
            stack_depth: self.frames.len(),
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
            stack_depth: self.frames.len(),
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
            .is_some_and(|p| p.stack_depth >= self.frames.len())
        {
            self.prompt_stack.pop();
        }
    }
    pub(super) fn pop_resolved_handlers(&mut self) {
        while self
            .exception_handlers
            .last()
            .is_some_and(|h| h.stack_depth >= self.frames.len())
        {
            self.exception_handlers.pop();
        }
    }
    pub(super) fn install_thunk_handlers(&mut self, handlers: &[ExceptionHandler]) {
        let depth = self.frames.len();
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
        self.registers = cc.registers.clone();
        self.frames = cc.frames.clone();
        self.dynamic_winds = cc.dynamic_winds.clone();
        self.prompt_stack = cc.prompt_stack.clone();
        self.exception_handlers = cc.exception_handlers.clone();
    }

    /// Abort fast path, only when there is no wind thunk left to run. The
    /// caller installs the handler stub before yielding to the dispatch loop.
    pub(super) fn truncate_to_prompt(&mut self, prompt_idx: usize) {
        let prompt = &self.prompt_stack[prompt_idx];
        debug_assert_eq!(
            self.dynamic_winds.len(),
            prompt.dynamic_wind_depth.min(self.dynamic_winds.len())
        );
        let landing_depth = prompt.stack_depth.min(self.frames.len());
        let registers_end = self.frames[..landing_depth]
            .last()
            .map_or(0, |f| f.register_base + f.num_regs as usize);
        self.frames.truncate(landing_depth);
        self.registers.truncate(registers_end);
        self.exception_handlers.truncate(
            prompt
                .exception_handler_depth
                .min(self.exception_handlers.len()),
        );
        self.prompt_stack.truncate(prompt_idx);
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
        // hand `relocate_depth` a base near 2^64, producing carried prompts with
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
        let outermost = self.frames.len();
        self.frames.extend(dc.frames.iter().cloned());
        for f in &mut self.frames[outermost..] {
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
            let stack_depth = relocate_depth(p.stack_depth, dc.depth_at_capture, outermost);
            debug_assert!(
                stack_depth >= outermost,
                "a carried prompt is inside the capture"
            );
            self.prompt_stack.push(PromptFrame {
                stack_depth,
                dynamic_wind_depth: relocate_depth(
                    p.dynamic_wind_depth,
                    dc.wind_depth_at_capture,
                    wind_base,
                ),
                exception_handler_depth: relocate_depth(
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
                stack_depth: relocate_depth(h.stack_depth, dc.depth_at_capture, outermost),
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

        self.frames[outermost].return_reg = dst;
        let top_base = self
            .frames
            .last()
            .expect("deliver_reg is Some only for a non-empty capture")
            .register_base;
        let deliver_reg = dc
            .deliver_reg
            .expect("the caller returns Identity when there is no hole");
        self.registers[top_base + deliver_reg as usize] = value;
        Ok(())
    }
}

/// Move a depth recorded against one stack onto another: `value` sat `from`
/// units up that stack, and the same position is `to` units up this one.
///
/// Saturating rather than wrapping, and not hypothetically:
/// `install_thunk_handlers` clamps a record's handler depths down to the
/// thunk's frame depth, which can be *below* the prompt a continuation was
/// delimited by, so `value < from` happens. Wrapping would send such a depth
/// to about 2^64, where `pop_exception_handlers`' `stack_depth >= frames.len()`
/// test drops it on the next return instead of keeping it for as long as the
/// resumed frames run.
fn relocate_depth(value: usize, from: usize, to: usize) -> usize {
    (to + value).saturating_sub(from)
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
    registers: &mut Vec<TaggedValue>,
    code: Rc<CodeObject>,
    return_reg: u16,
    handler: TaggedValue,
    val: TaggedValue,
    cont: TaggedValue,
) {
    let base = registers.len();
    registers.resize(base + abort_step::NUM_REGS as usize, TaggedValue::NULL);
    registers[base + abort_step::HANDLER as usize] = handler;
    registers[base + abort_step::VAL as usize] = val;
    registers[base + abort_step::CONT as usize] = cont;
    frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: abort_step::NUM_REGS,
        closure: None,
        return_reg,
        code,
    });
}
