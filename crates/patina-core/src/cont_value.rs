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

thread_local! {
    /// The one `Empty` node every empty `ContEnv` shares.
    ///
    /// Applying a procedure starts its body with a fresh continuation
    /// environment, so `new` runs once per call; allocating a node that
    /// carries nothing, only to drop it a few steps later, was a malloc and a
    /// free per call. The node is immutable and its identity is used only for
    /// GC dedup, so one shared node is indistinguishable from many.
    static EMPTY_CONT_ENV: Rc<ContEnvNode> = Rc::new(ContEnvNode::Empty);
}

impl ContEnv {
    /// Create an empty continuation environment.
    pub fn new() -> Self {
        ContEnv(EMPTY_CONT_ENV.with(Rc::clone))
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

/// A prompt on the tree-walker's prompt stack.
///
/// A prompt is a *boundary in the continuation*: the prompt body runs with
/// [`ContValue::PromptBoundary`] as its continuation rather than the
/// caller's, and the caller's lives here. Reaching the boundary pops this
/// frame and delivers to `cont`. That is what makes a delimited continuation
/// simple to capture — it is the aborting call's own continuation chain,
/// which ends at the boundary — and simple to compose: invoking one pushes a
/// frame of its own with the invoke site's continuation in `cont`, so the
/// chain returns to the invoker when it reaches its boundary.
///
/// The VM keeps the same three-stack position (`docs/VM_RUNTIME.md` §2.3);
/// there is no frame depth here because a CPS continuation is not a stack.
#[derive(Debug, Clone)]
pub struct PromptFrame {
    /// Which boundary this frame answers to. A `call-with-continuation-prompt`
    /// mints one; a composable invoke re-uses the id its continuation's
    /// chain ends at, so the same chain can be resumed any number of times,
    /// nested or not, and each resumption's boundary finds its own frame.
    pub id: u64,
    /// The tag an `abort-current-continuation` searches for.
    ///
    /// `None` for the frame a composable invoke pushes: a delimited
    /// continuation does not include its prompt (Racket, Guile and the VM
    /// agree), so an abort inside the resumed computation must find the
    /// enclosing prompt with that tag at the invoke site, or none.
    pub tag: Option<Rc<PromptTag>>,
    /// The prompt's handler, called as `(handler value k)` on an abort.
    /// `#f` when the call gave none, and for an invoke's frame.
    pub handler: TaggedValue,
    /// Where the body's value goes on normal return, and where the handler's
    /// value goes after an abort.
    pub cont: ContValue,
    /// `dynamic_winds.len()` when the prompt was pushed: an abort travels to
    /// this depth, and a delimited capture carries the records above it.
    pub wind_depth: usize,
    /// `exception_handlers.len()` when the prompt was pushed — the same
    /// boundary for the handler stack. Recorded, not inferred: the VM learned
    /// that no frame-depth comparison separates a handler installed in tail
    /// position of the prompt body from one whose thunk tail-called the
    /// prompt (`docs/VM_RUNTIME.md` §5.6).
    pub handler_depth: usize,
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
    /// A continuation jump in progress: `value` is on its way to `target`,
    /// and a wind thunk between here and there is running. When the thunk
    /// returns, the next one runs; when none is left, the escape is parked
    /// and the Rust stack unwinds to the outermost trampoline.
    ///
    /// The thunks run as ordinary steps, with this as their continuation, so
    /// a raise inside one finds its handlers on the same trampoline and a
    /// continuation captured inside one can be re-entered — a `guard` that
    /// declines re-enters the thunk through its `handler-k`, and the thunk
    /// then completes and the jump lands. They used to run on a nested
    /// trampoline with an empty environment, where a raise was fatal and a
    /// re-entry after the nested frame had unwound reached its `Halt` and
    /// ended the program with the thunk's value.
    Jump {
        /// The record whose `before` thunk just ran and whose extent is now
        /// entered — pushed when this continuation is reached, as
        /// `DynamicWindSetup` pushes after a `dynamic-wind` call's before
        /// thunk. `None` after an `after` thunk: its record was popped before
        /// it ran, so a second jump from inside it cannot run it again.
        entered: Option<DynamicWindRecord>,
        /// The value to deliver to `target`.
        value: TaggedValue,
        /// The continuation being jumped to.
        target: Rc<CpsContinuation>,
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
    /// The continuation of a prompt body: the boundary a delimited
    /// continuation is delimited by.
    ///
    /// Reaching it pops the [`PromptFrame`] with this `id` and delivers the
    /// value to that frame's `cont`. The frame is looked up by id rather than
    /// taken from the top so that a mismatch is an internal error and not a
    /// silent misdelivery; in a consistent machine it *is* the top, because
    /// every transfer that changes the continuation also sets the prompt
    /// stack to match — a full invoke restores its snapshot, an abort cuts
    /// back to below its prompt, a composable invoke pushes.
    PromptBoundary { id: u64 },
    /// Where an abort lands: call the prompt's handler with the abort's value
    /// and the delimited continuation, then deliver its result to the
    /// prompt's own continuation.
    ///
    /// The abort *travels* here through `jump_to_continuation`, which is what
    /// runs the after-thunks of every extent between the abort and its
    /// prompt, each under its own record's handler stack (the VM's #165).
    /// The arrival restores the dynamic environment below the prompt, so the
    /// handler runs there — which is A1 in `docs/VM_RUNTIME.md` §5.6.
    AbortLanding {
        /// The prompt's handler.
        handler: TaggedValue,
        /// The delimited continuation the handler receives as its second
        /// argument.
        delimited: TaggedValue,
        /// The prompt's continuation, which the handler's value goes to.
        cont: Box<ContValue>,
    },
    /// A composable invoke in progress: the `before` thunk of the captured
    /// extent at `index` has just returned, so its record is entered now and
    /// the next one's thunk runs, until the chain itself can be resumed with
    /// `value`.
    ///
    /// The thunks run as ordinary steps with this as their continuation, so
    /// a continuation captured inside one and re-entered later finishes the
    /// thunk and then the invoke — the analogue of the VM's
    /// `ResumeComposableInvoke` stub, and of `Jump` for a full continuation's
    /// travel. Not `Jump` itself: a jump's thunks run under their record's
    /// own handler stack because the target replaces the machine, while an
    /// invoke's target *extends* it, so these run under the invoke site's
    /// stack. Both backends pin that in `cps_features.rs`.
    ComposableInvokeStep {
        /// The delimited continuation being invoked.
        target: Rc<CpsContinuation>,
        /// The value to deliver into the captured chain once every extent
        /// has been re-entered.
        value: TaggedValue,
        /// Which of `target.dynamic_winds` this step has just entered.
        index: usize,
        /// The invoke site's continuation: where the resumed chain returns
        /// when it reaches its boundary.
        cont: Box<ContValue>,
    },
}
