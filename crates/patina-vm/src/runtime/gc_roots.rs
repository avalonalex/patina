//! GC root providers for the VM (`docs/GC_DESIGN.md` §5.2).
//!
//! Unlike the tree-walker, whose live state is a Rust local, the VM keeps
//! everything on `VmState` — so `impl GcRoots for VmState` covers almost the
//! whole root set. Two members of it are invisible to a heap scan and are the
//! reason this impl has to exist at all:
//!
//! - **The continuation side tables.** The heap holds only an opaque
//!   `VmContinuationRef(u64)`; the payloads live in `continuation_store` and
//!   `delimited_continuation_store`. Nothing but this impl reaches them —
//!   and they are **weak** (design §9.5, which has the measurements and the
//!   full argument): a payload is traced only when its ref object was
//!   itself marked (`trace_weak_ids`), and entries whose ref died are
//!   pruned after marking (`sweep_weak`). Tracing them as strong roots
//!   instead made every capture immortal — snapshots contain other
//!   continuation refs, so the tables pinned themselves transitively.
//! - **`CallFrame::closure`**, a bare `HeapIndex` rather than a
//!   `TaggedValue`, so a "scan every TaggedValue" pass would step straight
//!   past it. It is rooted through `GcVisitor::visit_object_index`.
//!
//! The weak protocol is sound because every store touch (capture, invoke)
//! is confined to one instruction dispatch and nested loops defer
//! collection — so at a collecting safe point an unmarked ref proves its
//! payload unreachable.
//!
//! Rust-stack temporaries (continuation-capture register clones, `mem::take`n
//! buffers, primitive argument vectors, the `saved_globals` swap windows) are
//! handled by deferral rather than tracing — see `GcDeferGuard` and §7.

use patina_core::{GcRoots, GcVisitor};
use rustc_hash::FxHashMap;

use crate::types::CallFrame;
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};

use super::vm_state::VmState;

impl GcRoots for VmState {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        // The register file is rooted in full, including slots past a frame's
        // live range: they hold `NULL` or stale-but-valid values, and marking
        // a dead-but-valid slot only delays its reclamation by one cycle.
        visitor.visit_slice(&self.registers);
        visitor.visit_slice(&self.scratch_args);

        // A hidden root while it is set: between the stash in `across_reentry`
        // and the `take()` in `run_loop_until`, the escaping continuation's
        // value is reachable from nowhere else. No safe point runs inside that
        // window today, so this is future-proofing on the same reasoning as
        // `scratch_args` above — and it mirrors the tree-walker's
        // `trace_pending_escape`, which roots the same value for the same
        // reason.
        if let Some(v) = self.pending_escape {
            visitor.visit(v);
        }

        trace_frames(&self.frames, visitor);

        // Code objects are never evicted, so their constants are effectively
        // immortal roots. Tracing the store covers every frame's `code` too,
        // since frames only ever hold objects taken from it.
        for code in self.code_store.iter().flatten() {
            visitor.visit_slice(&code.constants);
        }

        visitor.visit_env(&self.globals);

        trace_prompts(&self.prompt_stack, visitor);
        trace_winds(&self.dynamic_winds, visitor);
        trace_handlers(&self.exception_handlers, visitor);

        // The continuation side tables are deliberately NOT traced here —
        // they are weak; see the module comment and `trace_weak_ids`.

        // Register snapshots the tracer holds between its pre/post hooks.
        // Safe points are borrow-free, so this cannot conflict.
        if let Some(tracer) = &self.tracer {
            tracer.borrow().trace_roots(visitor);
        }
    }

    fn trace_weak_ids(&self, ids: &[u64], visitor: &mut GcVisitor<'_>) {
        // Each id was proven live by marking; an id keys at most one entry
        // across both stores (heap-minted), and one not found here belongs
        // to a dead temporary VmState — nothing to trace.
        let conts = self.continuation_store.borrow();
        let delims = self.delimited_continuation_store.borrow();
        for id in ids {
            if let Some(continuation) = conts.get(id) {
                trace_continuation(continuation, visitor);
            } else if let Some(continuation) = delims.get(id) {
                trace_delimited_continuation(continuation, visitor);
            }
        }
    }

    fn sweep_weak(&self, visitor: &GcVisitor<'_>) {
        // Drop every entry whose ref object did not survive marking. The
        // payloads live outside the arenas (Rc'd register/frame snapshots),
        // so dropping them here touches no heap state; the dead ref objects
        // themselves are reclaimed by the sweep that follows.
        prune_store(&self.continuation_store, visitor);
        prune_store(&self.delimited_continuation_store, visitor);
    }
}

/// Retain only live entries, and give back bucket capacity after a churn
/// spike — `retain` never shrinks, and a table that once held thousands of
/// dead captures would otherwise be walked at full width by every future
/// collection (the §9.5 monotonic-residue problem in miniature).
fn prune_store<T>(store: &std::cell::RefCell<FxHashMap<u64, T>>, visitor: &GcVisitor<'_>) {
    let mut store = store.borrow_mut();
    store.retain(|&id, _| visitor.weak_continuation_id_is_live(id));
    if store.len() * 8 < store.capacity() {
        store.shrink_to_fit();
    }
}

fn trace_frames(frames: &[CallFrame], visitor: &mut GcVisitor<'_>) {
    for frame in frames {
        // A bare HeapIndex, not a TaggedValue.
        if let Some(closure) = frame.closure {
            visitor.visit_object_index(closure);
        }
    }
}

fn trace_winds(winds: &[DynamicWindRecord], visitor: &mut GcVisitor<'_>) {
    // The VM has its own `DynamicWindRecord` (it carries a stack depth), so
    // `GcVisitor::visit_winds` — which takes the core type — does not apply.
    for wind in winds {
        visitor.visit(wind.before);
        visitor.visit(wind.after);
        // The handler stack of the record's `dynamic-wind` call, which its
        // thunks run under: reachable from nowhere else once the live stack
        // has moved on, which is exactly when a jump is about to use it.
        trace_handlers(&wind.handlers, visitor);
    }
}

fn trace_prompts(prompts: &[PromptFrame], visitor: &mut GcVisitor<'_>) {
    for prompt in prompts {
        visitor.visit(prompt.tag);
        visitor.visit(prompt.handler);
    }
}

fn trace_handlers(handlers: &[ExceptionHandler], visitor: &mut GcVisitor<'_>) {
    for handler in handlers {
        visitor.visit(handler.handler);
    }
}

fn trace_continuation(continuation: &VmContinuation, visitor: &mut GcVisitor<'_>) {
    visitor.visit_slice(&continuation.registers);
    trace_frames(&continuation.frames, visitor);
    trace_winds(&continuation.dynamic_winds, visitor);
    trace_prompts(&continuation.prompt_stack, visitor);
    trace_handlers(&continuation.exception_handlers, visitor);
}

fn trace_delimited_continuation(
    continuation: &VmDelimitedContinuation,
    visitor: &mut GcVisitor<'_>,
) {
    visitor.visit_slice(&continuation.registers);
    trace_frames(&continuation.frames, visitor);
    trace_winds(&continuation.dynamic_winds, visitor);
}
