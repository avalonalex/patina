//! GC root providers for the tree-walker (`docs/GC_DESIGN.md` §5.1).
//!
//! The evaluator holds almost no values itself: the live machine state is the
//! in-flight `StepResult` in the trampoline loop, which carries the value,
//! environment, continuation chain, and the wind/prompt/handler stacks. The
//! safe point lends a collection the machine's own state:
//!
//! - `Evaluator` — the global environment, and the escape parked between a
//!   jump and the trampoline that resumes it, which outlives any one step.
//! - [`StepRoots`] — the transient side, constructed fresh at each safe point
//!   from the loop's `current_step` plus the expression being evaluated,
//!   whose literals are live for the duration of the call.
//!
//! The library registry is registered with the heap instead (#682), and
//! rooted in `patina-runtime`, so both backends share one copy of the rule.
//!
//! Everything else the tree-walker keeps on the Rust stack (suspended outer
//! `StepResult`s in nested trampolines, primitive argument vectors) is handled
//! by deferral, not tracing — see `GcDeferGuard` and §7.
//!
//! Every value is reported pinned, by copy: the tree-walker's heaps never move
//! (`PRD/GC_PRD.md` §11.4), so no slot would ever be updated (#682).

use patina_core::cps_expr::CpsExpr;
use patina_core::{DynamicWindRecord, RootProvider, SlotVisitor, trace_winds};

use super::types::{ExceptionHandler, PromptFrame, StepResult};
use crate::eval::Evaluator;
// Continuation-value tracing moved to patina-core with ContValue itself.
use patina_core::{trace_cont_value, trace_exception_handler, trace_prompt_frame};

// Every provider here takes its struct apart by name (#623), so a new field
// does not compile until it is traced or written `field: _` with the reason it
// holds no value. The sentinel tests in `sentinel_tests` below put a fresh
// value in each traced field and read it back after a collection.

impl RootProvider for Evaluator {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        let Evaluator {
            global_env,
            // Which debug stages print.
            debug: _,
            // A root provider of its own, registered with the heap through
            // `library_roots` (#682).
            library_registry: _,
            // A registration.
            library_roots: _,
            pending_escape,
            // Loaders, which hold no values.
            loader_registry: _,
            // Primitive functions and their names.
            primitive_registry: _,
            // The filesystem.
            fs: _,
            // The collector's policy and statistics.
            gc: _,
            // A flag.
            gc_pending: _,
            // An error report: strings and library names.
            bootstrap_error: _,
        } = self;
        visitor.env(global_env);
        // A hidden root while it is set: between `set_pending_escape` and
        // `take_pending_escape`, the value and its continuation are reachable
        // from nowhere else (`docs/GC_DESIGN.md` §5.1).
        if let Some((value, k)) = pending_escape.borrow().as_ref() {
            visitor.pinned(*value);
            visitor.continuation(k);
        }
    }
}

/// Transient roots: the machine state at a trampoline safe point.
pub(super) struct StepRoots<'a> {
    /// The step about to be processed — the entire live machine state.
    pub step: &'a StepResult,
    /// The expression this trampoline was entered with. Its literals stay
    /// live for the whole call even once the step has moved past them. A
    /// callback run has none — and never collects either, being nested.
    pub expr: Option<&'a CpsExpr>,
}

impl RootProvider for StepRoots<'_> {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        let StepRoots { step, expr } = self;
        if let Some(expr) = expr {
            visitor.expr_literals(expr);
        }
        trace_step(step, visitor);
    }
}

fn trace_step(step: &StepResult, visitor: &mut dyn SlotVisitor) {
    match step {
        StepResult::Done(value) => visitor.pinned(*value),

        StepResult::Continue {
            expr,
            env,
            param,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } => {
            visitor.expr_literals(expr);
            visitor.env(env);
            // Delivered to the continuation and not yet bound: the trampoline
            // polls between the invocation and the step that binds it (#680).
            if let Some((
                // The parameter's name.
                _,
                value,
            )) = param
            {
                visitor.pinned(*value);
            }
            visitor.cont_env(cont_env);
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
            visitor.pinned(*value);
            trace_cont_value(cont, visitor);
            visitor.env(env);
            visitor.cont_env(cont_env);
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
            visitor.pinned(*proc);
            for &arg in args {
                visitor.pinned(arg);
            }
            trace_cont_value(cont, visitor);
            visitor.env(env);
            visitor.cont_env(cont_env);
            trace_stacks(prompt_stack, dynamic_winds, exception_handlers, visitor);
        }

        StepResult::CollectAtCall {
            // Which collection: no value.
            kind: _,
            cont,
            env,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        } => {
            trace_cont_value(cont, visitor);
            visitor.env(env);
            visitor.cont_env(cont_env);
            trace_stacks(prompt_stack, dynamic_winds, exception_handlers, visitor);
        }
    }
}

fn trace_stacks(
    prompt_stack: &[PromptFrame],
    dynamic_winds: &[DynamicWindRecord],
    exception_handlers: &[ExceptionHandler],
    visitor: &mut dyn SlotVisitor,
) {
    for frame in prompt_stack {
        trace_prompt_frame(frame, visitor);
    }
    trace_winds(dynamic_winds, visitor);
    for handler in exception_handlers {
        trace_exception_handler(handler, visitor);
    }
}

#[cfg(test)]
mod sentinel_tests;
