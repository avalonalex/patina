use std::rc::Rc;

use crate::cont_value::{ContValue, PromptFrame};
use crate::cps_expr::CpsExpr;
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

    /// `Some(id)` for a composable continuation: its chain ends at the
    /// [`ContValue::PromptBoundary`] with this id, and invoking it pushes a
    /// [`PromptFrame`] with the same id so the chain returns to the invoker.
    /// `None` for a full continuation, whose invocation replaces the machine.
    ///
    /// The two share this type because they share everything else — a
    /// continuation chain and the dynamic state to run it under — and differ
    /// only in what the stacks below mean: a full continuation's are the
    /// whole stacks at capture, a composable one's are the slices *above*
    /// its prompt, appended to the invoke site's.
    pub boundary: Option<u64>,

    /// The trampoline this continuation's chain ends in.
    ///
    /// A chain ends at a `Halt`, and a `Halt` means one of two things: the
    /// program is over, or a Rust primitive's callback has returned and the
    /// primitive continues. The tree-walker runs a callback on a nested
    /// trampoline, so a continuation captured inside one returns *to that
    /// trampoline*, and invoking it anywhere else has no primitive to return
    /// to. This is the field the jump reads: the same trampoline resumes the
    /// chain in place, an enclosing one is unwound to through the primitive
    /// (the escape), and one that has already returned is an error. It is
    /// what the VM's frame-depth test in `across_reentry` answers there —
    /// here a continuation is not a stack, so the answer is recorded.
    ///
    /// Every outermost trampoline shares the id `0`: a continuation captured
    /// in one top-level form and invoked from a later one resumes the rest
    /// of its own form and then falls through to the next, which is what
    /// every implementation does at the REPL.
    pub trampoline: u64,

    /// A composable continuation whose captured region crosses a primitive's
    /// callback: the abort ran inside the callback, the prompt sits outside
    /// it, so the chain from the abort point reaches the callback's `Halt`
    /// before it reaches the prompt's boundary — the "return to the
    /// primitive, which then continues" between the two is Rust stack, not
    /// a continuation. Resuming it would run the rest of the callback and
    /// then end whatever run it was resumed in. The tree-walker refuses it
    /// instead; the VM, whose callbacks run on the same machine, does not
    /// have the limit.
    pub crosses_callback: bool,

    /// Dynamic wind handlers that were active when this continuation was captured
    /// These need to be reinstalled when the continuation is invoked
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// The prompts established where this continuation was captured, restored
    /// on re-entry exactly as `dynamic_winds` and `exception_handlers` are
    /// (`docs/VM_RUNTIME.md` §5.6, the first row: a `call/cc` capture saves
    /// all of the dynamic state). Until the tree-walker had a prompt API the
    /// arrival reset this to empty, which was harmless only because the stack
    /// was always empty.
    ///
    /// For a composable continuation these are the prompts *inside* the
    /// captured region, with `wind_depth` / `handler_depth` relative to the
    /// region's base — an invoke adds the invoke site's lengths back. Carrying
    /// them is what lets a resumed computation abort to a prompt its own code
    /// established (the VM's #163); relocating them is what keeps such an
    /// abort from truncating the invoke site's stacks at depths that meant
    /// something else (#164).
    pub prompt_stack: Vec<PromptFrame>,

    /// The exception handlers installed where this continuation was captured.
    ///
    /// Restored on re-entry, exactly like `dynamic_winds`. R7RS 6.11 makes the
    /// handler stack part of the dynamic environment, so a continuation that
    /// does not carry it cannot restore the environment it names.
    ///
    /// The VM's `VmContinuation` has always carried the stack; the
    /// tree-walker's escape path reset it to empty on every re-entry, so a
    /// continuation captured under a handler came back without it. Two
    /// quarantined divergences were that gap and converge with this field.
    ///
    /// It is also what R7RS 7.3's `guard` will need (Track L triage families
    /// 22 and 28, in flight): that expansion jumps *out* of the raise point to
    /// run the clauses and, when none matches, back *in* to re-raise — and an
    /// empty stack on the way back turns the re-raise into "unhandled
    /// continuable exception".
    pub exception_handlers: Vec<crate::cont_value::ExceptionHandler>,

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
    pub resume: Option<ContValue>,
}

/// Global counter for generating unique dynamic-wind IDs
static DYNAMIC_WIND_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Global counter for prompt boundary ids.
static PROMPT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Mint a fresh prompt boundary identity, for a `PromptFrame` and the
/// `ContValue::PromptBoundary` that answers to it.
///
/// Not the prompt's *tag*: one tag serves any number of prompts, and a
/// composable continuation captured under one of them must come back to its
/// own frame, not to the innermost with that tag.
pub fn next_prompt_id() -> u64 {
    PROMPT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Mint a fresh `dynamic-wind` identity.
///
/// One counter for both backends, because both need the same thing and for
/// the same reason: a wind record has to be told apart from every other one
/// to find the common prefix of two wind stacks (R7RS §6.10). The `before`
/// thunk cannot serve — two `dynamic-wind` calls may share one closure — and
/// the depth cannot either, since the whole question is where two stacks stop
/// agreeing. Both backends mint identities through the shared record constructor.
pub fn next_dynamic_wind_id() -> u64 {
    DYNAMIC_WIND_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// A dynamic extent carried by continuations in either backend.
///
/// `H` is the backend's exception-handler representation. The VM carries
/// frame depths; the tree-walker does not. Installing and relocating those
/// handlers remains the backend's responsibility.
#[derive(Debug, Clone)]
pub struct WindRecord<H> {
    /// Identity of the `dynamic-wind` call this record came from.
    ///
    /// Compare stack prefixes positionally: an id is unique per invocation
    /// but may repeat within one live stack when a composable continuation
    /// appends its captured records. Do not use ids as unique stack keys.
    pub id: u64,
    /// The "before" thunk to call when entering this dynamic extent
    pub before: TaggedValue,
    /// The "after" thunk to call when leaving this dynamic extent
    pub after: TaggedValue,
    /// The exception handlers installed where `dynamic-wind` was called.
    ///
    /// R7RS 6.10: "The before and after thunks are called in the same
    /// dynamic environment as the call to dynamic-wind", and 6.11 puts the
    /// handler stack in that environment. So a thunk run by a continuation
    /// jump gets *this* stack, not whatever is current at the jump — which
    /// after a `guard` has fired no longer holds the guard's handler, so an
    /// after-thunk that raised went uncaught instead of reaching the guard a
    /// second time (Track L §6, the `finally` rule).
    ///
    /// Shared, not owned: records are cloned into every captured
    /// continuation, and a handler stack copied per clone was the cost the
    /// tree-walker's continuation capture could not afford.
    pub handlers: Rc<[H]>,
}

/// Tree-walker wind record, retaining its backend-specific handler type.
pub type DynamicWindRecord = WindRecord<crate::cont_value::ExceptionHandler>;

impl<H> WindRecord<H> {
    /// Create a new dynamic-wind record with a unique ID, remembering the
    /// handler stack of the `dynamic-wind` call it stands for.
    pub fn new(before: TaggedValue, after: TaggedValue, handlers: Rc<[H]>) -> Self {
        Self {
            id: next_dynamic_wind_id(),
            before,
            after,
            handlers,
        }
    }
}

/// The next action of a full continuation jump (including an abort landing).
/// Backends own the resumable thunk call and dynamic-state installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindStep {
    /// Pop the innermost live record before calling its after thunk.
    Exit,
    /// Call this target record's before thunk, then push it on return.
    Enter(usize),
    /// Install the target machine state and deliver the jump's value.
    Arrive,
}

/// Shared traversal policy for stacks ordered outermost first. Identity is
/// per dynamic-wind invocation, never thunk equality or stack depth. Recompute
/// after each resumable thunk; a thunk can abandon the transfer. Composable
/// invocation appends extents and must not use this replacement policy.
pub fn next_wind_step<T>(current: &[T], target: &[T], id: impl Fn(&T) -> u64) -> WindStep {
    let common = current
        .iter()
        .zip(target)
        .take_while(|(a, b)| id(a) == id(b))
        .count();
    if current.len() > common {
        WindStep::Exit
    } else if current.len() < target.len() {
        WindStep::Enter(current.len())
    } else {
        WindStep::Arrive
    }
}

#[cfg(test)]
mod wind_policy_tests {
    use super::{WindStep, next_wind_step};

    #[test]
    fn sibling_extents_exit_inside_out_then_enter_outside_in() {
        let target = [1, 4, 5];
        let mut live = vec![1, 2, 3];
        let mut steps = Vec::new();
        loop {
            let step = next_wind_step(&live, &target, |id| *id);
            steps.push(step);
            match step {
                WindStep::Exit => {
                    live.pop();
                }
                WindStep::Enter(i) => live.push(target[i]),
                WindStep::Arrive => break,
            }
        }
        assert_eq!(
            steps,
            [
                WindStep::Exit,
                WindStep::Exit,
                WindStep::Enter(1),
                WindStep::Enter(2),
                WindStep::Arrive
            ]
        );
        assert_eq!(next_wind_step::<u64>(&[], &[], |id| *id), WindStep::Arrive);
        assert_eq!(next_wind_step(&[1], &[2], |id| *id), WindStep::Exit);
    }
}
