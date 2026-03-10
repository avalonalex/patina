//! `VmState` — the complete mutable state of the VM during execution.
//!
//! See VM_RUNTIME.md §core-data-structures.

use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject};
use crate::types::continuation::{
    DynamicWindRecord, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::instruction::Instruction;
use crate::types::{CallFrame, CodeObjectId};
use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_core::tagged_value::TaggedValue;
use patina_primitives::PrimitiveRegistry;
use std::collections::HashMap;
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// VmState
// ─────────────────────────────────────────────────────────────────────────────

/// The complete mutable state of the VM during execution.
pub struct VmState {
    /// Flat register array. Each `CallFrame` owns a slice via `register_base + num_regs`.
    pub registers: Vec<TaggedValue>,
    /// The call stack. Currently-executing frame is `frames.last()`.
    pub frames: Vec<CallFrame>,
    /// Side channel for multiple return values (`values` / `call-with-values`).
    pub value_buffer: Vec<TaggedValue>,
    /// Stack of active continuation prompts (SRFI-226).
    pub prompt_stack: Vec<PromptFrame>,
    /// Stack of active `dynamic-wind` records.
    pub dynamic_winds: Vec<DynamicWindRecord>,
    /// All compiled `CodeObject`s, keyed by id.
    pub code_store: HashMap<CodeObjectId, Rc<CodeObject>>,
    /// Global variable environment, shared with the library loader.
    /// `Environment` has interior mutability, so no outer `RefCell` is needed.
    pub globals: Rc<Environment>,
    /// The heap, shared with `patina-runtime` primitives.
    pub heap: SharedHeap,
    /// Registry of all primitive procedures.
    pub primitive_registry: Rc<PrimitiveRegistry>,
    /// Side table for full (call/cc) continuations — keyed by opaque u64 id.
    /// The corresponding `TaggedValue` handle is `VmContinuationRef(id)` on the heap.
    pub continuation_store: HashMap<u64, Rc<VmContinuation>>,
    /// Side table for delimited continuations — keyed by opaque u64 id.
    pub delimited_continuation_store: HashMap<u64, Rc<VmDelimitedContinuation>>,
    /// Monotonically increasing counter for assigning continuation ids.
    pub next_cont_id: u64,
}

impl VmState {
    pub fn new(globals: Rc<Environment>) -> Self {
        let mut registry = PrimitiveRegistry::new();
        patina_primitives::register_all(&mut registry);
        // Share the heap with the environment so TaggedValue indices produced by
        // the parser (which also uses global_env().heap()) remain valid.
        let heap = globals.heap().clone();
        Self {
            registers: Vec::new(),
            frames: Vec::new(),
            value_buffer: Vec::new(),
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            code_store: HashMap::new(),
            globals,
            heap,
            primitive_registry: Rc::new(registry),
            continuation_store: HashMap::new(),
            delimited_continuation_store: HashMap::new(),
            next_cont_id: 0,
        }
    }

    /// Install all registered primitives into the global environment.
    ///
    /// Each primitive is stored as a `Procedure::Primitive` heap object so that
    /// `LoadGlobal` + `Call` can dispatch them via `try_call_primitive`.
    pub fn install_primitives(&mut self) {
        let prims: Vec<_> = self
            .primitive_registry
            .primitives()
            .map(|p| (p.name, p.qualified_name(), p.arity.clone()))
            .collect();
        for (name, qualified_name, arity) in prims {
            let proc = Rc::new(Procedure::Primitive {
                name,
                arity,
                qualified_name: Rc::from(qualified_name.as_str()),
            });
            let tv = self.heap.borrow_mut().alloc_procedure(proc);
            self.globals.define(name.to_string(), tv);
        }
    }

    /// Load a `CodeObject` (and nested ones) into the code store.
    pub fn load(&mut self, code: CodeObject) {
        let id = code.id;
        self.code_store.insert(id, Rc::new(code));
    }

    pub fn load_all(&mut self, codes: impl IntoIterator<Item = CodeObject>) {
        for c in codes {
            self.load(c);
        }
    }

    // ── Register helpers ───────────────────────────────────────────────────

    #[inline]
    fn frame_base(&self) -> usize {
        self.frames.last().expect("no active frame").register_base
    }

    #[inline]
    pub fn reg(&self, reg: u16) -> TaggedValue {
        self.registers[self.frame_base() + reg as usize]
    }

    #[inline]
    pub fn set_reg(&mut self, reg: u16, val: TaggedValue) {
        let base = self.frame_base();
        self.registers[base + reg as usize] = val;
    }

    /// Write into an arbitrary frame (for Return writing into the caller's slot).
    #[inline]
    fn set_reg_in_frame(&mut self, frame_idx: usize, reg: u16, val: TaggedValue) {
        let base = self.frames[frame_idx].register_base;
        self.registers[base + reg as usize] = val;
    }

    pub fn alloc_registers(&mut self, num_regs: u16) -> usize {
        let base = self.registers.len();
        self.registers
            .resize(base + num_regs as usize, TaggedValue::NULL);
        base
    }

    pub fn free_top_registers(&mut self, base: usize, num_regs: u16) {
        debug_assert_eq!(self.registers.len(), base + num_regs as usize);
        self.registers.truncate(base);
    }

    /// Allocate a full VM continuation, returning its heap `TaggedValue` handle.
    pub fn alloc_vm_continuation(&mut self, cont: VmContinuation) -> TaggedValue {
        let id = self.next_cont_id;
        self.next_cont_id += 1;
        self.continuation_store.insert(id, Rc::new(cont));
        self.heap.borrow_mut().alloc_vm_continuation_ref(id)
    }

    /// Allocate a delimited VM continuation, returning its heap `TaggedValue` handle.
    pub fn alloc_vm_delimited_continuation(
        &mut self,
        cont: VmDelimitedContinuation,
    ) -> TaggedValue {
        let id = self.next_cont_id;
        self.next_cont_id += 1;
        self.delimited_continuation_store.insert(id, Rc::new(cont));
        self.heap
            .borrow_mut()
            .alloc_vm_delimited_continuation_ref(id)
    }

    /// Look up a full continuation by its `TaggedValue` handle.
    pub fn get_vm_continuation(&self, tv: TaggedValue) -> Option<Rc<VmContinuation>> {
        let id = self.heap.borrow().get_vm_continuation_ref(tv)?;
        self.continuation_store.get(&id).cloned()
    }

    /// Look up a delimited continuation by its `TaggedValue` handle.
    pub fn get_vm_delimited_continuation(
        &self,
        tv: TaggedValue,
    ) -> Option<Rc<VmDelimitedContinuation>> {
        let id = self.heap.borrow().get_vm_delimited_continuation_ref(tv)?;
        self.delimited_continuation_store.get(&id).cloned()
    }

    pub fn current_code(&self) -> Result<Rc<CodeObject>, VmError> {
        let id = self.frames.last().expect("no active frame").code_id;
        self.code_store
            .get(&id)
            .cloned()
            .ok_or_else(|| VmError::Runtime {
                message: format!("missing CodeObject {:?}", id),
            })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution loop
// ─────────────────────────────────────────────────────────────────────────────

/// Execute the code object identified by `code_id` in `state`, with no
/// arguments. Returns the value in register 0 of the top frame on completion.
///
/// This is the primary entry point for running a compiled top-level expression.
pub fn execute(state: &mut VmState, code_id: CodeObjectId) -> Result<TaggedValue, VmError> {
    // Set up the initial frame.
    let code = state
        .code_store
        .get(&code_id)
        .cloned()
        .ok_or_else(|| VmError::Runtime {
            message: format!("CodeObject {:?} not loaded", code_id),
        })?;

    let base = state.alloc_registers(code.num_regs);
    state.frames.push(CallFrame {
        code_id,
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: None,
        return_reg: 0,
    });

    run_loop_until(state, 0)
}

/// Call a closure (heap index) with `args`, returning its result.
///
/// Used by the VM internally for calls to Scheme closures from the execution
/// loop and for `CallWithPrompt` body thunks.
fn call_closure(
    state: &mut VmState,
    closure_val: TaggedValue,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<(), VmError> {
    // Resolve the closure to a (code_id, _free_vars) pair.
    let (code_id, _free_vars) = resolve_closure(state, closure_val)?;

    let code = state
        .code_store
        .get(&code_id)
        .cloned()
        .ok_or_else(|| VmError::Runtime {
            message: format!("missing CodeObject {:?}", code_id),
        })?;

    // Arity check.
    check_arity(&code.arity, args.len())?;

    let base = state.alloc_registers(code.num_regs);
    // Copy args into the callee's parameter registers.
    for (i, &arg) in args.iter().enumerate() {
        state.registers[base + i] = arg;
    }
    // For variadic: collect rest args into a list (heap alloc).
    if let Arity::Variadic(fixed) = &code.arity {
        let fixed = *fixed as usize;
        let rest = build_list(state, &args[fixed..])?;
        state.registers[base + fixed] = rest;
    }

    state.frames.push(CallFrame {
        code_id,
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: closure_heap_index(closure_val),
        return_reg,
    });
    Ok(())
}

/// The main dispatch loop. Runs until `state.frames.len() == exit_depth`.
///
/// Use `exit_depth = 0` to run until the frame stack is fully empty (top-level).
/// Use `exit_depth = N` to run a nested thunk until it returns to depth N.
fn run_loop_until(state: &mut VmState, exit_depth: usize) -> Result<TaggedValue, VmError> {
    loop {
        // Fetch current frame state.
        let (code_id, pc) = {
            let f = state.frames.last().expect("empty frame stack");
            (f.code_id, f.pc)
        };

        let code = state
            .code_store
            .get(&code_id)
            .cloned()
            .ok_or_else(|| VmError::Runtime {
                message: format!("missing CodeObject {:?}", code_id),
            })?;

        let instr = code
            .instructions
            .get(pc)
            .ok_or_else(|| VmError::Runtime {
                message: format!("PC {} out of bounds in {:?}", pc, code_id),
            })?
            .clone();

        // Advance PC before dispatch (instructions that jump will overwrite).
        state.frames.last_mut().unwrap().pc += 1;

        match instr {
            // ── Load / Store ────────────────────────────────────────────────
            Instruction::LoadImmediate { dst, val } => {
                state.set_reg(dst, val);
            }

            Instruction::LoadConst { dst, idx } => {
                let val =
                    code.constants
                        .get(idx as usize)
                        .copied()
                        .ok_or_else(|| VmError::Runtime {
                            message: format!("constant index {} out of bounds", idx),
                        })?;
                state.set_reg(dst, val);
            }

            Instruction::Move { dst, src } => {
                let val = state.reg(src);
                state.set_reg(dst, val);
            }

            Instruction::LoadClosure { dst, slot } => {
                let closure_idx = state.frames.last().and_then(|f| f.closure).ok_or_else(|| {
                    VmError::Runtime {
                        message: "LoadClosure in non-closure frame".into(),
                    }
                })?;
                let val = state
                    .heap
                    .borrow()
                    .get_vm_closure_free_var(closure_idx, slot as usize)
                    .ok_or_else(|| VmError::Runtime {
                        message: format!("closure slot {} out of range", slot),
                    })?;
                state.set_reg(dst, val);
            }

            Instruction::StoreClosure { slot, src } => {
                let val = state.reg(src);
                let closure_idx = state.frames.last().and_then(|f| f.closure).ok_or_else(|| {
                    VmError::Runtime {
                        message: "StoreClosure in non-closure frame".into(),
                    }
                })?;
                let ok = state.heap.borrow_mut().set_vm_closure_free_var(
                    closure_idx,
                    slot as usize,
                    val,
                );
                if !ok {
                    return Err(VmError::Runtime {
                        message: format!("closure slot {} out of range", slot),
                    });
                }
            }

            Instruction::LoadGlobal { dst, name } => {
                let val = state
                    .globals
                    .get(&name)
                    .ok_or_else(|| VmError::UnboundVariable { name: name.clone() })?;
                state.set_reg(dst, val);
            }

            Instruction::StoreGlobal { name, src } => {
                let val = state.reg(src);
                // set() returns Err if unbound; for set! semantics we could
                // fall back to define. For now, propagate errors.
                state
                    .globals
                    .set(&name, val)
                    .map_err(|_| VmError::UnboundVariable { name: name.clone() })?;
            }

            Instruction::Define { name, src } => {
                let val = state.reg(src);
                state.globals.define(name.to_string(), val);
            }

            // ── Closure Creation ────────────────────────────────────────────
            Instruction::MakeClosure {
                dst,
                code_id: child_id,
                free_vars,
            } => {
                let captured: Vec<TaggedValue> = free_vars.iter().map(|&r| state.reg(r)).collect();
                let closure_val = state
                    .heap
                    .borrow_mut()
                    .alloc_vm_closure(child_id.0, captured);
                state.set_reg(dst, closure_val);
            }

            // ── Control Flow ────────────────────────────────────────────────
            Instruction::Jump { target } => {
                state.frames.last_mut().unwrap().pc = target;
            }

            Instruction::JumpIf { cond, target } => {
                let val = state.reg(cond);
                if val != TaggedValue::FALSE {
                    state.frames.last_mut().unwrap().pc = target;
                }
            }

            Instruction::JumpUnless { cond, target } => {
                let val = state.reg(cond);
                if val == TaggedValue::FALSE {
                    state.frames.last_mut().unwrap().pc = target;
                }
            }

            // ── Function Calls ──────────────────────────────────────────────
            Instruction::Call { func, args, dst } => {
                let func_val = state.reg(func);
                let arg_vals: Vec<TaggedValue> = args.iter().map(|&r| state.reg(r)).collect();
                // Intercept higher-order control primitives that need VM cooperation.
                if let Some(ctrl) = vm_control_primitive(state, func_val) {
                    handle_control_primitive(state, ctrl, arg_vals, dst, false)?;
                } else if let Some(result) = try_call_primitive(state, func_val, arg_vals.clone()) {
                    state.set_reg(dst, result?);
                } else if try_invoke_continuation(state, func_val, &arg_vals)? {
                    // Continuation was invoked — stack has been replaced/appended.
                } else {
                    call_closure(state, func_val, &arg_vals, dst)?;
                }
            }

            Instruction::TailCall { func, args } => {
                let func_val = state.reg(func);
                let arg_vals: Vec<TaggedValue> = args.iter().map(|&r| state.reg(r)).collect();

                // Intercept higher-order control primitives in tail position.
                // Strategy: pop the current frame first (simulating "already returned"),
                // then dispatch the primitive as if called from the parent frame.
                if let Some(ctrl) = vm_control_primitive(state, func_val) {
                    let frame = state.frames.pop().expect("TailCall ctrl with empty stack");
                    let return_reg = frame.return_reg;
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    // Now at depth N-1. Handle with dst = return_reg (slot in frame N-2).
                    if state.frames.len() == exit_depth {
                        // No parent frame — we're at the top of this loop's scope.
                        // Run the primitive (it may push frames for body/handler).
                        handle_control_primitive(state, ctrl, arg_vals, 0, false)?;
                        // If it returned a value immediately, return it.
                        if state.frames.len() == exit_depth {
                            // Primitive returned immediately (e.g. Values) — value is in r0
                            // of... but there's no current frame. Return the value directly.
                            // For the top-level case, return UNSPECIFIED.
                            return Ok(TaggedValue::UNSPECIFIED);
                        }
                        // Otherwise a body/handler frame was pushed — let the loop drive it.
                        continue;
                    }
                    handle_control_primitive(state, ctrl, arg_vals, return_reg, false)?;
                    // If the primitive returned a value (no new frames), write it and continue.
                    // If it pushed frames (body/handler), they'll deliver via normal Return.
                    continue;
                }

                // Continuation invocation in tail position.
                if try_invoke_continuation(state, func_val, &arg_vals)? {
                    // Stack replaced — just continue.
                    continue;
                }

                // Primitives in tail position: call them, write result to current
                // frame's return_reg, then simulate a Return.
                if let Some(result) = try_call_primitive(state, func_val, arg_vals.clone()) {
                    let result = result?;
                    let frame = state.frames.pop().expect("TailCall with empty stack");
                    if state.frames.len() == exit_depth {
                        state.free_top_registers(frame.register_base, frame.num_regs);
                        return Ok(result);
                    }
                    let caller_idx = state.frames.len() - 1;
                    let return_reg = frame.return_reg;
                    state.set_reg_in_frame(caller_idx, return_reg, result);
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    continue;
                }

                let (new_code_id, _) = resolve_closure(state, func_val)?;
                let new_code = state.code_store.get(&new_code_id).cloned().ok_or_else(|| {
                    VmError::Runtime {
                        message: format!("missing CodeObject {:?}", new_code_id),
                    }
                })?;

                check_arity(&new_code.arity, arg_vals.len())?;

                // Reuse the current frame's register window.
                // If the new code needs more registers, grow the window.
                let frame = state.frames.last_mut().unwrap();
                let old_base = frame.register_base;
                let old_num = frame.num_regs;
                let new_num = new_code.num_regs;

                if new_num > old_num {
                    let extra = new_num - old_num;
                    state
                        .registers
                        .resize(state.registers.len() + extra as usize, TaggedValue::NULL);
                    state.frames.last_mut().unwrap().num_regs = new_num;
                }

                // Write fixed args into r0..r(n-1), then handle variadic rest.
                if let Arity::Variadic(fixed) = &new_code.arity {
                    let fixed = *fixed as usize;
                    for (i, val) in arg_vals[..fixed].iter().enumerate() {
                        state.registers[old_base + i] = *val;
                    }
                    let rest = build_list(state, &arg_vals[fixed..])?;
                    state.registers[old_base + fixed] = rest;
                } else {
                    for (i, val) in arg_vals.iter().enumerate() {
                        state.registers[old_base + i] = *val;
                    }
                }

                // Update frame in-place.
                let frame = state.frames.last_mut().unwrap();
                frame.code_id = new_code_id;
                frame.pc = 0;
                frame.closure = closure_heap_index(func_val);
            }

            Instruction::Apply { func, args, dst } => {
                let func_val = state.reg(func);
                let mut arg_vals: Vec<TaggedValue> = args[..args.len() - 1]
                    .iter()
                    .map(|&r| state.reg(r))
                    .collect();
                let last = state.reg(*args.last().unwrap());
                let spread =
                    state
                        .heap
                        .borrow()
                        .list_to_vec(last)
                        .ok_or_else(|| VmError::Runtime {
                            message: "apply: last argument is not a proper list".into(),
                        })?;
                arg_vals.extend(spread);
                if let Some(result) = try_call_primitive(state, func_val, arg_vals.clone()) {
                    state.set_reg(dst, result?);
                } else {
                    call_closure(state, func_val, &arg_vals, dst)?;
                }
            }

            Instruction::TailApply { func, args } => {
                let func_val = state.reg(func);
                let mut arg_vals: Vec<TaggedValue> = args[..args.len() - 1]
                    .iter()
                    .map(|&r| state.reg(r))
                    .collect();
                let last = state.reg(*args.last().unwrap());
                let spread =
                    state
                        .heap
                        .borrow()
                        .list_to_vec(last)
                        .ok_or_else(|| VmError::Runtime {
                            message: "apply: last argument is not a proper list".into(),
                        })?;
                arg_vals.extend(spread);

                if let Some(result) = try_call_primitive(state, func_val, arg_vals.clone()) {
                    let result = result?;
                    let frame = state.frames.pop().expect("TailApply with empty stack");
                    if state.frames.len() == exit_depth {
                        state.free_top_registers(frame.register_base, frame.num_regs);
                        return Ok(result);
                    }
                    let caller_idx = state.frames.len() - 1;
                    let return_reg = frame.return_reg;
                    state.set_reg_in_frame(caller_idx, return_reg, result);
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    continue;
                }

                let (new_code_id, _) = resolve_closure(state, func_val)?;
                let new_code = state.code_store.get(&new_code_id).cloned().ok_or_else(|| {
                    VmError::Runtime {
                        message: format!("missing CodeObject {:?}", new_code_id),
                    }
                })?;

                check_arity(&new_code.arity, arg_vals.len())?;

                let frame = state.frames.last_mut().unwrap();
                let old_base = frame.register_base;
                let old_num = frame.num_regs;
                let new_num = new_code.num_regs;

                if new_num > old_num {
                    let extra = new_num - old_num;
                    state
                        .registers
                        .resize(state.registers.len() + extra as usize, TaggedValue::NULL);
                    state.frames.last_mut().unwrap().num_regs = new_num;
                }

                if let Arity::Variadic(fixed) = &new_code.arity {
                    let fixed = *fixed as usize;
                    for (i, val) in arg_vals[..fixed].iter().enumerate() {
                        state.registers[old_base + i] = *val;
                    }
                    let rest = build_list(state, &arg_vals[fixed..])?;
                    state.registers[old_base + fixed] = rest;
                } else {
                    for (i, val) in arg_vals.iter().enumerate() {
                        state.registers[old_base + i] = *val;
                    }
                }

                let frame = state.frames.last_mut().unwrap();
                frame.code_id = new_code_id;
                frame.pc = 0;
                frame.closure = closure_heap_index(func_val);
            }

            Instruction::Return { val } => {
                let result = state.reg(val);
                let frame = state.frames.pop().expect("Return with empty stack");
                if state.frames.len() == exit_depth {
                    // Reached target depth — this loop is done.
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    return Ok(result);
                }
                // Write result into caller's return_reg.
                let caller_idx = state.frames.len() - 1;
                let return_reg = frame.return_reg;
                state.set_reg_in_frame(caller_idx, return_reg, result);
                // Free the callee's register window.
                state.free_top_registers(frame.register_base, frame.num_regs);
                // Pop any PromptFrames whose body just returned normally.
                pop_resolved_prompts(state);
            }

            // ── Multiple Values ─────────────────────────────────────────────
            Instruction::ReturnMulti { vals } => {
                let results: Vec<TaggedValue> = vals.iter().map(|&r| state.reg(r)).collect();
                let frame = state.frames.pop().expect("ReturnMulti with empty stack");
                state.value_buffer = results.clone();
                if state.frames.is_empty() {
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    // Return the first value (or unspecified if empty).
                    return Ok(results
                        .into_iter()
                        .next()
                        .unwrap_or(TaggedValue::UNSPECIFIED));
                }
                state.free_top_registers(frame.register_base, frame.num_regs);
            }

            Instruction::ReceiveValues { dsts } => {
                let buf = std::mem::take(&mut state.value_buffer);
                if buf.len() != dsts.len() {
                    return Err(VmError::ContinuationValueMismatch);
                }
                for (&dst, val) in dsts.iter().zip(buf) {
                    state.set_reg(dst, val);
                }
            }

            // ── Continuations ───────────────────────────────────────────────
            Instruction::CallWithPrompt {
                body,
                tag,
                handler,
                dst,
            } => {
                let tag_val = state.reg(tag);
                let handler_val = state.reg(handler);
                let body_val = state.reg(body);
                state.prompt_stack.push(PromptFrame {
                    tag: tag_val,
                    stack_depth: state.frames.len(),
                    dynamic_wind_depth: state.dynamic_winds.len(),
                    handler: handler_val,
                    dst,
                });
                call_closure(state, body_val, &[], dst)?;
            }

            Instruction::AbortToPrompt { tag, val } => {
                let tag_val = state.reg(tag);
                let abort_val = state.reg(val);

                let prompt_idx = state
                    .prompt_stack
                    .iter()
                    .rposition(|p| p.tag == tag_val)
                    .ok_or(VmError::NoMatchingPrompt)?;
                let prompt = state.prompt_stack[prompt_idx].clone();

                // Snapshot the captured frame slice + register windows.
                let cap_frames = state.frames[prompt.stack_depth..].to_vec();
                let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
                let reg_base = if !cap_frames.is_empty() {
                    cap_frames[0].register_base
                } else {
                    state.registers.len()
                };
                let cap_regs = state.registers[reg_base..].to_vec();
                let cont = VmDelimitedContinuation {
                    frames: cap_frames,
                    dynamic_winds: cap_winds,
                    registers: cap_regs,
                    base_at_capture: reg_base,
                };
                let cont_tv = state.alloc_vm_delimited_continuation(cont);

                // Run dynamic-wind exit thunks for the unwound portion.
                let exit_winds: Vec<_> = state.dynamic_winds[prompt.dynamic_wind_depth..]
                    .iter()
                    .rev()
                    .cloned()
                    .collect();
                for record in exit_winds {
                    run_thunk(state, record.after)?;
                }

                // Unwind the call stack.
                state.frames.truncate(prompt.stack_depth);
                state.dynamic_winds.truncate(prompt.dynamic_wind_depth);
                state.prompt_stack.truncate(prompt_idx);
                // Reclaim register space freed by the unwind.
                if let Some(top) = state.frames.last() {
                    state
                        .registers
                        .truncate(top.register_base + top.num_regs as usize);
                } else {
                    state.registers.clear();
                }

                // Call handler(abort_val, captured_cont) — result goes to prompt.dst.
                call_closure(state, prompt.handler, &[abort_val, cont_tv], prompt.dst)?;
            }

            Instruction::CaptureComposable { dst, tag } => {
                let tag_val = state.reg(tag);
                let prompt_idx = state
                    .prompt_stack
                    .iter()
                    .rposition(|p| p.tag == tag_val)
                    .ok_or(VmError::NoMatchingPrompt)?;
                let prompt = &state.prompt_stack[prompt_idx];

                let cap_frames = state.frames[prompt.stack_depth..].to_vec();
                let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
                let reg_base = if !cap_frames.is_empty() {
                    cap_frames[0].register_base
                } else {
                    state.registers.len()
                };
                let cap_regs = state.registers[reg_base..].to_vec();
                let cont = VmDelimitedContinuation {
                    frames: cap_frames,
                    dynamic_winds: cap_winds,
                    registers: cap_regs,
                    base_at_capture: reg_base,
                };
                let cont_tv = state.alloc_vm_delimited_continuation(cont);
                state.set_reg(dst, cont_tv);
            }

            Instruction::InvokeContinuation {
                cont,
                val,
                composable,
            } => {
                let cont_tv = state.reg(cont);
                let deliver_val = state.reg(val);

                if composable {
                    // Composable (delimited): append captured frames to current stack.
                    let dc = state
                        .get_vm_delimited_continuation(cont_tv)
                        .ok_or_else(|| VmError::TypeError {
                            message: "InvokeContinuation: not a delimited continuation".into(),
                        })?;

                    // Run wind entry thunks for the newly-appended winds.
                    let enter_winds = dc.dynamic_winds.clone();
                    for record in &enter_winds {
                        run_thunk(state, record.before)?;
                    }

                    // Relocate captured frames onto the end of the register array.
                    let new_base = state.registers.len();
                    let old_base = dc.base_at_capture;
                    let shift = new_base.wrapping_sub(old_base);
                    state.registers.extend_from_slice(&dc.registers);
                    let mut new_frames: Vec<CallFrame> = dc.frames.clone();
                    for f in &mut new_frames {
                        f.register_base = f.register_base.wrapping_add(shift);
                    }
                    state.frames.extend(new_frames);
                    state.dynamic_winds.extend(enter_winds);

                    // Deliver the value into the topmost appended frame's return slot.
                    // The continuation expects the value where the sub-call result would land.
                    if let Some(top) = state.frames.last() {
                        let ret_reg = top.return_reg;
                        // Write into the *caller* of the top frame (below the top).
                        let n = state.frames.len();
                        if n >= 2 {
                            let caller_base = state.frames[n - 2].register_base;
                            state.registers[caller_base + ret_reg as usize] = deliver_val;
                        }
                    }
                } else {
                    // Non-composable (call/cc): replace entire stack.
                    let cc =
                        state
                            .get_vm_continuation(cont_tv)
                            .ok_or_else(|| VmError::TypeError {
                                message: "InvokeContinuation: not a full continuation".into(),
                            })?;

                    // Wind transition.
                    let target_winds = cc.dynamic_winds.clone();
                    run_wind_transition(state, &target_winds)?;

                    // Restore the full state snapshot.
                    state.registers = cc.registers.clone();
                    state.frames = cc.frames.clone();
                    state.dynamic_winds = cc.dynamic_winds.clone();
                    state.prompt_stack = cc.prompt_stack.clone();

                    // Deliver value into deliver_reg of the top frame.
                    if let Some(top) = state.frames.last() {
                        let base = top.register_base;
                        state.registers[base + cc.deliver_reg as usize] = deliver_val;
                    }
                }
            }

            // ── Primitives ──────────────────────────────────────────────────
            Instruction::CallPrimitive { .. } => {
                return Err(VmError::Runtime {
                    message: "CallPrimitive not yet wired (A3+)".into(),
                });
            }

            Instruction::AllocCell { dst, src } => {
                let val = state.reg(src);
                let cell = state.heap.borrow_mut().alloc_mutable_cell(val);
                state.set_reg(dst, cell);
            }

            Instruction::ReadCell { dst, cell } => {
                let cell_tv = state.reg(cell);
                let val = state
                    .heap
                    .borrow()
                    .read_mutable_cell(cell_tv)
                    .ok_or_else(|| VmError::Runtime {
                        message: "ReadCell: not a MutableCell".into(),
                    })?;
                state.set_reg(dst, val);
            }

            Instruction::WriteCell { cell, src } => {
                let cell_tv = state.reg(cell);
                let val = state.reg(src);
                let ok = state.heap.borrow().write_mutable_cell(cell_tv, val);
                if !ok {
                    return Err(VmError::Runtime {
                        message: "WriteCell: not a MutableCell".into(),
                    });
                }
            }

            Instruction::Nop => {}
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn resolve_closure(
    state: &VmState,
    val: TaggedValue,
) -> Result<(CodeObjectId, Vec<TaggedValue>), VmError> {
    state
        .heap
        .borrow()
        .get_vm_closure(val)
        .map(|(id, free_vars)| (CodeObjectId(id), free_vars))
        .ok_or_else(|| VmError::TypeError {
            message: format!("expected a procedure, got {}", val.type_name()),
        })
}

fn closure_heap_index(val: TaggedValue) -> Option<patina_core::tagged_value::HeapIndex> {
    // VmClosures are TAG_OBJECT (generic heap objects).
    if val.is_object() {
        Some(val.heap_index())
    } else {
        None
    }
}

fn check_arity(arity: &Arity, n: usize) -> Result<(), VmError> {
    if !arity.accepts(n) {
        return Err(VmError::ArityMismatch {
            expected: match arity {
                Arity::Fixed(k) => format!("{}", k),
                Arity::Variadic(k) => format!("at least {}", k),
            },
            got: n,
        });
    }
    Ok(())
}

fn build_list(state: &mut VmState, items: &[TaggedValue]) -> Result<TaggedValue, VmError> {
    let mut result = TaggedValue::NULL;
    for &item in items.iter().rev() {
        result = state.heap.borrow_mut().alloc_pair(item, result);
    }
    Ok(result)
}

/// Run a thunk (0-arg closure) to completion, returning its result.
///
/// Pushes the thunk's frame, runs the execution loop until that frame returns,
/// then returns the result. Used by `dynamic-wind` and wind-transition hooks.
fn run_thunk(state: &mut VmState, thunk: TaggedValue) -> Result<TaggedValue, VmError> {
    let depth_before = state.frames.len();
    // Use slot 0 as the scratch return register.
    // If there is no current frame (depth = 0), call_closure will push the thunk
    // frame and run_loop_until will stop when frames returns to 0.
    let scratch_ret = 0u16;
    call_closure(state, thunk, &[], scratch_ret)?;
    // run_loop_until returns when frames.len() drops back to depth_before.
    // At depth_before = 0, return from the bottom frame triggers exit.
    run_loop_until(state, depth_before)
}

/// Handle a VM-intercepted control primitive call.
///
/// `is_tail` is currently unused (all are handled as non-tail for A6).
fn handle_control_primitive(
    state: &mut VmState,
    ctrl: VmControlPrimitive,
    args: Vec<TaggedValue>,
    dst: u16,
    _is_tail: bool,
) -> Result<(), VmError> {
    match ctrl {
        VmControlPrimitive::DynamicWind => {
            if args.len() != 3 {
                return Err(VmError::ArityMismatch {
                    expected: "3".into(),
                    got: args.len(),
                });
            }
            run_thunk(state, args[0])?;
            let wind_depth = state.dynamic_winds.len();
            state.dynamic_winds.push(DynamicWindRecord {
                before: args[0],
                after: args[2],
            });
            let result = run_thunk(state, args[1])?;
            // If the body aborted, the abort already ran exit thunks and
            // truncated dynamic_winds. Only do cleanup if our record is still there.
            if state.dynamic_winds.len() > wind_depth {
                state.dynamic_winds.pop();
                run_thunk(state, args[2])?;
            }
            state.set_reg(dst, result);
        }

        VmControlPrimitive::CallWithContinuationPrompt => {
            // (call-with-continuation-prompt body tag [handler])
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let body = args[0];
            let tag = if args.len() > 1 {
                args[1]
            } else {
                // No tag: use a fresh default tag (not ideal but functional for A6)
                use patina_core::cps_expr::PromptTag;
                state
                    .heap
                    .borrow_mut()
                    .alloc_prompt_tag(std::rc::Rc::new(PromptTag::new("default")))
            };
            let handler = if args.len() > 2 {
                args[2]
            } else {
                TaggedValue::FALSE
            };
            state.prompt_stack.push(PromptFrame {
                tag,
                stack_depth: state.frames.len(),
                dynamic_wind_depth: state.dynamic_winds.len(),
                handler,
                dst,
            });
            call_closure(state, body, &[], dst)?;
            // When body returns normally, pop_resolved_prompts will clean up the prompt.
        }

        VmControlPrimitive::AbortCurrentContinuation => {
            // (abort-current-continuation tag val...)
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let tag = args[0];
            let val = if args.len() > 1 {
                args[1]
            } else {
                TaggedValue::UNSPECIFIED
            };

            let prompt_idx = state
                .prompt_stack
                .iter()
                .rposition(|p| p.tag == tag)
                .ok_or(VmError::NoMatchingPrompt)?;
            let prompt = state.prompt_stack[prompt_idx].clone();

            let cap_frames = state.frames[prompt.stack_depth..].to_vec();
            let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
            let reg_base = if !cap_frames.is_empty() {
                cap_frames[0].register_base
            } else {
                state.registers.len()
            };
            let cap_regs = state.registers[reg_base..].to_vec();
            let cont = VmDelimitedContinuation {
                frames: cap_frames,
                dynamic_winds: cap_winds,
                registers: cap_regs,
                base_at_capture: reg_base,
            };
            let cont_tv = state.alloc_vm_delimited_continuation(cont);

            let exit_winds: Vec<_> = state.dynamic_winds[prompt.dynamic_wind_depth..]
                .iter()
                .rev()
                .cloned()
                .collect();
            for record in exit_winds {
                run_thunk(state, record.after)?;
            }

            state.frames.truncate(prompt.stack_depth);
            state.dynamic_winds.truncate(prompt.dynamic_wind_depth);
            state.prompt_stack.truncate(prompt_idx);
            if let Some(top) = state.frames.last() {
                state
                    .registers
                    .truncate(top.register_base + top.num_regs as usize);
            } else {
                state.registers.clear();
            }

            call_closure(state, prompt.handler, &[val, cont_tv], prompt.dst)?;
        }

        VmControlPrimitive::CallWithCurrentContinuation => {
            // (call/cc proc) — capture the current continuation and call proc with it
            if args.len() != 1 {
                return Err(VmError::ArityMismatch {
                    expected: "1".into(),
                    got: args.len(),
                });
            }
            let proc = args[0];
            // Capture a full continuation: snapshot of entire current state
            let cont = VmContinuation {
                frames: state.frames.clone(),
                dynamic_winds: state.dynamic_winds.clone(),
                prompt_stack: state.prompt_stack.clone(),
                registers: state.registers.clone(),
                deliver_reg: dst,
            };
            let cont_tv = state.alloc_vm_continuation(cont);
            // Call proc with the continuation object
            call_closure(state, proc, &[cont_tv], dst)?;
        }

        VmControlPrimitive::Values => {
            // (values v1 v2 ...) — return multiple values via value_buffer
            state.value_buffer = args;
            let primary = state
                .value_buffer
                .first()
                .copied()
                .unwrap_or(TaggedValue::UNSPECIFIED);
            state.set_reg(dst, primary);
        }

        VmControlPrimitive::CallWithValues => {
            // (call-with-values producer consumer)
            if args.len() != 2 {
                return Err(VmError::ArityMismatch {
                    expected: "2".into(),
                    got: args.len(),
                });
            }
            let producer = args[0];
            let consumer = args[1];
            // Run producer (0 args), collect multiple values from value_buffer
            let primary = run_thunk(state, producer)?;
            let produced_vals = if state.value_buffer.is_empty() {
                vec![primary]
            } else {
                std::mem::take(&mut state.value_buffer)
            };
            call_closure(state, consumer, &produced_vals, dst)?;
        }
    }
    Ok(())
}

/// Pop any `PromptFrame`s whose `stack_depth` now exceeds the current frame depth
/// (i.e., the prompt's body returned normally).
fn pop_resolved_prompts(state: &mut VmState) {
    while let Some(pf) = state.prompt_stack.last() {
        if pf.stack_depth >= state.frames.len() {
            state.prompt_stack.pop();
        } else {
            break;
        }
    }
}

/// Run wind-transition thunks when moving from current dynamic-wind state
/// to `target_winds`.
fn run_wind_transition(
    state: &mut VmState,
    target_winds: &[DynamicWindRecord],
) -> Result<(), VmError> {
    // Find the common prefix (matched by `before` thunk identity).
    let common = state
        .dynamic_winds
        .iter()
        .zip(target_winds.iter())
        .take_while(|(a, b)| a.before == b.before)
        .count();

    // Run 'after' thunks for records being exited (innermost first).
    let exit_winds: Vec<_> = state.dynamic_winds[common..]
        .iter()
        .rev()
        .cloned()
        .collect();
    for record in exit_winds {
        run_thunk(state, record.after)?;
    }

    // Run 'before' thunks for records being entered (outermost first).
    let enter_winds: Vec<_> = target_winds[common..].to_vec();
    for record in enter_winds {
        run_thunk(state, record.before)?;
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// VmApplyContext — implements ApplyContext for heap-only primitives
// ─────────────────────────────────────────────────────────────────────────────

/// Minimal `ApplyContext` implementation for the VM.
///
/// Heap-only primitives (~290) work fine. Higher-order primitives that call
/// back via `apply_proc` or `eval_expr` are stubbed — full support in A4+.
struct VmApplyContext<'a> {
    heap: &'a SharedHeap,
}

impl<'a> patina_primitives::ApplyContext for VmApplyContext<'a> {
    fn heap(&self) -> &SharedHeap {
        self.heap
    }

    fn apply_proc(
        &self,
        _proc: TaggedValue,
        _args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        Err(patina_primitives::EvalError::InvalidSyntax(
            "higher-order primitive apply_proc not yet supported in VM (A4+)".into(),
        ))
    }

    fn eval_expr(
        &self,
        _expr: TaggedValue,
        _env: &Rc<patina_core::environment::Environment>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        Err(patina_primitives::EvalError::InvalidSyntax(
            "eval not yet supported in VM (A4+)".into(),
        ))
    }

    fn load_scheme_library(
        &self,
        _name: &[String],
    ) -> Result<Rc<patina_core::library::Library>, patina_primitives::EvalError> {
        Err(patina_primitives::EvalError::InvalidSyntax(
            "load_scheme_library not yet supported in VM (A4+)".into(),
        ))
    }
}

/// Recognized VM-intercepted control primitives.
enum VmControlPrimitive {
    DynamicWind,
    CallWithContinuationPrompt,
    AbortCurrentContinuation,
    CallWithCurrentContinuation,
    CallWithValues,
    Values,
}

/// If `func_val` is a VM-intercepted control primitive, return which one.
fn vm_control_primitive(state: &VmState, func_val: TaggedValue) -> Option<VmControlPrimitive> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    let Procedure::Primitive { qualified_name, .. } = proc.as_ref() else {
        return None;
    };
    match qualified_name.as_ref() {
        "patina.internal.control/dynamic-wind" => Some(VmControlPrimitive::DynamicWind),
        "patina.internal.control/call-with-continuation-prompt" => {
            Some(VmControlPrimitive::CallWithContinuationPrompt)
        }
        "patina.internal.control/abort-current-continuation" => {
            Some(VmControlPrimitive::AbortCurrentContinuation)
        }
        "patina.internal.control/call-with-current-continuation"
        | "patina.internal.control/call/cc" => {
            Some(VmControlPrimitive::CallWithCurrentContinuation)
        }
        "patina.internal.control/call-with-values" => Some(VmControlPrimitive::CallWithValues),
        "patina.internal.control/values" => Some(VmControlPrimitive::Values),
        _ => None,
    }
}

/// Try to invoke `func_val` as a continuation. Returns `Ok(true)` if it was
/// a continuation and was invoked (stack has been replaced/appended),
/// `Ok(false)` if not a continuation.
fn try_invoke_continuation(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Result<bool, VmError> {
    let deliver_val = args.first().copied().unwrap_or(TaggedValue::UNSPECIFIED);

    // Full (call/cc) continuation?
    if let Some(cc) = state.get_vm_continuation(func_val) {
        let target_winds = cc.dynamic_winds.clone();
        run_wind_transition(state, &target_winds)?;
        state.registers = cc.registers.clone();
        state.frames = cc.frames.clone();
        state.dynamic_winds = cc.dynamic_winds.clone();
        state.prompt_stack = cc.prompt_stack.clone();
        // Deliver value into deliver_reg of the top frame (where call/cc's
        // result was expected).
        if let Some(top) = state.frames.last() {
            let base = top.register_base;
            state.registers[base + cc.deliver_reg as usize] = deliver_val;
        }
        return Ok(true);
    }

    // Delimited (composable) continuation?
    if let Some(dc) = state.get_vm_delimited_continuation(func_val) {
        let enter_winds = dc.dynamic_winds.clone();
        for record in &enter_winds {
            run_thunk(state, record.before)?;
        }
        let new_base = state.registers.len();
        let old_base = dc.base_at_capture;
        let shift = new_base.wrapping_sub(old_base);
        state.registers.extend_from_slice(&dc.registers);
        let mut new_frames: Vec<CallFrame> = dc.frames.clone();
        for f in &mut new_frames {
            f.register_base = f.register_base.wrapping_add(shift);
        }
        state.frames.extend(new_frames);
        state.dynamic_winds.extend(enter_winds);
        if let Some(top) = state.frames.last() {
            let ret_reg = top.return_reg;
            let n = state.frames.len();
            if n >= 2 {
                let caller_base = state.frames[n - 2].register_base;
                state.registers[caller_base + ret_reg as usize] = deliver_val;
            }
        }
        return Ok(true);
    }

    Ok(false)
}

/// Try to call `func_val` as a primitive. Returns `Some(result)` if it was a
/// primitive, `None` if it's a VM closure (caller should push a frame instead).
fn try_call_primitive(
    state: &VmState,
    func_val: TaggedValue,
    args: Vec<TaggedValue>,
) -> Option<Result<TaggedValue, VmError>> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    let Procedure::Primitive { qualified_name, .. } = proc.as_ref() else {
        return None;
    };
    let ctx = VmApplyContext { heap: &state.heap };
    let result = state
        .primitive_registry
        .apply_tagged(qualified_name, args, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        });
    Some(result)
}
