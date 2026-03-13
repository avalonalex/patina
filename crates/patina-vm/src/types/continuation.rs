//! Continuation-related types: prompt frames, dynamic-wind records,
//! and captured (delimited) continuations.
//!
//! See VM_ISA.md §6 and VM_RUNTIME.md §continuations.

use super::{CallFrame, Reg};
use patina_core::tagged_value::TaggedValue;

/// An entry on the prompt stack, recording the dynamic context needed to
/// handle an `AbortToPrompt` or to restore after normal return.
///
/// Created by `CallWithPrompt`, popped on normal return or abort.
#[derive(Debug, Clone)]
pub struct PromptFrame {
    /// The prompt tag (opaque heap object, compared by identity).
    pub tag: TaggedValue,

    /// `VmState::frames.len()` at the point where the prompt was established.
    /// `AbortToPrompt` unwinds to this depth.
    pub stack_depth: usize,

    /// The handler procedure: called as `(handler abort-val captured-cont)`.
    pub handler: TaggedValue,

    /// Register in the prompt-establishing frame where the normal return value
    /// (or the handler's return value) should be written.
    pub dst: Reg,

    /// `VmState::dynamic_winds.len()` at the time the prompt was pushed.
    /// Used to run the correct subset of wind hooks during unwinding.
    pub dynamic_wind_depth: usize,
}

/// Records a `dynamic-wind` in progress.
///
/// The VM maintains a stack of these. When a continuation crossing a wind
/// boundary is invoked, the VM runs the appropriate `before`/`after` thunks.
#[derive(Debug, Clone)]
pub struct DynamicWindRecord {
    /// Thunk to call when entering this dynamic extent.
    pub before: TaggedValue,
    /// Thunk to call when leaving this dynamic extent.
    pub after: TaggedValue,
    /// Frame stack depth when this wind was installed.
    /// Used by `pop_resolved_winds` to detect when the body has returned
    /// (e.g. after a continuation invocation resumes and completes).
    pub stack_depth: usize,
}

/// A captured delimited continuation (created by `AbortToPrompt` or
/// `CaptureComposable`).
///
/// Contains a snapshot of the call-stack slice between the prompt and the
/// capture site, plus the dynamic-wind records in that slice.
///
/// `CallFrame: Clone` is a hard requirement for this to work —
/// see VM_DECISIONS.md §4 and VM_ISA.md §2.2.
#[derive(Debug, Clone)]
pub struct VmDelimitedContinuation {
    /// Cloned call-stack slice (prompt boundary → capture site, exclusive of
    /// the prompt frame itself).
    pub frames: Vec<CallFrame>,

    /// Dynamic-wind records active within the captured slice.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Register snapshot for all frames in this continuation.
    ///
    /// Contains `registers[frames[0].register_base .. end]` at capture time.
    /// On invocation, these values are relocated to a new base in the live
    /// register array so each frame's `register_base` must be adjusted.
    pub registers: Vec<patina_core::tagged_value::TaggedValue>,

    /// The `register_base` of `frames[0]` at capture time.
    /// Used to compute the relocation offset when invoking the continuation.
    pub base_at_capture: usize,
}

/// An installed exception handler (from `with-exception-handler`).
///
/// Handlers form a stack on `VmState`. When `raise` is called, the
/// topmost handler is popped and invoked with the exception object.
/// When the thunk returns normally, handlers installed at depths >= the
/// returning frame's depth are popped.
#[derive(Debug, Clone)]
pub struct ExceptionHandler {
    /// The handler procedure: called as `(handler condition)`.
    pub handler: TaggedValue,
    /// Dynamic-wind records at the point the handler was installed.
    /// Used by `raise` to unwind after-thunks before invoking the handler.
    pub dynamic_winds: Vec<DynamicWindRecord>,
    /// `VmState::frames.len()` at the time the handler was installed.
    /// Used to pop the handler when the thunk returns normally.
    pub stack_depth: usize,
}

/// A full (non-delimited) continuation captured by `call/cc`.
///
/// Invoking it replaces the entire call stack (non-composable).
#[derive(Debug, Clone)]
pub struct VmContinuation {
    /// Full cloned frame stack at capture time.
    pub frames: Vec<CallFrame>,

    /// Dynamic-wind records active at capture time.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Prompt stack at capture time (for restoring barrier state).
    pub prompt_stack: Vec<PromptFrame>,

    /// Exception handler stack at capture time.
    pub exception_handlers: Vec<ExceptionHandler>,

    /// Register snapshot for all live frames (registers[0..end]).
    pub registers: Vec<patina_core::tagged_value::TaggedValue>,

    /// Register in the top frame where the delivered value should be written
    /// when the continuation is invoked. This is the `dst` register from the
    /// `Call call/cc` instruction that captured the continuation.
    pub deliver_reg: Reg,
}
