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

use patina_core::tagged_value::TaggedValue;
use patina_core::{SlotVisitor, Trace};
use std::cell::Cell;
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
///
/// Its trace (derived, #686) reports its closure, a slot like a register,
/// and its code by id ([`FrameCode`], #836).
#[derive(Debug, Clone, Trace)]
pub struct CallFrame {
    /// Program counter — index of the next instruction to execute.
    #[trace(skip, reason = "an instruction index")]
    pub pc: usize,
    /// Offset into `ExecutionState::registers` where this frame's r0 lives.
    #[trace(
        skip,
        reason = "a register index; the window is traced with the register file"
    )]
    pub register_base: usize,
    /// Number of registers owned by this frame.
    #[trace(skip, reason = "a count")]
    pub num_regs: u16,
    /// The closure this activation runs, a `VmClosure`, or
    /// [`CallFrame::NO_CLOSURE`] for code that runs none: a program's
    /// top-level code, which reads the machine's globals, and the stubs.
    /// Top-level code compiled in another environment, such as a library's
    /// body form or a host namespace's form, runs a closure made retired
    /// whose globals are that environment (#779). A slot like a register,
    /// and traced as one (#682).
    pub closure: Cell<TaggedValue>,
    /// Register in the *caller's* frame where `Return` should write its result.
    /// When there is no caller (top-level), this field is unused.
    #[trace(skip, reason = "a register number")]
    pub return_reg: Reg,
    /// The code object this frame executes, resolved once at push time.
    /// Code objects are immutable after loading, so caching the `Rc` here is
    /// safe and lets the dispatch loop fetch instructions without a
    /// `code_store` lookup per instruction. Its id is `code.id` — the
    /// frame stores no separate copy that could drift out of sync. Traced by
    /// that id ([`FrameCode`]): the unit's link table through
    /// `trace_code_ids`, the constants through `code_store`, which keeps
    /// every code object a frame or a captured continuation can run (#338).
    pub code: FrameCode,
}

/// A frame's code (#836): what the dispatch loop runs, and the frame's edge
/// to its unit's link table. Its trace reports the code's id, which the
/// collector resolves to the unit's table (`VmState::trace_code_ids`); the
/// constants stay traced from `code_store` until 4e. A newtype, because the
/// derive cannot report an id, an `Rc<CodeObject>` unskipped would trace
/// the constants through it, and a field of `CodeObject` that reported its
/// id would be reported from `code_store` too, which would keep every
/// loaded unit's table.
///
/// It is the `Rc` alone, so `CallFrame` keeps its size, and the
/// continuation snapshots that copy frames their charge (#606).
#[derive(Debug, Clone)]
pub struct FrameCode(pub Rc<CodeObject>);

impl std::ops::Deref for FrameCode {
    type Target = Rc<CodeObject>;

    #[inline(always)]
    fn deref(&self) -> &Rc<CodeObject> {
        &self.0
    }
}

/// Its one field by name (`scripts/check_gc_trace_names.py`).
impl Trace for FrameCode {
    fn trace<V: SlotVisitor + ?Sized>(&self, visitor: &mut V) {
        let FrameCode(code) = self;
        visitor.code(code.id.0);
    }
}

impl CallFrame {
    /// What a frame that runs no closure holds as its `closure`.
    pub const NO_CLOSURE: TaggedValue = TaggedValue::FALSE;

    /// The closure this frame runs, if it runs one.
    #[inline(always)]
    pub fn closure(&self) -> Option<TaggedValue> {
        let closure = self.closure.get();
        closure.is_object().then_some(closure)
    }
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
    /// Slot index corresponds to the `slot` operand in `LoadClosure`.
    pub free_vars: Vec<TaggedValue>,
}
