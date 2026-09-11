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
// The outermost one — a top-level form, or a `Backend::apply` from outside
// any run — is id 0; every nested one, which is how a Rust primitive's
// callback and the `eval` primitive's expression run, gets a fresh id. A
// captured continuation records the id of the trampoline its chain ends in
// (`CpsContinuation::trampoline`), and a jump compares it with the current
// one: the same trampoline resumes the chain in place; an enclosing one is
// reached by parking the escape and unwinding the Rust stack through the
// primitive; a nested one that has already returned has nothing to return
// to, and says so.
//
// Thread-local, like the pending escape: both are properties of the running
// trampolines, not of any value the steps thread.
thread_local! {
    static ACTIVE_TRAMPOLINES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static NEXT_NESTED_TRAMPOLINE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// The id every outermost trampoline runs as.
pub(super) const OUTERMOST_TRAMPOLINE: u64 = 0;

/// Marks a trampoline active for as long as the guard lives — including when
/// the loop leaves by `?`, which is how an escape unwinds through it.
pub(super) struct TrampolineGuard {
    id: u64,
}

impl TrampolineGuard {
    pub(super) fn enter() -> Self {
        let id = ACTIVE_TRAMPOLINES.with(|active| {
            let mut active = active.borrow_mut();
            let id = if active.is_empty() {
                OUTERMOST_TRAMPOLINE
            } else {
                NEXT_NESTED_TRAMPOLINE.with(|next| {
                    let id = next.get();
                    next.set(id + 1);
                    id
                })
            };
            active.push(id);
            id
        });
        Self { id }
    }

    pub(super) fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for TrampolineGuard {
    fn drop(&mut self) {
        ACTIVE_TRAMPOLINES.with(|active| {
            let popped = active.borrow_mut().pop();
            debug_assert_eq!(popped, Some(self.id), "trampolines leave in LIFO order");
        });
    }
}

/// The trampoline a capture or a jump is happening in.
pub(super) fn current_trampoline() -> u64 {
    ACTIVE_TRAMPOLINES.with(|active| {
        active
            .borrow()
            .last()
            .copied()
            .unwrap_or(OUTERMOST_TRAMPOLINE)
    })
}

/// Whether the trampoline with this id is still running — on the Rust
/// stack below the current one, or the current one itself.
pub(super) fn trampoline_is_active(id: u64) -> bool {
    ACTIVE_TRAMPOLINES.with(|active| active.borrow().contains(&id))
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
