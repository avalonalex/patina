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
}

/// A full (non-delimited) continuation captured by `call/cc` or
/// `call-with-composable-continuation`.
///
/// For `call/cc` (non-composable): the entire call stack at capture time.
/// For composable: same as `VmDelimitedContinuation` but composed differently
/// on invocation — modelled uniformly here as a frame snapshot.
#[derive(Debug, Clone)]
pub struct VmContinuation {
    /// Full cloned frame stack at capture time.
    pub frames: Vec<CallFrame>,

    /// Dynamic-wind records active at capture time.
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Whether this continuation is composable.
    pub composable: bool,
}
