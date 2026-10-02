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
//! - **`CallFrame::closure`**, a bare index (`ObjectIndex`) rather than a
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

use patina_core::{GC_CHECK, GcRoots, GcVisitor};
use rustc_hash::FxHashMap;

use crate::types::CallFrame;
use crate::types::code_object::CodeObject;
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use patina_core::TaggedValue;

use super::VmState;

/// What retirement writes into a register its frame's map calls dead. In a
/// check build (`GC_CHECK`: debug, or release with `gc-check`) that is
/// `DEAD_SLOT`, which the VM refuses to read (`VmState::reg_at`,
/// `control::argument_reg`) and the heap refuses to store, so a map that
/// calls a live register dead panics at the read rather than handing the
/// program a legal `UNSPECIFIED` (#625, GC_PRD §11.1 invariant 3). A plain
/// release build writes `UNSPECIFIED`.
const RETIRED: TaggedValue = if GC_CHECK {
    TaggedValue::DEAD_SLOT
} else {
    TaggedValue::UNSPECIFIED
};

/// Clear finished compiler temporaries before collection or snapshotting.
/// Keeping the full-vector tracing contract means no captured snapshot or
/// tracer can later expose an untraced pointer to an already swept slot.
/// Delimited snapshots use absolute frame bases but store only their slice.
#[cold]
#[inline(never)]
pub(in crate::runtime) fn retire_registers(
    registers: &mut [TaggedValue],
    frames: &[CallFrame],
    base_at_capture: usize,
) {
    for frame in frames {
        let Some(maps) = &frame.code.register_roots else {
            continue;
        };
        let Some(roots) = maps.get(frame.pc) else {
            continue;
        };
        let dropped = wrong_maps::dropped(roots, frame.num_regs);
        let base = frame.register_base - base_at_capture;
        let window = &mut registers[base..base + frame.num_regs as usize];
        for (reg, value) in window.iter_mut().enumerate() {
            // A tail call can reuse a window larger than the new code needs.
            let live = roots.get(reg / 64).copied().unwrap_or(0) & (1 << (reg % 64)) != 0;
            if !live || dropped == Some(reg) {
                *value = RETIRED;
            }
        }
    }
}

/// The register a wrong-map control drops from `roots` (#625): `None`
/// unless `test-support` is compiled in and its switch is on
/// (`test_support::DropHighestLive`). Without the feature this is a
/// constant `None`, and the comparison in `retire_registers` folds away.
#[cfg(not(feature = "test-support"))]
mod wrong_maps {
    #[inline(always)]
    pub(super) fn dropped(_roots: &[u64], _num_regs: u16) -> Option<usize> {
        None
    }
}

#[cfg(feature = "test-support")]
use crate::test_support as wrong_maps;

impl GcRoots for VmState {
    /// Every field of `VmState` is named (#623): a new one does not compile
    /// here until it is traced or written `field: _` with the reason it holds
    /// no value. The sentinel test `every_vm_state_root_is_traced`
    /// (`trace_sentinel_tests.rs`) puts a fresh value in each traced field
    /// and reads it back after a collection.
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        let VmState {
            execution,
            pending_escape,
            // A flag.
            pending_transfer: _,
            // Re-entry boundary ids.
            reentry: _,
            // The next boundary id.
            next_reentry: _,
            // A count.
            reentry_kept: _,
            code_store,
            // Code with no instructions and no constants.
            empty_code: _,
            // Code ids.
            free_code_ids: _,
            // Code ids.
            code_units: _,
            // A code id; the code is in `code_store`.
            wind_jump_code: _,
            // A code id; the code is in `code_store`.
            value_wind_code: _,
            // A code id; the code is in `code_store`.
            value_cwv_code: _,
            // A code id; the code is in `code_store`.
            abort_handler_code: _,
            // A code id; the code is in `code_store`.
            invoke_step_code: _,
            // A code id; the code is in `code_store`.
            raise_step_code: _,
            // A code id; the code is in `code_store`.
            force_code: _,
            // Code ids; the code is in `code_store`.
            resume_codes: _,
            // A registry index.
            parameter_set: _,
            globals,
            // A handle to the arenas, which the collector is marking.
            heap: _,
            // Primitive functions and their names.
            primitive_registry: _,
            // A bitset over registry indices.
            shadowed_primitives: _,
            // A bitset.
            shadowed_controls: _,
            scratch_args,
            // Weak (design §9.5): traced by `trace_weak_ids` below for the ids
            // marking reached, and pruned by `sweep_weak`.
            continuation_store: _,
            // Weak, like `continuation_store`.
            delimited_continuation_store: _,
            tracer,
            // A root provider of its own: each safe point passes the
            // registry beside this state (`maybe_collect`).
            library_registry: _,
            // Loaders, which hold no values.
            loader_registry: _,
            // The filesystem.
            fs: _,
            // The collector's policy and statistics.
            gc: _,
            // A flag.
            gc_pending: _,
        } = self;

        // The register file, frames and dynamic extents.
        execution.trace_roots(visitor);
        visitor.visit_slice(scratch_args);

        // A hidden root while it is set: between the stash in `across_reentry`
        // and the `take()` in `run_loop_until`, the escaping continuation's
        // value is reachable from nowhere else. No safe point runs inside that
        // window today, so this is future-proofing on the same reasoning as
        // `scratch_args` above — and it mirrors the tree-walker's
        // `trace_pending_escape`, which roots the same value for the same
        // reason.
        if let Some(v) = pending_escape {
            visitor.visit(*v);
        }

        // The constants of every code object still loaded. A form's code stays
        // in the store only while a frame, a captured continuation or a live
        // closure can still run it (#338), so this follows the code in use
        // rather than everything ever compiled. It covers every frame's
        // `code` too: a frame holding a code object is itself what keeps that
        // object in the store.
        for code in code_store.iter() {
            trace_code(code, visitor);
        }

        visitor.visit_env(globals);

        // Register snapshots the tracer holds between its pre/post hooks.
        // Safe points are borrow-free, so this cannot conflict.
        if let Some(tracer) = tracer {
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

/// The constants of a loaded code object: every heap value its instructions
/// can load. Every field is named (#623).
fn trace_code(code: &CodeObject, visitor: &mut GcVisitor<'_>) {
    let CodeObject {
        // A code id.
        id: _,
        // A name.
        name: _,
        // Opcodes and register numbers; an immediate operand is a value that
        // fits in the word, never a heap reference, which goes in `constants`.
        instructions: _,
        constants,
        // A count.
        num_regs: _,
        // Argument counts.
        arity: _,
        // Source positions.
        source_map: _,
        // Liveness bitsets.
        register_roots: _,
        // Environment ids and slot numbers.
        global_cache: _,
        // A count.
        live_closures: _,
    } = code;
    visitor.visit_slice(constants);
}

pub(in crate::runtime) fn trace_frames(frames: &[CallFrame], visitor: &mut GcVisitor<'_>) {
    for frame in frames {
        let CallFrame {
            // An instruction index.
            pc: _,
            // A register index; the window is traced with the register file.
            register_base: _,
            // A count.
            num_regs: _,
            closure,
            // A register number.
            return_reg: _,
            // Its constants are traced through `code_store`, which keeps every
            // code object a frame or a captured continuation can run (#338).
            code: _,
        } = frame;
        // A bare index (`ObjectIndex`), not a TaggedValue.
        if let Some(closure) = closure {
            visitor.visit_object_index(*closure);
        }
    }
}

pub(in crate::runtime) fn trace_winds(winds: &[DynamicWindRecord], visitor: &mut GcVisitor<'_>) {
    visitor.visit_winds_with(winds, trace_handler);
}

pub(in crate::runtime) fn trace_prompts(prompts: &[PromptFrame], visitor: &mut GcVisitor<'_>) {
    for prompt in prompts {
        let PromptFrame {
            tag,
            // A frame depth.
            stack_depth: _,
            handler,
            // A register number.
            dst: _,
            // A depth.
            dynamic_wind_depth: _,
            // A depth.
            exception_handler_depth: _,
        } = prompt;
        visitor.visit(*tag);
        visitor.visit(*handler);
    }
}

pub(in crate::runtime) fn trace_handlers(
    handlers: &[ExceptionHandler],
    visitor: &mut GcVisitor<'_>,
) {
    for handler in handlers {
        trace_handler(handler, visitor);
    }
}

fn trace_handler(handler: &ExceptionHandler, visitor: &mut GcVisitor<'_>) {
    let ExceptionHandler {
        handler,
        // A frame depth.
        stack_depth: _,
    } = handler;
    visitor.visit(*handler);
}

fn trace_continuation(continuation: &VmContinuation, visitor: &mut GcVisitor<'_>) {
    let VmContinuation {
        frames,
        dynamic_winds,
        prompt_stack,
        exception_handlers,
        registers,
        // A register number.
        deliver_reg: _,
        // An exit status.
        exit_status: _,
        // A flag.
        abort_landing: _,
        // Re-entry boundary ids.
        reentry: _,
    } = continuation;
    visitor.visit_slice(registers);
    trace_frames(frames, visitor);
    trace_winds(dynamic_winds, visitor);
    trace_prompts(prompt_stack, visitor);
    trace_handlers(exception_handlers, visitor);
}

fn trace_delimited_continuation(
    continuation: &VmDelimitedContinuation,
    visitor: &mut GcVisitor<'_>,
) {
    let VmDelimitedContinuation {
        frames,
        dynamic_winds,
        registers,
        // A register index.
        base_at_capture: _,
        // A register number.
        deliver_reg: _,
        // A depth.
        depth_at_capture: _,
        // A depth.
        wind_depth_at_capture: _,
        // A depth.
        handler_depth_at_capture: _,
        prompt_stack,
        exception_handlers,
    } = continuation;
    visitor.visit_slice(registers);
    trace_frames(frames, visitor);
    trace_winds(dynamic_winds, visitor);
    trace_prompts(prompt_stack, visitor);
    trace_handlers(exception_handlers, visitor);
}
