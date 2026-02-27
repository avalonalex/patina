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

    /// Captured continuation bindings that were in scope when this continuation
    /// was captured. Each entry is (name, continuation) representing a let-cont
    /// binding that the continuation body may reference.
    /// This is a Vec of boxed CpsContinuations rather than a HashMap to avoid
    /// circular type dependencies and to keep patina-core dependency-free.
    pub captured_cont_bindings: Vec<(Rc<str>, Rc<CpsContinuation>)>,
}

/// Global counter for generating unique dynamic-wind IDs
static DYNAMIC_WIND_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
            id: DYNAMIC_WIND_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            before,
            after,
        }
    }
}
