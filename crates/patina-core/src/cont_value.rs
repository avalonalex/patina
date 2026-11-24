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
use crate::environment::{CAPTURED_FRAME_BYTES, Environment, RC_COUNTS};
use crate::tagged_value::TaggedValue;
use crate::{CpsContinuation, DynamicWindRecord, Trace};
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use std::fmt;
use std::mem::size_of;
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

impl Drop for ContEnvNode {
    /// #617's rule, for the chain a program's recursion builds (#725). A
    /// continuation's environment holds the continuation it returns to,
    /// whose own environment holds the next, so the chain is as deep as the
    /// recursion. Dropped at once, it recursed a level at a time, and 300,000
    /// levels overflowed the main thread's stack: abandoned by an escape or
    /// an uncaught error, or freed with a continuation object a collection
    /// found dead. With no room left, the children go to `drop_deep`, which
    /// finishes on a new segment.
    fn drop(&mut self) {
        let ContEnvNode::Entry { value, rest, .. } = self else {
            return;
        };
        if crate::walk::has_stack_room() {
            return;
        }
        let value = std::mem::replace(value, ContValue::Halt);
        let rest = std::mem::replace(rest, Rc::new(ContEnvNode::Empty));
        crate::walk::drop_deep((value, rest));
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
    /// `GcVisitor`'s `SlotVisitor::cont_env`.
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

// ==================== The levels a capture holds (#656) ====================

/// What a tree-walker capture is charged for each continuation level it is
/// the first to capture (#656; `heap/account.rs`,
/// `cps_continuation_payload`): the level a non-tail call leaves pending, as
/// `(+ 1 (f x))` leaves one while `f` runs. That is the call's frame
/// ([`CAPTURED_FRAME_BYTES`], a frame binding one parameter), the frame of
/// temporaries its pending continuation resumes in, which binds by name in
/// its inline slots, and two entries of continuation environment: the
/// continuation's own, and the one that binds it as the callee's return.
/// 980 bytes on a 64-bit target, which
/// `captured_level_bytes_is_a_non_tail_calls_level` checks. With collection
/// off, each level of #656's program adds 963 bytes of footprint
/// (2026-11-24, at 8e83f889).
///
/// A heuristic, as `CAPTURED_FRAME_BYTES` is: a level may hold more
/// temporaries or bind more; and a frame a closure captured is charged by
/// the closure as well. Across eleven shapes of recursion the account saw
/// between 1.03 and 1.71 times what the footprint grew by (#656's PR has
/// the table).
pub(crate) const CAPTURED_LEVEL_BYTES: usize = CAPTURED_FRAME_BYTES
    + RC_COUNTS
    + size_of::<Environment>()
    + 2 * (RC_COUNTS + size_of::<ContEnvNode>());

impl ContValue {
    /// The local continuation this value delivers to in the end, as the
    /// frame it resumes in and the continuation environment it resumes
    /// under: itself, or the one inside its wrappers. `None` where the chain
    /// ends, or goes on through something a capture reaches another way: a
    /// prompt's boundary, whose frame the capture's prompt stack holds, and a
    /// captured continuation, which was charged when it was made.
    ///
    /// Every variant is named, so a new one does not compile here until it
    /// says where its chain goes on.
    fn level(&self) -> Option<(&Rc<Environment>, &ContEnv)> {
        let mut value = self;
        loop {
            value = match value {
                ContValue::Local {
                    param: _,
                    body: _,
                    env,
                    cont_env,
                } => return Some((env, cont_env)),
                ContValue::CallWithValuesConsumer {
                    consumer: _,
                    original_cont,
                }
                | ContValue::ForceCache {
                    promise: _,
                    original_cont,
                }
                | ContValue::ResumePrimitive {
                    index: _,
                    state: _,
                    original_cont,
                }
                | ContValue::DynamicWindCleanup {
                    after: _,
                    wind_id: _,
                    original_cont,
                }
                | ContValue::DynamicWindAfterDone {
                    result_value: _,
                    original_cont,
                }
                | ContValue::ExceptionHandlerCleanup { original_cont }
                | ContValue::RaiseHandlerReturn {
                    continuable: _,
                    original_exception: _,
                    original_cont,
                    popped_handler: _,
                } => original_cont,
                ContValue::DynamicWindSetup {
                    wind_record: _,
                    body: _,
                    cleanup_cont,
                } => cleanup_cont,
                ContValue::AbortLanding {
                    handler: _,
                    delimited: _,
                    cont,
                }
                | ContValue::ComposableInvokeStep {
                    target: _,
                    value: _,
                    index: _,
                    cont,
                } => cont,
                ContValue::Captured(_)
                | ContValue::Halt
                | ContValue::Jump {
                    entered: _,
                    value: _,
                    target: _,
                }
                | ContValue::PromptBoundary { id: _ }
                | ContValue::ExitLanding { status: _ } => return None,
            };
        }
    }
}

/// The walk [`CpsContinuation::mark_levels`] makes: the levels counted so
/// far, the continuation environments still to walk, and the ones under a
/// namespace's continuation that this walk has entered.
///
/// The stack is inline for the usual capture, which queues one or two; the
/// set allocates only when the walk meets a namespace's continuation.
#[derive(Default)]
struct LevelWalk<'a> {
    levels: usize,
    pending: SmallVec<[&'a Rc<ContEnvNode>; 8]>,
    entered: FxHashSet<*const ContEnvNode>,
}

impl<'a> LevelWalk<'a> {
    /// Counts the level `value` delivers to, if no capture was charged for
    /// it, and queues what that level returns into. `rest` is what follows
    /// `value` in the continuation environment it was read from, which the
    /// walk goes on to anyway: a `let-cont`'s entry is followed by the
    /// environment its continuation was defined under.
    fn value(&mut self, value: &'a ContValue, rest: Option<&Rc<ContEnvNode>>) {
        let Some((env, cont_env)) = value.level() else {
            return;
        };
        let below = &cont_env.0;
        let follows = rest.is_some_and(|rest| Rc::ptr_eq(rest, below));
        if env.is_namespace() {
            // A top-level form's continuation, a library body's or an
            // `eval`'s: no frame to mark, so each walk goes through it, once.
            if follows || !self.entered.insert(Rc::as_ptr(below)) {
                return;
            }
        } else if env.mark_captured() {
            self.levels += 1;
            if follows {
                return;
            }
        } else {
            return;
        }
        self.pending.push(below);
    }
}

impl CpsContinuation {
    /// Counts the continuation levels this capture holds that no earlier
    /// capture was charged for, and marks each (#656): what
    /// `Heap::alloc_continuation` charges it for, at
    /// `CAPTURED_LEVEL_BYTES` a level (`heap/account.rs` says what the
    /// estimate misses).
    ///
    /// A level is a frame some pending continuation resumes in. The walk
    /// starts at the capture's own and follows each level to the one it
    /// returns into, through the continuation environments, and stops at a
    /// frame an earlier capture marked: that capture walked whatever lies
    /// below it, since a frame belongs to one call and a call returns into
    /// one continuation. So a tail that many captures share is walked and
    /// charged once, and a capture costs the levels made since the last.
    ///
    /// A loop over a stack of its own, since a chain is as deep as the
    /// program's recursion (#725).
    pub(crate) fn mark_levels(&self) -> usize {
        let mut walk = LevelWalk::default();
        // The capture is a local continuation taken apart: its frame, and the
        // environment its body resumes under, which is walked whatever the
        // frame says.
        if !self.env.is_namespace() && self.env.mark_captured() {
            walk.levels += 1;
        }
        walk.pending.push(&self.captured_cont_env.0);
        // A wrapper the capture resumes into, and where each of its prompts'
        // bodies returns: a chain ends at its prompt's boundary, and goes on
        // from the prompt's frame.
        if let Some(resume) = &self.resume {
            walk.value(resume, None);
        }
        for prompt in &self.prompt_stack {
            walk.value(&prompt.cont, None);
        }
        while let Some(mut node) = walk.pending.pop() {
            while let ContEnvNode::Entry {
                name: _,
                value,
                rest,
            } = &**node
            {
                walk.value(value, Some(rest));
                node = rest;
            }
        }
        walk.levels
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
#[derive(Debug, Clone, Trace)]
pub struct PromptFrame {
    /// Which boundary this frame answers to. A `call-with-continuation-prompt`
    /// mints one; a composable invoke re-uses the id its continuation's
    /// chain ends at, so the same chain can be resumed any number of times,
    /// nested or not, and each resumption's boundary finds its own frame.
    #[trace(skip, reason = "a prompt id")]
    pub id: u64,
    /// The tag an `abort-current-continuation` searches for.
    ///
    /// `None` for the frame a composable invoke pushes: a delimited
    /// continuation does not include its prompt (Racket, Guile and the VM
    /// agree), so an abort inside the resumed computation must find the
    /// enclosing prompt with that tag at the invoke site, or none.
    #[trace(skip, reason = "a plain Rust struct shared by `Rc`, a name and an id")]
    pub tag: Option<Rc<PromptTag>>,
    /// The prompt's handler, called as `(handler value k)` on an abort.
    /// `#f` when the call gave none, and for an invoke's frame.
    pub handler: TaggedValue,
    /// Where the body's value goes on normal return, and where the handler's
    /// value goes after an abort.
    pub cont: ContValue,
    /// `dynamic_winds.len()` when the prompt was pushed: an abort travels to
    /// this depth, and a delimited capture carries the records above it.
    #[trace(skip, reason = "a depth")]
    pub wind_depth: usize,
    /// The trampoline the prompt was pushed in — see
    /// [`CpsContinuation::trampoline`](crate::CpsContinuation::trampoline).
    /// An abort's landing is a jump to *this* trampoline, whatever trampoline
    /// the abort itself runs on: an abort from inside a primitive's callback
    /// to a prompt outside it must unwind through the primitive.
    #[trace(skip, reason = "a trampoline id")]
    pub trampoline: u64,
    /// `exception_handlers.len()` when the prompt was pushed — the same
    /// boundary for the handler stack. Recorded, not inferred: the VM learned
    /// that no frame-depth comparison separates a handler installed in tail
    /// position of the prompt body from one whose thunk tail-called the
    /// prompt (`docs/VM_RUNTIME.md` §5.6).
    #[trace(skip, reason = "a depth")]
    pub handler_depth: usize,
}

/// An exception handler installed by with-exception-handler
///
/// Exception handlers form a stack (like dynamic-wind records).
/// When raise is called, the topmost handler is invoked.
#[derive(Debug, Clone, Trace)]
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
///
/// Its trace reports what a variant holds and carries on with the
/// continuation the variant wraps, its `tail`, in a loop: most variants
/// differ only in what they report before handing off, and a chain can be as
/// deep as the program's recursion. Local continuation environments and
/// captured continuations go on the visitor's worklists. Pinned by the
/// sentinel test `cont_value_variants`.
#[derive(Debug, Clone, Trace)]
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
        #[trace(tail)]
        original_cont: Box<ContValue>,
    },
    /// Special continuation for force
    /// When the thunk returns, cache the value and continue. Holds the
    /// promise *object*, not its box: a force nested inside the thunk can
    /// re-point the promise at another box (`Heap::promise_update`), so the
    /// box is looked up again when the thunk returns.
    ForceCache {
        promise: TaggedValue,
        #[trace(tail)]
        original_cont: Box<ContValue>,
    },
    /// A resumable primitive's call has returned: resume the primitive at
    /// registry `index` with the `state` it kept and the value, then carry on
    /// with `original_cont` — the tree-walker's form of the VM's
    /// `resume_stub` (`patina_primitives::Step`, #477, #478). A continuation
    /// captured in the call carries this, so re-entered after the primitive
    /// returned it resumes the primitive again.
    ResumePrimitive {
        #[trace(skip, reason = "an index into the primitive registry")]
        index: usize,
        state: TaggedValue,
        #[trace(tail)]
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
        #[trace(skip, reason = "a number minted per `dynamic-wind` call")]
        wind_id: u64,
        #[trace(tail)]
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
        #[trace(tail)]
        cleanup_cont: Box<ContValue>,
    },
    /// Special continuation for dynamic-wind after thunk completion
    /// After the "after" thunk returns, continue with the saved body result
    DynamicWindAfterDone {
        /// The result from the body (to pass through, as TaggedValue for efficiency)
        result_value: TaggedValue,
        #[trace(tail)]
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
    ExceptionHandlerCleanup {
        #[trace(tail)]
        original_cont: Box<ContValue>,
    },
    /// Special continuation for raise when handler returns
    /// For non-continuable raise: if handler returns, raise secondary exception
    /// For continuable raise: handler's return value continues from raise-continuable
    RaiseHandlerReturn {
        /// Whether this was a continuable raise
        #[trace(skip, reason = "a flag")]
        continuable: bool,
        /// For non-continuable: the original exception (to include in secondary error)
        original_exception: Option<TaggedValue>,
        /// For continuable: the continuation to continue with
        #[trace(tail)]
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
    PromptBoundary {
        #[trace(skip, reason = "a prompt id")]
        id: u64,
    },
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
        #[trace(tail)]
        cont: Box<ContValue>,
    },
    /// Where an `exit` lands: every extent has been left, so the process ends
    /// with `status`.
    ///
    /// The exit *travels* here through `jump_to_continuation` like any jump,
    /// which is what runs the after thunk of every outstanding `dynamic-wind`
    /// first (R7RS 6.14, #336); the arrival ends the process rather than
    /// resuming anything.
    ExitLanding {
        /// The status the process ends with.
        #[trace(skip, reason = "an exit status")]
        status: i32,
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
    /// stack. Both backends pin that in `tests/scheme/control/prompts.scm`.
    ComposableInvokeStep {
        /// The delimited continuation being invoked.
        target: Rc<CpsContinuation>,
        /// The value to deliver into the captured chain once every extent
        /// has been re-entered.
        value: TaggedValue,
        /// Which of `target.dynamic_winds` this step has just entered.
        #[trace(
            skip,
            reason = "a position in `target.dynamic_winds`, which `target`'s own trace covers"
        )]
        index: usize,
        /// The invoke site's continuation: where the resumed chain returns
        /// when it reaches its boundary.
        #[trace(tail)]
        cont: Box<ContValue>,
    },
}

/// What a tree-walker capture is charged for (#656): the levels
/// [`CpsContinuation::mark_levels`] counts, on chains built as the evaluator
/// builds them, and the account's charge and credit of them.
#[cfg(test)]
mod level_tests {
    use super::*;
    use crate::cps_expr::CpsExprKind;
    use crate::heap::trace_sentinels::collect_from;
    use crate::heap::{Heap, OBJECT_SLOT_BYTES, cps_continuation_payload, new_shared_heap};

    fn namespace() -> Rc<Environment> {
        Rc::new(Environment::with_heap(&new_shared_heap()))
    }

    /// A frame of temporaries under `global`, as a pending continuation
    /// resumes in.
    fn frame(global: &Rc<Environment>) -> Rc<Environment> {
        Rc::new(Environment::for_temporaries(Rc::clone(global)))
    }

    /// A local continuation that resumes in `env` under `cont_env`.
    fn local(env: &Rc<Environment>, cont_env: &ContEnv) -> ContValue {
        ContValue::Local {
            param: Rc::from("v"),
            body: Rc::new(CpsExpr::new(CpsExprKind::ContRef("k".into()))),
            env: Rc::clone(env),
            cont_env: cont_env.clone(),
        }
    }

    /// The continuation environment a call's body starts under: its return,
    /// and nothing else.
    fn returning_to(cont: ContValue) -> ContEnv {
        ContEnv::new().insert(Rc::from("k"), cont)
    }

    /// `depth` non-tail calls above `below`: each leaves a continuation
    /// pending in a frame of its own, bound as the next call's return.
    fn recursion(global: &Rc<Environment>, below: &ContEnv, depth: usize) -> ContEnv {
        let mut cont_env = below.clone();
        for _ in 0..depth {
            cont_env = returning_to(local(&frame(global), &cont_env));
        }
        cont_env
    }

    /// A capture made in `env` under `cont_env`.
    fn capture(env: &Rc<Environment>, cont_env: &ContEnv) -> CpsContinuation {
        CpsContinuation {
            body: Rc::new(CpsExpr::new(CpsExprKind::ContRef("k".into()))),
            param: Rc::from("v"),
            env: Rc::clone(env),
            boundary: None,
            trampoline: 0,
            crosses_callback: false,
            dynamic_winds: vec![],
            prompt_stack: vec![],
            exception_handlers: vec![],
            captured_cont_env: cont_env.clone(),
            resume: None,
        }
    }

    /// A capture counts its own frame and each level below it, and a second
    /// capture of the same chain counts nothing.
    #[test]
    fn a_capture_counts_each_level_it_holds_once() {
        let global = namespace();
        let chain = recursion(&global, &ContEnv::new(), 5);
        let own = frame(&global);

        assert_eq!(capture(&own, &chain).mark_levels(), 6);
        assert_eq!(capture(&own, &chain).mark_levels(), 0);
    }

    /// Captures that share a tail: the first is charged for it, and each
    /// later one for the levels made since, wherever its chain joins.
    #[test]
    fn a_shared_tail_is_counted_by_the_first_capture_alone() {
        let global = namespace();
        let tail = recursion(&global, &ContEnv::new(), 1000);
        assert_eq!(capture(&frame(&global), &tail).mark_levels(), 1001);

        let deeper = recursion(&global, &tail, 2);
        assert_eq!(capture(&frame(&global), &deeper).mark_levels(), 3);
        let beside = recursion(&global, &tail, 4);
        assert_eq!(capture(&frame(&global), &beside).mark_levels(), 5);
        // The levels are marked, not the captures: one made in a marked
        // frame, over a marked chain, costs nothing more.
        let own = frame(&global);
        assert_eq!(capture(&own, &deeper).mark_levels(), 1);
        assert_eq!(capture(&own, &deeper).mark_levels(), 0);
    }

    /// A marked frame ends the walk: nothing below it is visited. Here the
    /// continuation that resumes in a marked frame is given a chain no
    /// capture has walked, which the evaluator never builds, so that a walk
    /// that went on would count it.
    #[test]
    fn a_marked_frame_ends_the_walk() {
        let global = namespace();
        let marked = frame(&global);
        assert_eq!(capture(&marked, &ContEnv::new()).mark_levels(), 1);

        let unwalked = recursion(&global, &ContEnv::new(), 5);
        let chain = returning_to(local(&marked, &unwalked));
        assert_eq!(capture(&frame(&global), &chain).mark_levels(), 1);
    }

    /// A capture made where no frame is, at a top level: its chain is
    /// counted, and nothing for the namespace.
    #[test]
    fn a_capture_in_a_namespace_counts_its_chain_alone() {
        let global = namespace();
        let chain = recursion(&global, &ContEnv::new(), 3);
        assert_eq!(capture(&global, &chain).mark_levels(), 3);
    }

    /// A namespace's continuation in the chain, as a top-level form's or an
    /// `eval`'s: it has no frame to mark, so every capture walks through it
    /// to the levels below, which are still counted once.
    #[test]
    fn the_walk_goes_through_a_namespaces_continuation() {
        let global = namespace();
        let below = recursion(&global, &ContEnv::new(), 4);
        let top_level = returning_to(local(&global, &below));
        let above = recursion(&global, &top_level, 2);

        assert_eq!(capture(&frame(&global), &above).mark_levels(), 7);
        let later = recursion(&global, &top_level, 1);
        assert_eq!(capture(&frame(&global), &later).mark_levels(), 2);
    }

    /// A namespace's continuation is entered once in a walk, however many
    /// entries hold it. Forty environments of two entries each, both holding
    /// the namespace's continuation over the environment below: a walk that
    /// entered it from each would take 2⁴⁰ steps.
    #[test]
    fn a_namespaces_continuation_is_entered_once_in_a_walk() {
        let global = namespace();
        let mut cont_env = recursion(&global, &ContEnv::new(), 1);
        for _ in 0..40 {
            let held = local(&global, &cont_env);
            cont_env = returning_to(held.clone()).insert(Rc::from("j"), held);
        }
        assert_eq!(capture(&frame(&global), &cont_env).mark_levels(), 2);
    }

    /// A `let-cont`'s entry is followed by the environment its continuation
    /// was defined under, so that environment is reached twice from every
    /// entry. Sixty-four of them nested, in a namespace and in one frame:
    /// a walk that went both ways would take 2⁶⁴ steps.
    #[test]
    fn a_let_conts_definition_environment_is_walked_once() {
        let global = namespace();
        let shared = frame(&global);
        for env in [&global, &shared] {
            let mut cont_env = recursion(&global, &ContEnv::new(), 1);
            for i in 0..64 {
                let name: Rc<str> = Rc::from(format!("k{i}"));
                cont_env = cont_env.insert(name, local(env, &cont_env));
            }
            let levels = capture(&frame(&global), &cont_env).mark_levels();
            // Its own frame and the level below, and the shared frame once.
            let expected = if env.is_namespace() { 2 } else { 3 };
            assert_eq!(levels, expected);
        }
    }

    /// A wrapper is not a level, and the continuation inside it is: each
    /// wrapper that carries one on, over a chain of two.
    #[test]
    fn a_wrapped_continuation_is_counted_through_its_wrappers() {
        let global = namespace();
        let wrappers: [fn(Box<ContValue>) -> ContValue; 9] = [
            |original_cont| ContValue::CallWithValuesConsumer {
                consumer: TaggedValue::NULL,
                original_cont,
            },
            |original_cont| ContValue::ForceCache {
                promise: TaggedValue::NULL,
                original_cont,
            },
            |original_cont| ContValue::ResumePrimitive {
                index: 0,
                state: TaggedValue::NULL,
                original_cont,
            },
            |original_cont| ContValue::DynamicWindCleanup {
                after: TaggedValue::NULL,
                wind_id: 0,
                original_cont,
            },
            |original_cont| ContValue::DynamicWindAfterDone {
                result_value: TaggedValue::NULL,
                original_cont,
            },
            |original_cont| ContValue::ExceptionHandlerCleanup { original_cont },
            |original_cont| ContValue::RaiseHandlerReturn {
                continuable: true,
                original_exception: None,
                original_cont,
                popped_handler: None,
            },
            |cleanup_cont| ContValue::DynamicWindSetup {
                wind_record: DynamicWindRecord::new(
                    TaggedValue::NULL,
                    TaggedValue::NULL,
                    Rc::from(vec![]),
                ),
                body: TaggedValue::NULL,
                cleanup_cont,
            },
            |cont| ContValue::AbortLanding {
                handler: TaggedValue::NULL,
                delimited: TaggedValue::NULL,
                cont,
            },
        ];
        for wrap in wrappers {
            let below = recursion(&global, &ContEnv::new(), 2);
            let pending = local(&frame(&global), &below);
            // Twice wrapped, as a `dynamic-wind` inside a `call-with-values`.
            let wrapped = wrap(Box::new(wrap(Box::new(pending))));
            let chain = returning_to(wrapped);
            assert_eq!(capture(&frame(&global), &chain).mark_levels(), 4);
        }
    }

    /// A chain ends at its prompt's boundary and goes on from the prompt's
    /// frame, which the capture's prompt stack holds; and a capture resumes
    /// into `resume` where it has one. Both are walked.
    #[test]
    fn the_prompt_stack_and_the_resume_value_are_walked() {
        let global = namespace();
        let outside = recursion(&global, &ContEnv::new(), 3);
        let inside = recursion(
            &global,
            &returning_to(ContValue::PromptBoundary { id: 7 }),
            2,
        );
        let mut k = capture(&frame(&global), &inside);
        k.prompt_stack.push(PromptFrame {
            id: 7,
            tag: None,
            handler: TaggedValue::FALSE,
            cont: local(&frame(&global), &outside),
            wind_depth: 0,
            trampoline: 0,
            handler_depth: 0,
        });
        // Its frame, two levels inside the prompt, the prompt's own
        // continuation and three below it.
        assert_eq!(k.mark_levels(), 7);

        let mut k = capture(&global, &ContEnv::new());
        k.resume = Some(ContValue::DynamicWindCleanup {
            after: TaggedValue::NULL,
            wind_id: 0,
            original_cont: Box::new(local(
                &frame(&global),
                &recursion(&global, &ContEnv::new(), 2),
            )),
        });
        assert_eq!(k.mark_levels(), 3);
    }

    /// What ends a chain, or goes on through an object charged on its own,
    /// counts nothing: the halt, an exit's landing, a prompt's boundary, and
    /// a captured continuation, as a jump's target is.
    #[test]
    fn a_chains_end_counts_nothing() {
        let global = namespace();
        let target = Rc::new(capture(
            &frame(&global),
            &recursion(&global, &ContEnv::new(), 5),
        ));
        let ends = [
            ContValue::Halt,
            ContValue::ExitLanding { status: 0 },
            ContValue::PromptBoundary { id: 1 },
            ContValue::Captured(Rc::clone(&target)),
            ContValue::Jump {
                entered: None,
                value: TaggedValue::NULL,
                target: Rc::clone(&target),
            },
        ];
        for end in ends {
            assert_eq!(capture(&global, &returning_to(end)).mark_levels(), 0);
        }
        // A composable invoke's own continuation is walked; its target is
        // not.
        let invoke = ContValue::ComposableInvokeStep {
            target: Rc::clone(&target),
            value: TaggedValue::NULL,
            index: 0,
            cont: Box::new(local(&frame(&global), &ContEnv::new())),
        };
        assert_eq!(capture(&global, &returning_to(invoke)).mark_levels(), 1);
        // The target was never walked from here: it is still to be counted.
        assert_eq!(target.mark_levels(), 6);
    }

    /// A chain is as deep as the program's recursion (#725), and the walk is
    /// a loop: 100,000 levels on a test thread's 2 MiB stack.
    #[test]
    fn a_chain_as_deep_as_a_recursion_is_walked_in_a_loop() {
        let global = namespace();
        let chain = recursion(&global, &ContEnv::new(), 100_000);
        assert_eq!(capture(&frame(&global), &chain).mark_levels(), 100_001);
    }

    /// The account, through `Heap::alloc_continuation`: a capture is charged
    /// its slot, its own allocation and `CAPTURED_LEVEL_BYTES` for each
    /// level it is the first to capture; a second capture of the chain, its
    /// slot and its own allocation; a collection counts a live capture's
    /// charge in L; and the sweep of a dead one gives its charge back.
    #[test]
    fn a_capture_is_charged_its_levels_and_credited_them_when_it_dies() {
        let shared = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&shared));
        let chain = recursion(&global, &ContEnv::new(), 10);
        let own = RC_COUNTS + size_of::<CpsContinuation>();
        let mut heap = shared.borrow_mut();
        let heap: &mut Heap = &mut heap;

        let before = heap.bytes_allocated();
        let first = heap.alloc_continuation(Rc::new(capture(&frame(&global), &chain)));
        assert_eq!(
            heap.bytes_allocated() - before,
            (OBJECT_SLOT_BYTES + own + 11 * CAPTURED_LEVEL_BYTES) as u64
        );
        let before = heap.bytes_allocated();
        let second = heap.alloc_continuation(Rc::new(capture(&frame(&global), &chain)));
        assert_eq!(
            heap.bytes_allocated() - before,
            (OBJECT_SLOT_BYTES + own + CAPTURED_LEVEL_BYTES) as u64
        );

        collect_from(heap, &[first, second]);
        let both = heap.live_bytes();
        let reclaimed = heap.bytes_reclaimed();
        collect_from(heap, &[second]);
        let firsts = OBJECT_SLOT_BYTES + own + 11 * CAPTURED_LEVEL_BYTES;
        assert_eq!(both - heap.live_bytes(), firsts);
        assert_eq!(heap.bytes_reclaimed() - reclaimed, firsts as u64);
    }

    /// The three stacks a capture copies are its own bytes, at capacity.
    #[test]
    fn a_captures_stacks_are_charged_at_capacity() {
        let global = namespace();
        let mut k = capture(&global, &ContEnv::new());
        k.exception_handlers = Vec::with_capacity(8);
        k.dynamic_winds = Vec::with_capacity(4);
        k.prompt_stack = Vec::with_capacity(2);
        assert_eq!(
            cps_continuation_payload(&k),
            RC_COUNTS
                + size_of::<CpsContinuation>()
                + 8 * size_of::<ExceptionHandler>()
                + 4 * size_of::<DynamicWindRecord>()
                + 2 * size_of::<PromptFrame>()
        );
    }
}
