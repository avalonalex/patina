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
//!   `delimited_continuation_store`. Nothing but this impl reaches them.
//! - **`CallFrame::closure`**, a bare `HeapIndex` rather than a
//!   `TaggedValue`, so a "scan every TaggedValue" pass would step straight
//!   past it. It is rooted through `GcVisitor::visit_object_index`.
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

        for frame in &self.frames {
            trace_frame(frame, visitor);
        }

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

        // The side tables — see the module comment.
        for continuation in self.continuation_store.values() {
            trace_continuation(continuation, visitor);
        }
        for continuation in self.delimited_continuation_store.values() {
            trace_delimited_continuation(continuation, visitor);
        }

        // Register snapshots the tracer holds between its pre/post hooks.
        // Safe points are borrow-free, so this cannot conflict.
        if let Some(tracer) = &self.tracer {
            tracer.borrow().trace_gc_roots(visitor);
        }
    }
}

fn trace_frame(frame: &CallFrame, visitor: &mut GcVisitor<'_>) {
    // A bare HeapIndex, not a TaggedValue.
    if let Some(closure) = frame.closure {
        visitor.visit_object_index(closure);
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
    for frame in &continuation.frames {
        trace_frame(frame, visitor);
    }
    trace_winds(&continuation.dynamic_winds, visitor);
    trace_prompts(&continuation.prompt_stack, visitor);
    trace_handlers(&continuation.exception_handlers, visitor);
}

fn trace_delimited_continuation(
    continuation: &VmDelimitedContinuation,
    visitor: &mut GcVisitor<'_>,
) {
    visitor.visit_slice(&continuation.registers);
    for frame in &continuation.frames {
        trace_frame(frame, visitor);
    }
    trace_winds(&continuation.dynamic_winds, visitor);
}
