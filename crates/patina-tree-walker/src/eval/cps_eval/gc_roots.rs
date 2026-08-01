//! GC root providers for the tree-walker (`docs/GC_DESIGN.md` §5.1).
//!
//! The evaluator holds almost no values itself: the live machine state is the
//! in-flight `StepResult` in the trampoline loop, which carries the value,
//! environment, continuation chain, and the wind/prompt/handler stacks. That
//! makes rooting a two-part job:
//!
//! - [`EvaluatorRoots`] — the long-lived side: global environment and every
//!   loaded library (exports *and* environment).
//! - [`StepRoots`] — the transient side, constructed fresh at each safe point
//!   from the loop's `current_step` plus the expression being evaluated,
//!   whose literals are live for the duration of the call.
//!
//! Everything else the tree-walker keeps on the Rust stack (suspended outer
//! `StepResult`s in nested trampolines, primitive argument vectors) is handled
//! by deferral, not tracing — see `GcDeferGuard` and §7.

use patina_core::cps_expr::CpsExpr;
use patina_core::{DynamicWindRecord, GcRoots, GcVisitor, PromiseState};

use super::types::{
    ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult, trace_pending_escape,
};
use crate::eval::Evaluator;

/// Long-lived roots: the global environment and the library registry.
///
/// Wrapping `Evaluator` rather than implementing `GcRoots` on it directly
/// keeps the trait dependency out of the evaluator's public API and makes the
/// borrow requirement explicit: the caller must ensure `library_registry` is
/// not already mutably borrowed (the safe point checks this).
pub(super) struct EvaluatorRoots<'a>(pub &'a Evaluator);

impl GcRoots for EvaluatorRoots<'_> {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        visitor.visit_env(&self.0.global_env);
        for library in self.0.library_registry.borrow().iter_libraries() {
            for (_, tv) in library.exports_iter_tagged() {
                visitor.visit(tv);
            }
            visitor.visit_env(&library.env);
        }
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
        trace_pending_escape(visitor);
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
        trace_winds(&frame.dynamic_winds, visitor);
    }
    trace_winds(dynamic_winds, visitor);
    for handler in exception_handlers {
        trace_exception_handler(handler, visitor);
    }
}

fn trace_winds(winds: &[DynamicWindRecord], visitor: &mut GcVisitor<'_>) {
    for wind in winds {
        visitor.visit(wind.before);
        visitor.visit(wind.after);
    }
}

fn trace_exception_handler(handler: &ExceptionHandler, visitor: &mut GcVisitor<'_>) {
    visitor.visit(handler.handler);
    trace_winds(&handler.dynamic_winds, visitor);
}

fn trace_cont_env(cont_env: &ContEnv, visitor: &mut GcVisitor<'_>) {
    for (_, value) in cont_env.iter() {
        trace_cont_value(value, visitor);
    }
}

/// Trace a continuation value. Recursive over the `Box<ContValue>` chain;
/// depth is bounded by nesting of `dynamic-wind` / `call-with-values` /
/// exception handlers, not by data size.
fn trace_cont_value(cont: &ContValue, visitor: &mut GcVisitor<'_>) {
    match cont {
        ContValue::Halt => {}

        ContValue::Local {
            body,
            env,
            cont_env,
            ..
        } => {
            visitor.visit_expr_literals(body);
            visitor.visit_env(env);
            trace_cont_env(cont_env, visitor);
        }

        ContValue::Captured(k) => visitor.visit_continuation(k),

        ContValue::CallWithValuesConsumer {
            consumer,
            original_cont,
        } => {
            visitor.visit(*consumer);
            trace_cont_value(original_cont, visitor);
        }

        ContValue::ForceCache {
            promise,
            original_cont,
        } => {
            match *promise.borrow() {
                PromiseState::Delayed(tv) | PromiseState::Forced(tv) => visitor.visit(tv),
            }
            trace_cont_value(original_cont, visitor);
        }

        ContValue::DynamicWindCleanup {
            after,
            original_cont,
            ..
        } => {
            visitor.visit(*after);
            trace_cont_value(original_cont, visitor);
        }

        ContValue::DynamicWindSetup {
            wind_record,
            body,
            cleanup_cont,
        } => {
            visitor.visit(wind_record.before);
            visitor.visit(wind_record.after);
            visitor.visit(*body);
            trace_cont_value(cleanup_cont, visitor);
        }

        ContValue::DynamicWindAfterDone {
            result_value,
            original_cont,
        } => {
            visitor.visit(*result_value);
            trace_cont_value(original_cont, visitor);
        }

        ContValue::ExceptionHandlerCleanup { original_cont } => {
            trace_cont_value(original_cont, visitor)
        }

        ContValue::RaiseHandlerReturn {
            original_exception,
            original_cont,
            popped_handler,
            ..
        } => {
            if let Some(exception) = original_exception {
                visitor.visit(*exception);
            }
            trace_cont_value(original_cont, visitor);
            if let Some(handler) = popped_handler {
                trace_exception_handler(handler, visitor);
            }
        }
    }
}
