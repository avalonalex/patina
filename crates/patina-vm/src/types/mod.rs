//! Core VM data types: instructions, code objects, call frames, closures,
//! continuations, and supporting structures.
//!
//! These types define the VM's data model as specified in VM_ISA.md and
//! VM_RUNTIME.md. No execution logic lives here — only type definitions.

pub mod code_object;
pub mod continuation;
pub mod instruction;

pub use code_object::{Arity, CodeObject, CodeObjectId, ConstIdx, GlobalCacheEntry};
pub use continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
pub use instruction::Instruction;

use patina_core::tagged_value::HeapIndex;
use patina_core::tagged_value::TaggedValue;
use std::rc::Rc;

/// Register index (frame-relative, u16 allows up to 65535 registers per frame)
pub type Reg = u16;

/// Index into the CodeObject's `constants` pool
pub type LabelId = usize; // instruction index (absolute within a CodeObject)

// ─────────────────────────────────────────────────────────────────────────────
// CallFrame
// ─────────────────────────────────────────────────────────────────────────────

/// A single activation record on the call stack.
///
/// `CallFrame` MUST always implement `Clone`. This is a non-negotiable invariant:
/// stack-snapshot continuations (`CaptureComposable`, `AbortToPrompt`) need to
/// clone the entire call stack. See VM_DECISIONS.md §4 and VM_ISA.md §2.2.
///
/// `return_reg` is stored in the *callee's* frame and names the register in the
/// *caller's* window where the return value should be written. The `Return`
/// instruction reads this field without needing to inspect the caller's frame.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// Program counter — index of the next instruction to execute.
    pub pc: usize,
    /// Offset into `VmState::registers` where this frame's r0 lives.
    pub register_base: usize,
    /// Number of registers owned by this frame.
    pub num_regs: u16,
    /// The closure for this activation (if the function is a closure).
    /// `None` for top-level functions with no free variables.
    pub closure: Option<HeapIndex>,
    /// Register in the *caller's* frame where `Return` should write its result.
    /// When there is no caller (top-level), this field is unused.
    pub return_reg: Reg,
    /// The code object this frame executes, resolved once at push time.
    /// Code objects are immutable after loading, so caching the `Rc` here is
    /// safe and lets the dispatch loop fetch instructions without a
    /// `code_store` lookup per instruction. Its id is `code.id` — the
    /// frame stores no separate copy that could drift out of sync.
    pub code: Rc<CodeObject>,
}

// ─────────────────────────────────────────────────────────────────────────────
// VmClosure
// ─────────────────────────────────────────────────────────────────────────────

/// A flat closure: a code object plus a snapshot of captured free variables.
///
/// Stored in the heap as `HeapObjectData::VmClosure`. Mutable captured
/// variables are heap-boxed as `MutableCell` — the cell pointer is what appears
/// in `free_vars`, not the variable's current value.
///
/// See VM_ISA.md §2.4.
#[derive(Debug, Clone)]
pub struct VmClosure {
    /// The compiled code for this closure.
    pub code_id: CodeObjectId,
    /// Captured values indexed by closure slot.
    /// Slot index corresponds to the `slot` operand in `LoadClosure`/`StoreClosure`.
    pub free_vars: Vec<TaggedValue>,
}
