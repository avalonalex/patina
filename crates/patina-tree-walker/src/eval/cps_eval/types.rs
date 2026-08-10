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
        expr: CpsExpr,
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
