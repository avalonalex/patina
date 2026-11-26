//! Core VM data types: instructions, code objects, call frames, closures,
//! continuations, and supporting structures.
//!
//! These types define the VM's data model as specified in VM_ISA.md and
//! VM_RUNTIME.md. No execution logic lives here — only type definitions.

pub mod code_object;
pub mod continuation;
mod depth;
pub(crate) mod frame_stack;
pub mod instruction;

pub use code_object::{Arity, CodeObject, CodeObjectId, ConstIdx};
pub use continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
pub(crate) use depth::Depth;
pub use frame_stack::FrameStack;
pub use instruction::Instruction;

use patina_core::tagged_value::TaggedValue;
use patina_core::{SlotVisitor, Trace};
use std::rc::Rc;

/// Register index (frame-relative, u16 allows up to 65535 registers per frame)
pub type Reg = u16;

/// Index into the CodeObject's `constants` pool
pub type LabelId = usize; // instruction index (absolute within a CodeObject)

// ─────────────────────────────────────────────────────────────────────────────
// CallFrame
// ─────────────────────────────────────────────────────────────────────────────

/// A view of one activation record on the register stack: what its header
/// holds, read out.
///
/// Until GC stage 4d.7b (#908) this was the record itself, a vector of
/// them beside the registers, and its `Clone` was what a continuation
/// copied. A frame is now five words of the register stack below its
/// registers ([`FrameStack`]), and this is only how the step tracer, the
/// collector's walk and the machine's rarer paths read one: nothing stores
/// it, and a write goes through the stack.
///
/// `return_reg` is kept in the *callee's* frame and names the register in
/// the *caller's* window where the return value should be written, so
/// `Return` does not inspect the caller's frame.
#[derive(Debug, Clone, Copy)]
pub struct CallFrame<'a> {
    /// Program counter: the index of the next instruction to execute.
    pub pc: usize,
    /// The index in the register stack's words of this frame's r0. Its
    /// header is the five words below.
    pub register_base: usize,
    /// Number of registers this frame's window holds: its code's, or more
    /// where a tail call reused a larger window.
    pub num_regs: u16,
    /// The closure this activation runs, a `VmClosure`, or
    /// [`CallFrame::NO_CLOSURE`] for code that runs none: top-level code,
    /// wherever it was compiled (the program, a library's body form, a host
    /// namespace's form), which reads through its unit's link table, and the
    /// stubs (#779, #836). Read only for a closure's free variables
    /// (`LoadClosure`), never for an environment. A word of the stack like
    /// a register, and traced as one (#682).
    pub closure: TaggedValue,
    /// Register in the *caller's* frame where `Return` should write its result.
    /// When there is no caller (top-level), this field is unused.
    pub return_reg: Reg,
    /// The code object this frame executes. The frame's `code` word holds
    /// its address, and the stack's side vector the `Rc` this borrows,
    /// which keeps the code's unit loaded while the frame or a captured
    /// copy of it lives (#338).
    pub code: &'a Rc<CodeObject>,
}

/// A frame's code, as the register stack's side vector holds it (#836,
/// #908): what keeps the frame's unit loaded, and the frame's edge to its
/// unit's link table. Its trace reports the code's id, which the
/// collector resolves to the unit's table (`VmState::trace_code_ids`); the
/// constants stay traced from `code_store` until 4e. A newtype, because the
/// derive cannot report an id, an `Rc<CodeObject>` unskipped would trace
/// the constants through it, and a field of `CodeObject` that reported its
/// id would be reported from `code_store` too, which would keep every
/// loaded unit's table.
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

impl CallFrame<'_> {
    /// What a frame that runs no closure holds as its `closure`.
    pub const NO_CLOSURE: TaggedValue = TaggedValue::FALSE;

    /// The `return_reg` of a frame whose value goes to no register of its
    /// caller (#714): a higher-order primitive's callback, whose value the
    /// dispatch loop hands the primitive (`run_apply_proc`). No window
    /// reaches it, since a window's registers number at most `u16::MAX`,
    /// so it is past the end of the register stack, and `return_into` writes
    /// no register for it. A continuation that re-enters the callback after
    /// the primitive has returned then returns into nothing.
    pub(crate) const NO_RETURN: Reg = Reg::MAX;

    /// The closure this frame runs, if it runs one.
    #[inline(always)]
    pub fn closure(&self) -> Option<TaggedValue> {
        self.closure.is_object().then_some(self.closure)
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
