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

use super::types::{
    ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult, trace_pending_escape,
};
use crate::eval::Evaluator;

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
        // `tag: Rc<PromptTag>` is a plain Rust struct, not a heap value.
        trace_cont_value(&frame.cont, visitor);
        visitor.visit_winds(&frame.dynamic_winds);
    }
    visitor.visit_winds(dynamic_winds);
    for handler in exception_handlers {
        trace_exception_handler(handler, visitor);
    }
}

fn trace_exception_handler(handler: &ExceptionHandler, visitor: &mut GcVisitor<'_>) {
    visitor.visit(handler.handler);
    visitor.visit_winds(&handler.dynamic_winds);
}

/// Trace a continuation environment.
///
/// Deduped by chain identity: `ContEnv` is a persistent `Rc` list and every
/// `ContValue::Local` captures the chain below it, so an un-memoized walk is
/// exponential (`2ⁿ − 1` node visits — measured at 6.8 s for one collection
/// at nesting depth 26). Skipping an already-seen chain is safe: its entries,
/// and therefore its whole tail, were traced when it was first seen.
fn trace_cont_env(cont_env: &ContEnv, visitor: &mut GcVisitor<'_>) {
    if !visitor.visit_once(cont_env.gc_identity()) {
        return;
    }
    for (_, value) in cont_env.iter() {
        trace_cont_value(value, visitor);
    }
}

/// Trace a continuation value, walking the `Box<ContValue>` chain
/// iteratively — most variants differ only in what they visit before handing
/// off to the continuation they wrap.
fn trace_cont_value(cont: &ContValue, visitor: &mut GcVisitor<'_>) {
    let mut cont = cont;
    loop {
        cont = match cont {
            ContValue::Halt => return,

            ContValue::Local {
                body,
                env,
                cont_env,
                ..
            } => {
                visitor.visit_expr_literals(body);
                visitor.visit_env(env);
                trace_cont_env(cont_env, visitor);
                return;
            }

            ContValue::Captured(k) => return visitor.visit_continuation(k),

            ContValue::CallWithValuesConsumer {
                consumer,
                original_cont,
            } => {
                visitor.visit(*consumer);
                original_cont
            }

            ContValue::ForceCache {
                promise,
                original_cont,
            } => {
                visitor.visit_promise(promise);
                original_cont
            }

            ContValue::DynamicWindCleanup {
                after,
                original_cont,
                ..
            } => {
                visitor.visit(*after);
                original_cont
            }

            ContValue::DynamicWindSetup {
                wind_record,
                body,
                cleanup_cont,
            } => {
                visitor.visit_wind(wind_record);
                visitor.visit(*body);
                cleanup_cont
            }

            ContValue::DynamicWindAfterDone {
                result_value,
                original_cont,
            } => {
                visitor.visit(*result_value);
                original_cont
            }

            ContValue::ExceptionHandlerCleanup { original_cont } => original_cont,

            ContValue::RaiseHandlerReturn {
                original_exception,
                original_cont,
                popped_handler,
                ..
            } => {
                if let Some(exception) = original_exception {
                    visitor.visit(*exception);
                }
                if let Some(handler) = popped_handler {
                    trace_exception_handler(handler, visitor);
                }
                original_cont
            }
        };
    }
}
