//! Type definitions for CPS evaluation
//!
//! This module contains the core type definitions used throughout the CPS evaluator:
//! - `ContValue` - Continuation values (local, captured, special continuations)
//! - `StepResult` - Trampoline step results
//! - `PromptFrame` - Prompt frames for delimited continuations
//! - `ExceptionHandler` - Exception handler stack entries

use patina_core::Environment;
use patina_core::cps_expr::{CpsExpr, PromptTag};
use patina_core::value::{CpsContinuation, DynamicWindRecord, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// Thread-local storage for continuation escapes.
// When a Value::Continuation is invoked inside apply_from_direct,
// we store it here so the outer eval loop can retrieve it.
thread_local! {
    static PENDING_ESCAPE: RefCell<Option<(Value, Rc<CpsContinuation>)>> = const { RefCell::new(None) };
}

pub(super) fn set_pending_escape(value: Value, cont: Rc<CpsContinuation>) {
    PENDING_ESCAPE.with(|cell| *cell.borrow_mut() = Some((value, cont)));
}

pub(super) fn take_pending_escape() -> Option<(Value, Rc<CpsContinuation>)> {
    PENDING_ESCAPE.with(|cell| cell.borrow_mut().take())
}

/// Helper: construct a Scheme list from a Vec<Value>
pub(super) fn list_from_vec(vec: Vec<Value>) -> Value {
    let mut result = Value::Null;
    for item in vec.into_iter().rev() {
        result = Value::Pair(Rc::new(RefCell::new((item, result))));
    }
    result
}

/// A prompt frame on the meta-continuation stack
#[derive(Debug, Clone)]
pub(super) struct PromptFrame {
    /// The tag identifying this prompt
    pub tag: Rc<PromptTag>,
    /// Continuation to invoke when prompt is reached
    pub cont: ContValue,
    /// Dynamic wind records active at this prompt
    pub dynamic_winds: Vec<DynamicWindRecord>,
}

/// An exception handler installed by with-exception-handler
///
/// Exception handlers form a stack (like dynamic-wind records).
/// When raise is called, the topmost handler is invoked.
#[derive(Debug, Clone)]
pub(super) struct ExceptionHandler {
    /// The handler procedure (lambda (condition) ...)
    pub handler: Value,
}

/// Continuation values used during CPS evaluation
///
/// Continuations can be either:
/// - A CpsExpr body with captured environment (for LetCont-defined continuations)
/// - A first-class continuation value (for captured continuations)
/// - The halt continuation (program end)
/// - Special continuations for CPS-aware primitives
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(super) enum ContValue {
    /// A local continuation defined by LetCont
    Local {
        param: Rc<str>,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
        /// The continuation environment at the point where this continuation was defined
        /// This is needed because the continuation body may reference other continuations
        /// that were in scope when the let-cont was evaluated.
        cont_env: HashMap<Rc<str>, ContValue>,
    },
    /// A captured first-class continuation (used when re-invoking serialized continuations)
    /// Currently only matched, not constructed - will be used when continuation serialization is implemented
    #[allow(dead_code)]
    Captured(Rc<CpsContinuation>),
    /// The halt continuation - returns final value
    Halt,
    /// Special continuation for call-with-values
    /// When the producer returns, unpack its values and call the consumer
    CallWithValuesConsumer {
        consumer: Value,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for force
    /// When the thunk returns, cache the value and continue
    ForceCache {
        promise: Rc<std::cell::RefCell<patina_core::value::PromiseState>>,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for parameterize cleanup
    /// When the body returns, pop parameter values and continue
    ParameterizeCleanup {
        params: Vec<Value>,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind cleanup
    /// When the body returns, pop the wind record, call after thunk, and continue
    DynamicWindCleanup {
        /// The "after" thunk to call when leaving this dynamic extent
        after: Value,
        /// The wind record ID to pop (for verification)
        wind_id: u64,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind setup
    /// After the "before" thunk returns, push wind record and call body
    DynamicWindSetup {
        /// The wind record to push
        wind_record: DynamicWindRecord,
        /// The body thunk to call
        body: Value,
        /// The cleanup continuation (will pop and call after)
        cleanup_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind after thunk completion
    /// After the "after" thunk returns, continue with the saved body result
    DynamicWindAfterDone {
        /// The result from the body (to pass through)
        result_value: Value,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for with-exception-handler cleanup
    /// When the thunk completes normally, pop the exception handler and continue
    ExceptionHandlerCleanup { original_cont: Box<ContValue> },
    /// Special continuation for raise when handler returns
    /// For non-continuable raise: if handler returns, raise secondary exception
    /// For continuable raise: handler's return value continues from raise-continuable
    RaiseHandlerReturn {
        /// Whether this was a continuable raise
        continuable: bool,
        /// For non-continuable: the original exception (to include in secondary error)
        original_exception: Option<Value>,
        /// For continuable: the continuation to continue with
        original_cont: Box<ContValue>,
    },
}

/// Result of a single evaluation step (for trampoline)
///
/// The CPS evaluator processes expressions one step at a time.
/// Each step either produces a final value or indicates the
/// next expression to evaluate.
pub(super) enum StepResult {
    /// Final value - evaluation is complete
    Done(Value),
    /// Continue evaluation with a new expression and state
    Continue {
        expr: CpsExpr,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Invoke a continuation with a value
    InvokeContinuation {
        cont: ContValue,
        value: Value,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
    /// Apply a procedure
    ApplyProc {
        proc: Value,
        args: Vec<Value>,
        cont: ContValue,
        env: Rc<Environment>,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    },
}
