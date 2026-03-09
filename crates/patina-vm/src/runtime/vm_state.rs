//! `VmState` — the complete mutable state of the VM during execution.
//!
//! See VM_RUNTIME.md §core-data-structures.

use crate::error::VmError;
use crate::types::code_object::CodeObject;
use crate::types::continuation::{DynamicWindRecord, PromptFrame};
use crate::types::{CallFrame, CodeObjectId};
use patina_core::environment::Environment;
use patina_core::heap::{SharedHeap, new_shared_heap};
use patina_core::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The complete mutable state of the VM during execution.
///
/// All registers live in a single flat `Vec<TaggedValue>`. Each `CallFrame`
/// owns a contiguous slice via `register_base` and `num_regs`. Frames are
/// pushed / popped on `Call` / `Return`; for tail calls the current frame is
/// reused in-place (see VM_ISA.md §5).
///
/// `value_buffer` is the side channel for multiple return values (`values` /
/// `call-with-values`). It is populated by `ReturnMulti` and consumed by
/// `ReceiveValues`. Using a side buffer avoids heap allocation for the common
/// single-value case.
pub struct VmState {
    // ── Register file ─────────────────────────────────────────────────────────
    /// Flat register array. Frame windows are slices of this vec.
    ///
    /// The vec grows on demand (amortised O(1)); base offsets remain valid
    /// because we never shrink or reorder the array. (VM_DECISIONS.md §registers)
    pub registers: Vec<TaggedValue>,

    // ── Call stack ────────────────────────────────────────────────────────────
    /// The call stack. The currently-executing frame is `frames.last()`.
    pub frames: Vec<CallFrame>,

    // ── Multiple-value side buffer ────────────────────────────────────────────
    /// Populated by `ReturnMulti`, consumed by `ReceiveValues`.
    /// Empty in the common single-value case — no heap allocation needed.
    pub value_buffer: Vec<TaggedValue>,

    // ── Continuation machinery ────────────────────────────────────────────────
    /// Stack of active continuation prompts (SRFI-226).
    /// The innermost (most recent) prompt is at the back.
    pub prompt_stack: Vec<PromptFrame>,

    /// Stack of active `dynamic-wind` records.
    /// The innermost wind is at the back.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    // ── Code store ───────────────────────────────────────────────────────────
    /// All compiled `CodeObject`s, keyed by their id.
    /// Loaded at startup from the compiler output; nested lambdas are
    /// inserted here when `MakeClosure` is first executed.
    pub code_store: HashMap<CodeObjectId, Rc<CodeObject>>,

    // ── Global environment and heap ───────────────────────────────────────────
    /// The global variable environment, shared with the library loader.
    pub globals: Rc<RefCell<Environment>>,

    /// The heap, shared with `patina-runtime` primitives.
    pub heap: SharedHeap,
}

impl VmState {
    /// Create a fresh `VmState` with an empty environment and heap.
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

    /// Load a compiled `CodeObject` into the code store.
    pub fn load(&mut self, code: CodeObject) {
        let id = code.id;
        self.code_store.insert(id, Rc::new(code));
    }

    /// Read register `reg` in the current (top) frame.
    ///
    /// # Panics
    /// Panics if there is no current frame or `reg` is out of range.
    #[inline]
    pub fn reg(&self, reg: u16) -> TaggedValue {
        let frame = self.frames.last().expect("no active frame");
        self.registers[frame.register_base + reg as usize]
    }

    /// Write register `reg` in the current (top) frame.
    ///
    /// # Panics
    /// Panics if there is no current frame or `reg` is out of range.
    #[inline]
    pub fn set_reg(&mut self, reg: u16, val: TaggedValue) {
        let base = self.frames.last().expect("no active frame").register_base;
        self.registers[base + reg as usize] = val;
    }

    /// Write register `reg` in the frame at `frame_idx` (0 = bottom of stack).
    #[inline]
    pub fn set_reg_in(&mut self, frame_idx: usize, reg: u16, val: TaggedValue) {
        let base = self.frames[frame_idx].register_base;
        self.registers[base + reg as usize] = val;
    }

    /// Allocate a new register window of `num_regs` slots at the end of the
    /// register array and return the base offset.
    pub fn alloc_registers(&mut self, num_regs: u16) -> usize {
        let base = self.registers.len();
        self.registers
            .resize(base + num_regs as usize, TaggedValue::NULL);
        base
    }

    /// Release the top frame's register window (on `Return`).
    ///
    /// Only valid to call after `frames.pop()`.
    pub fn free_top_registers(&mut self, base: usize, num_regs: u16) {
        debug_assert_eq!(
            self.registers.len(),
            base + num_regs as usize,
            "register window was not at the top of the array"
        );
        self.registers.truncate(base);
    }

    /// Look up the current `CodeObject`.
    ///
    /// # Errors
    /// Returns `VmError::Runtime` if the code id is not in the store (indicates
    /// a compiler bug).
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
