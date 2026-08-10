//! Type definitions for CPS evaluation
//!
//! This module contains the core type definitions used throughout the CPS evaluator:
//! - `ContEnv` - Persistent linked-list environment for continuation bindings
//! - `ContValue` - Continuation values (local, captured, special continuations)
//! - `StepResult` - Trampoline step results
//! - `PromptFrame` - Prompt frames for delimited continuations
//! - `ExceptionHandler` - Exception handler stack entries

use patina_core::Environment;
use patina_core::cps_expr::{CpsExpr, PromptTag};
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord};
use std::cell::RefCell;
use std::fmt;
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

// ==================== ContEnv ====================

/// Persistent linked-list environment for continuation bindings.
///
/// Optimized for the CPS evaluator's access pattern:
/// - O(1) snapshot (clone) — just an Rc increment
/// - O(1) insert (prepend) — allocates one small node
/// - O(n) lookup — linear scan, but n is typically ≤ 5
///
/// This replaces `HashMap<Rc<str>, ContValue>` which was responsible for
/// ~48% of CPU time due to clone/drop/malloc/free cycles on every CPS step.
/// For small n, linear scan outperforms HashMap (no hashing, no bucket allocation).
#[derive(Clone)]
pub(super) struct ContEnv(Rc<ContEnvNode>);

enum ContEnvNode {
    Empty,
    Entry {
        name: Rc<str>,
        value: ContValue,
        rest: Rc<ContEnvNode>,
    },
}

impl ContEnv {
    /// Create an empty continuation environment.
    pub fn new() -> Self {
        ContEnv(Rc::new(ContEnvNode::Empty))
    }

    /// Look up a continuation by name. Returns the most recently inserted
    /// binding for the given name (shadowing earlier bindings).
    pub fn get(&self, key: &str) -> Option<&ContValue> {
        let mut current: &ContEnvNode = &self.0;
        loop {
            match current {
                ContEnvNode::Empty => return None,
                ContEnvNode::Entry { name, value, rest } => {
                    if name.as_ref() == key {
                        return Some(value);
                    }
                    current = rest;
                }
            }
        }
    }

    /// Insert a new binding, returning a new ContEnv that shadows
    /// any existing binding with the same name.
    /// The original ContEnv is unchanged (persistent/functional).
    pub fn insert(&self, name: Rc<str>, value: ContValue) -> ContEnv {
        ContEnv(Rc::new(ContEnvNode::Entry {
            name,
            value,
            rest: Rc::clone(&self.0),
        }))
    }

    /// Iterate over all bindings (may include shadowed entries).
    /// Yields entries from most-recently-inserted to oldest.
    pub fn iter(&self) -> ContEnvIter<'_> {
        ContEnvIter { current: &self.0 }
    }

    /// Identity of this chain's head node, for GC dedup. Chains are shared by
    /// `Rc`, so tracing must memoize on this or go exponential — see
    /// `gc_roots::trace_cont_env`.
    pub fn gc_identity(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
}

impl fmt::Debug for ContEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContEnv{{")?;
        let mut first = true;
        for (name, _) in self.iter() {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "{}", name)?;
            first = false;
        }
        write!(f, "}}")
    }
}

/// Iterator over ContEnv bindings.
pub(super) struct ContEnvIter<'a> {
    current: &'a ContEnvNode,
}

impl<'a> Iterator for ContEnvIter<'a> {
    type Item = (&'a Rc<str>, &'a ContValue);

    fn next(&mut self) -> Option<Self::Item> {
        match self.current {
            ContEnvNode::Empty => None,
            ContEnvNode::Entry { name, value, rest } => {
                self.current = rest;
                Some((name, value))
            }
        }
    }
}

// ==================== PromptFrame ====================

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
    /// The handler procedure as TaggedValue (lambda (condition) ...)
    pub handler: TaggedValue,
    /// The dynamic winds active when this handler was installed.
    /// Used by `raise` to unwind dynamic-wind after-thunks before invoking the handler.
    pub dynamic_winds: Vec<DynamicWindRecord>,
}

// ==================== ContValue ====================

/// Continuation values used during CPS evaluation
///
/// Continuations can be either:
/// - A CpsExpr body with captured environment (for LetCont-defined continuations)
/// - A first-class continuation value (for captured continuations)
/// - The halt continuation (program end)
/// - Special continuations for CPS-aware primitives
#[derive(Debug, Clone)]
pub(super) enum ContValue {
    /// A local continuation defined by LetCont
    Local {
        param: Rc<str>,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
        /// The continuation environment at the point where this continuation was defined.
        /// This is needed because the continuation body may reference other continuations
        /// that were in scope when the let-cont was evaluated.
        cont_env: ContEnv,
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
        consumer: TaggedValue,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for force
    /// When the thunk returns, cache the value and continue
    ForceCache {
        promise: Rc<std::cell::RefCell<patina_core::PromiseState>>,
        original_cont: Box<ContValue>,
    },
    // Note: ParameterizeCleanup has been removed.
    // Parameterize is now a macro using dynamic-wind (lib/scheme/base/parameters.scm)
    /// Special continuation for dynamic-wind cleanup
    /// When the body returns, pop the wind record, call after thunk, and continue
    DynamicWindCleanup {
        /// The "after" thunk to call when leaving this dynamic extent (as TaggedValue)
        after: TaggedValue,
        /// The wind record ID to pop (for verification)
        wind_id: u64,
        original_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind setup
    /// After the "before" thunk returns, push wind record and call body
    DynamicWindSetup {
        /// The wind record to push
        wind_record: DynamicWindRecord,
        /// The body thunk to call (as TaggedValue)
        body: TaggedValue,
        /// The cleanup continuation (will pop and call after)
        cleanup_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind after thunk completion
    /// After the "after" thunk returns, continue with the saved body result
    DynamicWindAfterDone {
        /// The result from the body (to pass through, as TaggedValue for efficiency)
        result_value: TaggedValue,
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
        original_exception: Option<TaggedValue>,
        /// For continuable: the continuation to continue with
        original_cont: Box<ContValue>,
        /// The handler that was popped, so it can be re-pushed after continuable raise
        popped_handler: Option<ExceptionHandler>,
    },
}

impl ContValue {
    /// The continuation this one decorates, if it decorates one.
    ///
    /// Several variants wrap another continuation to attach an effect that runs
    /// on the way out -- popping an exception handler, caching a forced
    /// promise, running a `dynamic-wind` thunk, delivering multiple values.
    /// Capture and reification both unwrap to the inner continuation: the
    /// effect cannot be serialized, but the continuation underneath must not be
    /// lost.
    ///
    /// `DynamicWindCleanup` is deliberately absent -- it has bespoke
    /// serialization that preserves the wind identity, so it must not be
    /// silently unwrapped.
    pub(super) fn wrapped_cont(&self) -> Option<&ContValue> {
        match self {
            ContValue::CallWithValuesConsumer { original_cont, .. }
            | ContValue::ForceCache { original_cont, .. }
            | ContValue::DynamicWindAfterDone { original_cont, .. }
            | ContValue::ExceptionHandlerCleanup { original_cont }
            | ContValue::RaiseHandlerReturn { original_cont, .. } => Some(original_cont),
            ContValue::DynamicWindSetup { cleanup_cont, .. } => Some(cleanup_cont),
            ContValue::Local { .. }
            | ContValue::Captured(_)
            | ContValue::Halt
            | ContValue::DynamicWindCleanup { .. } => None,
        }
    }
}

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
