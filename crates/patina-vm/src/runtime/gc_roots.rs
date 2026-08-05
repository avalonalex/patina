//! GC root providers for the VM (`docs/GC_DESIGN.md` §5.2).
//!
//! Unlike the tree-walker, whose live state is a Rust local, the VM keeps
//! everything on `VmState` — so `impl GcRoots for VmState` covers almost the
//! whole root set. Two members of it are invisible to a heap scan and are the
//! reason this impl has to exist at all:
//!
//! - **The continuation side tables.** The heap holds only an opaque
//!   `VmContinuationRef(u64)`; the captured registers, frames, and wind /
//!   prompt / handler stacks live in `continuation_store` and
//!   `delimited_continuation_store`. Nothing but this impl reaches them —
//!   and they are **weak** (design §9.5): a payload is traced only when its
//!   ref object was itself marked (`trace_weak_roots`), and entries whose
//!   ref died are pruned after marking (`sweep_weak`). Tracing them as
//!   strong roots instead would make every capture immortal: payload
//!   snapshots routinely contain other continuation refs (in ctak, chains
//!   of them), so the tables would pin themselves transitively forever —
//!   the measured 4 GB blowup.
//! - **`CallFrame::closure`**, a bare `HeapIndex` rather than a
//!   `TaggedValue`, so a "scan every TaggedValue" pass would step straight
//!   past it. It is rooted through `GcVisitor::visit_object_index`.
//!
//! The weak protocol is sound because store entries are only ever reached
//! through their ref `TaggedValue`: capture inserts the entry and allocates
//! the ref within one instruction dispatch, invocation copies the payload
//! back into `VmState` within one dispatch, and every nested loop
//! (wind thunks, re-entrant primitives) defers collection — so at a
//! collecting safe point an unmarked ref proves its payload unreachable.
//!
//! Rust-stack temporaries (continuation-capture register clones, `mem::take`n
//! buffers, primitive argument vectors, the `saved_globals` swap windows) are
//! handled by deferral rather than tracing — see `GcDeferGuard` and §7.

use patina_core::{GcRoots, GcVisitor};

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
        visitor.visit_slice(&self.value_buffer);
        visitor.visit_slice(&self.scratch_args);

        trace_frames(&self.frames, visitor);

        // Code objects are never evicted, so their constants are effectively
        // immortal roots. Tracing the store covers every frame's `code` too,
        // since frames only ever hold objects taken from it.
        for code in self.code_store.values() {
            visitor.visit_slice(&code.constants);
        }

        visitor.visit_env(&self.globals);

        trace_prompts(&self.prompt_stack, visitor);
        trace_winds(&self.dynamic_winds, visitor);
        trace_handlers(&self.exception_handlers, visitor);

        // The continuation side tables are deliberately NOT traced here —
        // they are weak; see the module comment and `trace_weak_roots`.

        // Register snapshots the tracer holds between its pre/post hooks.
        // Safe points are borrow-free, so this cannot conflict.
        if let Some(tracer) = &self.tracer {
            tracer.borrow().trace_roots(visitor);
        }
    }

    fn trace_weak_roots(&self, visitor: &mut GcVisitor<'_>) -> bool {
        let mut progress = false;

        // Ids handed out by the visitor were proven live by marking; trace
        // the payloads they key. A payload may reference further ref
        // objects — the driver re-drains and calls back until quiescent.
        // Ids absent from the store (a foreign VmState's captures) key no
        // payload here and are not progress.
        let store = self.continuation_store.borrow();
        for id in visitor.take_new_vm_continuation_ids() {
            if let Some(continuation) = store.get(&id) {
                trace_continuation(continuation, visitor);
                progress = true;
            }
        }
        drop(store);

        let store = self.delimited_continuation_store.borrow();
        for id in visitor.take_new_vm_delimited_continuation_ids() {
            if let Some(continuation) = store.get(&id) {
                trace_delimited_continuation(continuation, visitor);
                progress = true;
            }
        }

        progress
    }

    fn sweep_weak(&self, visitor: &GcVisitor<'_>) {
        // Drop every entry whose ref object did not survive marking. The
        // payloads live outside the arenas (Rc'd register/frame snapshots),
        // so dropping them here touches no heap state; the dead ref objects
        // themselves are reclaimed by the sweep that follows.
        self.continuation_store
            .borrow_mut()
            .retain(|&id, _| visitor.vm_continuation_is_live(id));
        self.delimited_continuation_store
            .borrow_mut()
            .retain(|&id, _| visitor.vm_delimited_continuation_is_live(id));
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
        trace_winds(&handler.dynamic_winds, visitor);
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
