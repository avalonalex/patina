//! `VmState` — the complete mutable state of the VM during execution.
//!
//! See VM_RUNTIME.md §core-data-structures.

use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject};
use crate::types::continuation::{DynamicWindRecord, PromptFrame};
use crate::types::instruction::Instruction;
use crate::types::{CallFrame, CodeObjectId};
use patina_core::environment::Environment;
use patina_core::heap::{SharedHeap, new_shared_heap};
use patina_core::tagged_value::TaggedValue;
use std::cell::RefCell;
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
    pub globals: Rc<RefCell<Environment>>,
    /// The heap, shared with `patina-runtime` primitives.
    pub heap: SharedHeap,
}

impl VmState {
    pub fn new(globals: Rc<RefCell<Environment>>) -> Self {
        Self {
            registers: Vec::new(),
            frames: Vec::new(),
            value_buffer: Vec::new(),
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            code_store: HashMap::new(),
            globals,
            heap: new_shared_heap(),
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

    run_loop(state)
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

/// The main dispatch loop. Runs until the frame stack empties.
fn run_loop(state: &mut VmState) -> Result<TaggedValue, VmError> {
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
                    .borrow()
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
                    .borrow()
                    .set(&name, val)
                    .map_err(|_| VmError::UnboundVariable { name: name.clone() })?;
            }

            Instruction::Define { name, src } => {
                let val = state.reg(src);
                state.globals.borrow().define(name.to_string(), val);
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
                call_closure(state, func_val, &arg_vals, dst)?;
                // Control transferred to new frame; loop continues.
            }

            Instruction::TailCall { func, args } => {
                let func_val = state.reg(func);
                let arg_vals: Vec<TaggedValue> = args.iter().map(|&r| state.reg(r)).collect();

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

                // Write args sequentially into r0..r(n-1).
                // Pass 4 ensures no overlap with source registers when is_tail=true.
                for (i, val) in arg_vals.iter().enumerate() {
                    state.registers[old_base + i] = *val;
                }

                // Update frame in-place.
                let frame = state.frames.last_mut().unwrap();
                frame.code_id = new_code_id;
                frame.pc = 0;
                frame.closure = closure_heap_index(func_val);
            }

            Instruction::Return { val } => {
                let result = state.reg(val);
                let frame = state.frames.pop().expect("Return with empty stack");
                if state.frames.is_empty() {
                    // Top-level return — done.
                    state.free_top_registers(frame.register_base, frame.num_regs);
                    return Ok(result);
                }
                // Write result into caller's return_reg.
                let caller_idx = state.frames.len() - 1;
                let return_reg = frame.return_reg;
                state.set_reg_in_frame(caller_idx, return_reg, result);
                // Free the callee's register window.
                state.free_top_registers(frame.register_base, frame.num_regs);
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
            Instruction::CallWithPrompt { .. }
            | Instruction::AbortToPrompt { .. }
            | Instruction::CaptureComposable { .. }
            | Instruction::InvokeContinuation { .. } => {
                return Err(VmError::Runtime {
                    message: "continuation instructions not yet implemented (A3+)".into(),
                });
            }

            // ── Primitives ──────────────────────────────────────────────────
            Instruction::CallPrimitive { .. } => {
                return Err(VmError::Runtime {
                    message: "CallPrimitive not yet wired (A3+)".into(),
                });
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
