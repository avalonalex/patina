//! Continuation values for the CPS evaluator.
//!
//! These live in `patina-core` rather than in the tree-walker because
//! `CpsContinuation` has to store one: a captured continuation's body may refer
//! to continuation bindings that were in scope where it was captured, and those
//! bindings are `ContValue`s.
//!
//! They used to be projected into a `Vec<(Rc<str>, Rc<CpsContinuation>)>` to
//! keep this crate free of the dependency. That projection could only represent
//! `ContValue::Local`, so every other variant had to be smuggled through it
//! under sentinel names or dropped -- which is what stranded continuation
//! binders and produced "Undefined variable: k_N" on a `guard` nested inside
//! another exception handler. Storing the real type removes the encoding, and
//! with it the capture/restore/reify special-casing that surrounded it.
//!
//! Nothing here depends on the tree-walker: every payload is a `patina-core`
//! type, which is why the original circular-dependency concern did not hold.

use crate::cps_expr::{CpsExpr, PromptTag};
use crate::environment::Environment;
use crate::tagged_value::TaggedValue;
use crate::{CpsContinuation, DynamicWindRecord};
use std::fmt;
use std::rc::Rc;

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
pub struct ContEnv(Rc<ContEnvNode>);

enum ContEnvNode {
    Empty,
    Entry {
        name: Rc<str>,
        value: ContValue,
        rest: Rc<ContEnvNode>,
    },
}

impl Default for ContEnv {
    fn default() -> Self {
        Self::new()
    }
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
pub struct ContEnvIter<'a> {
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
pub struct PromptFrame {
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
pub struct ExceptionHandler {
    /// The handler procedure as TaggedValue (lambda (condition) ...)
    pub handler: TaggedValue,
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
pub enum ContValue {
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
    /// A captured first-class continuation.
    ///
    /// Never constructed by the current evaluator: `reify_continuation`
    /// flattens a `Local` into the `CpsContinuation`'s own fields and stores
    /// effect-carrying wrappers in its `resume` field instead. Retained
    /// because first-class continuation work (Track Q Q2) will construct it;
    /// invoking one decodes the `CpsContinuation` with the same decoder the
    /// escape path uses (`continuation_cont_value` in the tree-walker).
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
    /// When the thunk returns, cache the value and continue. Holds the
    /// promise *object*, not its box: a force nested inside the thunk can
    /// re-point the promise at another box (`Heap::promise_update`), so the
    /// box is looked up again when the thunk returns.
    ForceCache {
        promise: TaggedValue,
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
