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
//! (`PRD/GC_PRD.md` §11.4), so no slot would ever be updated (#682). Each
//! provider's trace is derived from its struct (#686).

use patina_core::cps_expr::CpsExpr;
use patina_core::{RootProvider, SlotVisitor, Trace};

use super::types::StepResult;
use crate::eval::Evaluator;

// Each provider's trace is derived (#686): its struct says what each field
// holds. The sentinel tests in `sentinel_tests` below put a fresh value in
// each traced field and read it back after a collection.

impl RootProvider for Evaluator {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        Trace::trace(self, visitor);
    }
}

/// Transient roots: the machine state at a trampoline safe point.
#[derive(Trace)]
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
        Trace::trace(self, visitor);
    }
}

#[cfg(test)]
mod sentinel_tests;
