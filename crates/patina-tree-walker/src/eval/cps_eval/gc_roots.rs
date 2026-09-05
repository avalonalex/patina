//! GC root providers for the tree-walker (`docs/GC_DESIGN.md` §5.1).
//!
//! The evaluator holds almost no values itself: the live machine state is the
//! in-flight `StepResult` in the trampoline loop, which carries the value,
//! environment, continuation chain, and the wind/prompt/handler stacks. The
//! safe point therefore passes several providers:
//!
//! - `Evaluator` — the global environment.
//! - `LibraryRegistry` — every loaded library, rooted in `patina-runtime` so
//!   both backends share one copy of the rule.
//! - [`StepRoots`] — the transient side, constructed fresh at each safe point
//!   from the loop's `current_step` plus the expression being evaluated,
//!   whose literals are live for the duration of the call.
//! - [`EscapeRoots`] — the `PENDING_ESCAPE` thread-local, which outlives any
//!   single step and so is not part of `StepRoots`.
//!
//! Everything else the tree-walker keeps on the Rust stack (suspended outer
//! `StepResult`s in nested trampolines, primitive argument vectors) is handled
//! by deferral, not tracing — see `GcDeferGuard` and §7.

use patina_core::cps_expr::CpsExpr;
use patina_core::{DynamicWindRecord, GcRoots, GcVisitor};

use super::types::{ExceptionHandler, PromptFrame, StepResult, trace_pending_escape};
use crate::eval::Evaluator;
// Continuation-value tracing moved to patina-core with ContValue itself.
use patina_core::{trace_cont_env, trace_cont_value, trace_exception_handler, trace_prompt_frame};

impl GcRoots for Evaluator {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        visitor.visit_env(&self.global_env);
    }
}

/// The `PENDING_ESCAPE` thread-local: a value and continuation in flight
/// between `set_pending_escape` and `take_pending_escape`, reachable from
/// nowhere else. Its own provider rather than part of [`StepRoots`] because
/// it is not step state — any collection entry must root it, including
/// future ones that do not construct a `StepRoots`.
pub(super) struct EscapeRoots;

impl GcRoots for EscapeRoots {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        trace_pending_escape(visitor);
    }
}

/// Transient roots: the machine state at a trampoline safe point.
pub(super) struct StepRoots<'a> {
    /// The step about to be processed — the entire live machine state.
    pub step: &'a StepResult,
    /// The expression this trampoline was entered with. Its literals stay
    /// live for the whole call even once the step has moved past them.
    pub expr: &'a CpsExpr,
}

impl GcRoots for StepRoots<'_> {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        visitor.visit_expr_literals(self.expr);
        trace_step(self.step, visitor);
    }
}

fn trace_step(step: &StepResult, visitor: &mut GcVisitor<'_>) {
    match step {
        StepResult::Done(value) => visitor.visit(*value),

        StepResult::Continue {
            expr,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } => {
            visitor.visit_expr_literals(expr);
            visitor.visit_env(env);
            trace_cont_env(cont_env, visitor);
            trace_stacks(prompt_stack, dynamic_winds, exception_handlers, visitor);
        }

        StepResult::InvokeContinuation {
            cont,
            value,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } => {
            visitor.visit(*value);
            trace_cont_value(cont, visitor);
            visitor.visit_env(env);
            trace_cont_env(cont_env, visitor);
            trace_stacks(prompt_stack, dynamic_winds, exception_handlers, visitor);
        }

        StepResult::ApplyProc {
            proc,
            args,
            cont,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } => {
            visitor.visit(*proc);
            visitor.visit_slice(args);
            trace_cont_value(cont, visitor);
            visitor.visit_env(env);
            trace_cont_env(cont_env, visitor);
            trace_stacks(prompt_stack, dynamic_winds, exception_handlers, visitor);
        }
    }
}

fn trace_stacks(
    prompt_stack: &[PromptFrame],
    dynamic_winds: &[DynamicWindRecord],
    exception_handlers: &[ExceptionHandler],
    visitor: &mut GcVisitor<'_>,
) {
    for frame in prompt_stack {
        trace_prompt_frame(frame, visitor);
    }
    visitor.visit_winds(dynamic_winds);
    for handler in exception_handlers {
        trace_exception_handler(handler, visitor);
    }
}
