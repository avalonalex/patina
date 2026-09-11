//! Type definitions for CPS evaluation
//!
//! This module contains the core type definitions used throughout the CPS evaluator:
//! - `ContEnv` - Persistent linked-list environment for continuation bindings
//! - `ContValue` - Continuation values (local, captured, special continuations)
//! - `StepResult` - Trampoline step results
//! - `PromptFrame` - Prompt frames for delimited continuations
//! - `ExceptionHandler` - Exception handler stack entries

use patina_core::Environment;
use patina_core::cps_expr::CpsExpr;
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use std::cell::RefCell;
use std::rc::Rc;

// Thread-local storage for continuation escapes.
// When a Value::Continuation is invoked inside apply_from_direct,
// we store it here so the outer eval loop can retrieve it.
thread_local! {
    static PENDING_ESCAPE: RefCell<Option<(TaggedValue, Rc<CpsContinuation>)>> = const { RefCell::new(None) };
}

pub(super) fn set_pending_escape(value: TaggedValue, cont: Rc<CpsContinuation>) {
    PENDING_ESCAPE.with(|cell| *cell.borrow_mut() = Some((value, cont)));
}

pub(super) fn take_pending_escape() -> Option<(TaggedValue, Rc<CpsContinuation>)> {
    PENDING_ESCAPE.with(|cell| cell.borrow_mut().take())
}

/// Root the in-flight escape value, if any. A hidden root: between
/// `set_pending_escape` and `take_pending_escape` the value and its
/// continuation are reachable from nowhere else (design §5.1).
pub(super) fn trace_pending_escape(visitor: &mut patina_core::GcVisitor<'_>) {
    PENDING_ESCAPE.with(|cell| {
        if let Some((value, k)) = cell.borrow().as_ref() {
            visitor.visit(*value);
            visitor.visit_continuation(k);
        }
    });
}

// ==================== Trampoline identity ====================
//
// A trampoline is one run of the step loop (`CpsEvaluator::run_trampoline`).
// A captured continuation records the id of the trampoline its chain ends in
// (`CpsContinuation::trampoline`), and a jump compares it with the current
// one: the same trampoline resumes the chain in place; an enclosing one is
// reached by parking the escape and unwinding the Rust stack through the
// primitive; and one that has already returned has no primitive to return
// to, so the nearest enclosing *form-level* run resumes it and its `Halt`
// ends that run — which is what every implementation does when a form
// re-enters a continuation captured by an earlier form, at the REPL or under
// `load`, and what the outermost loop did here before trampolines had ids.
//
// The kind is what makes that last rule land in the right place: a run that
// is a form (a top-level form, or the `eval` primitive's expression) may end
// early with a resumed chain's value; a run that is a primitive's callback
// may not, because the primitive is waiting for the callback's value, so it
// passes the escape up instead.
//
// Thread-local, like the pending escape: both are properties of the running
// trampolines, not of any value the steps thread.
thread_local! {
    static ACTIVE_TRAMPOLINES: RefCell<Vec<(u64, TrampolineKind)>> = const { RefCell::new(Vec::new()) };
    static NEXT_TRAMPOLINE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static UNHANDLED_IN_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What a run of the loop is *for*, which decides what its `Halt` means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TrampolineKind {
    /// A form: `Halt` is the form's value. A top-level form, or the
    /// expression the `eval` primitive was handed.
    Form,
    /// A primitive's callback: `Halt` is the value the primitive is waiting
    /// for.
    Callback,
}

/// Marks a trampoline active for as long as the guard lives — including when
/// the loop leaves by `?`, which is how an escape unwinds through it.
pub(super) struct TrampolineGuard {
    id: u64,
    kind: TrampolineKind,
    outermost: bool,
}

impl TrampolineGuard {
    pub(super) fn enter(kind: TrampolineKind) -> Self {
        let id = NEXT_TRAMPOLINE.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        let outermost = ACTIVE_TRAMPOLINES.with(|active| {
            let mut active = active.borrow_mut();
            active.push((id, kind));
            active.len() == 1
        });
        Self {
            id,
            kind,
            outermost,
        }
    }

    /// Whether this run is the one that resumes a parked escape to `target`.
    ///
    /// Its own continuations, always. A continuation of a run that is still
    /// on the stack below, never — the escape keeps unwinding to it. And one
    /// of a run that has returned: yes if this run is a form (or the
    /// outermost run, which has nothing above it to defer to), no if it is a
    /// callback, whose primitive is owed a value of its own.
    pub(super) fn resumes(&self, target: u64) -> bool {
        self.id == target
            || (!trampoline_is_active(target)
                && (self.kind == TrampolineKind::Form || self.outermost))
    }
}

impl Drop for TrampolineGuard {
    fn drop(&mut self) {
        ACTIVE_TRAMPOLINES.with(|active| {
            let popped = active.borrow_mut().pop().map(|(id, _)| id);
            debug_assert_eq!(popped, Some(self.id), "trampolines leave in LIFO order");
        });
    }
}

/// The trampoline a capture or a jump is happening in.
pub(super) fn current_trampoline() -> u64 {
    ACTIVE_TRAMPOLINES.with(|active| active.borrow().last().map_or(0, |(id, _)| *id))
}

/// Whether the trampoline with this id is still running — on the Rust
/// stack below the current one, or the current one itself.
pub(super) fn trampoline_is_active(id: u64) -> bool {
    ACTIVE_TRAMPOLINES.with(|active| active.borrow().iter().any(|(active, _)| *active == id))
}

/// Whether the current trampoline is the outermost one — the one whose
/// `Halt` ends the program rather than a form or a callback.
pub(super) fn is_outermost_trampoline() -> bool {
    ACTIVE_TRAMPOLINES.with(|active| active.borrow().len() == 1)
}

/// A callback's run has just returned a catchable error that every handler
/// it inherited declined (or there were none). The step that called the
/// primitive holds the *same* handlers, so it must not offer the error to
/// them again — R7RS 6.11 gives a secondary exception only the handlers
/// *outside* the one that returned, and re-routing ran that handler twice.
/// Set by `CallbackContext` on the way out, taken by the call site as soon
/// as the primitive returns, so it can never outlive the call it describes.
pub(super) fn mark_unhandled_in_callback() {
    UNHANDLED_IN_CALLBACK.with(|flag| flag.set(true));
}

pub(super) fn take_unhandled_in_callback() -> bool {
    UNHANDLED_IN_CALLBACK.with(|flag| flag.replace(false))
}

// ==================== Continuation values ====================
//
// Defined in patina-core because `CpsContinuation` stores a `ContEnv`; see the
// module docs there. Re-exported here so the evaluator keeps referring to them
// unqualified.
pub(super) use patina_core::cont_value::{ContEnv, ContValue, ExceptionHandler, PromptFrame};

// ==================== StepResult ====================

/// Result of a single evaluation step (for trampoline)
///
/// The CPS evaluator processes expressions one step at a time.
/// Each step either produces a final value or indicates the
/// next expression to evaluate.
pub(super) enum StepResult {
    /// Final value - evaluation is complete (as TaggedValue for efficient storage)
    Done(TaggedValue),
    /// Continue evaluation with a new expression and state
    Continue {
        expr: Rc<CpsExpr>,
        env: Rc<Environment>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Invoke a continuation with a value (as TaggedValue for efficiency)
    InvokeContinuation {
        cont: ContValue,
        value: TaggedValue,
        env: Rc<Environment>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Apply a procedure
    ApplyProc {
        proc: TaggedValue,
        args: Vec<TaggedValue>,
        cont: ContValue,
        env: Rc<Environment>,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
}
