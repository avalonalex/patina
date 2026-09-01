use std::rc::Rc;

use crate::cps_expr::{CpsExpr, PromptTag};
use crate::environment::Environment;
use crate::tagged_value::TaggedValue;

/// A captured CPS continuation
///
/// In CPS, a continuation represents "what to do with a value". When captured
/// by `call/cc` or `shift`, the continuation becomes a first-class value that
/// can be stored and invoked later.
///
/// ## Full vs Delimited Continuations
///
/// - **Full continuation** (from `call/cc`): Captures everything from the call
///   site to the top level. When invoked, abandons the current computation.
///
/// - **Delimited continuation** (from `shift`): Captures only up to the nearest
///   enclosing `reset` prompt. When invoked, can return to the caller.
#[derive(Debug, Clone)]
pub struct CpsContinuation {
    /// The CPS expression representing the captured computation
    /// When the continuation is invoked with a value, this expression
    /// is evaluated with the value bound to `param`.
    pub body: Rc<CpsExpr>,

    /// The parameter name that receives the value when continuation is invoked
    pub param: Rc<str>,

    /// The captured environment at the point of continuation capture
    pub env: Rc<Environment>,

    /// For delimited continuations: the prompt tag this was captured at
    /// None for full continuations (call/cc)
    pub prompt_tag: Option<Rc<PromptTag>>,

    /// Dynamic wind handlers that were active when this continuation was captured
    /// These need to be reinstalled when the continuation is invoked
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// The continuation environment in scope where this was captured.
    ///
    /// The body may reference let-cont bindings by name, so re-entry has to
    /// restore them. This holds the evaluator's own `ContEnv` -- an `Rc` cons
    /// list, so capturing is a refcount bump and restoring is a move.
    ///
    /// It used to be a `Vec<(Rc<str>, Rc<CpsContinuation>)>` projection, on the
    /// stated grounds of keeping this crate dependency-free. That did not hold
    /// -- every payload of `ContValue` is already a `patina-core` type -- and
    /// the projection could only express `ContValue::Local`, so the other
    /// variants were either encoded under sentinel names or silently dropped.
    pub captured_cont_env: crate::cont_value::ContEnv,

    /// The continuation value to resume into, when it is not simply the
    /// `body`/`param`/`env` triple above.
    ///
    /// Effect-carrying variants -- `DynamicWindCleanup` above all -- cannot be
    /// flattened into a body: re-entry has to re-establish the wind cleanup, not
    /// just jump to the expression underneath. They used to be encoded as fake
    /// bindings named `__dw_after__` / `__dw_wind_id__` / `__dw_original__`
    /// behind a `__dynamic_wind_cleanup__` marker body, which three separate
    /// places had to recognise and decode. Now the value is simply stored.
    pub resume: Option<crate::cont_value::ContValue>,
}

/// Global counter for generating unique dynamic-wind IDs
static DYNAMIC_WIND_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Mint a fresh `dynamic-wind` identity.
///
/// One counter for both backends, because both need the same thing and for
/// the same reason: a wind record has to be told apart from every other one
/// to find the common prefix of two wind stacks (R7RS §6.10). The `before`
/// thunk cannot serve — two `dynamic-wind` calls may share one closure — and
/// the depth cannot either, since the whole question is where two stacks stop
/// agreeing. The VM keeps its own record type, so it mints through here
/// rather than duplicating the counter.
pub fn next_dynamic_wind_id() -> u64 {
    DYNAMIC_WIND_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// A record of a dynamic-wind that needs to be managed during continuation jumps
#[derive(Debug, Clone, Copy)]
pub struct DynamicWindRecord {
    /// Unique identifier for this dynamic-wind invocation
    /// Used to find the common prefix when switching continuations
    pub id: u64,
    /// The "before" thunk to call when entering this dynamic extent
    pub before: TaggedValue,
    /// The "after" thunk to call when leaving this dynamic extent
    pub after: TaggedValue,
}

impl DynamicWindRecord {
    /// Create a new dynamic-wind record with a unique ID
    pub fn new(before: TaggedValue, after: TaggedValue) -> Self {
        Self {
            id: next_dynamic_wind_id(),
            before,
            after,
        }
    }
}
