//! Sentinel tests for the VM's root provider (#623).
//!
//! Each builds the record under test by a struct literal, so a new field
//! breaks the test as well as the trace that names it; puts a fresh value in
//! every field that can hold one; roots the VM state and nothing else;
//! collects; and checks every value survived (`patina_core::heap::sentinels`,
//! which names the field of one that did not). The collector is driven
//! directly, through `collect_for_tests`, as in `weak_continuation_tests.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::heap::gc::collect_for_tests;
use patina_core::heap::sentinels::Sentinels;
use patina_core::tagged_value::{ObjectIndex, TaggedValue};
use patina_core::{GcController, Heap};
use patina_primitives::PrimitiveRegistry;
use rustc_hash::FxHashMap;

use super::super::control::resume_step;
use super::super::execution_state::ExecutionState;
use super::VmState;
use crate::tracer::StepTracer;
use crate::types::code_object::{Arity, CodeObject};
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::{CallFrame, CodeObjectId};

/// Code with these constants and nothing else.
fn code_holding(constants: Vec<TaggedValue>) -> Rc<CodeObject> {
    Rc::new(CodeObject {
        id: CodeObjectId::new(0, 0),
        name: None,
        instructions: Vec::new(),
        constants,
        num_regs: 1,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
        register_roots: None,
        global_cache: Vec::new(),
        live_closures: Cell::new(0),
    })
}

/// A frame running `code` for the closure `closure`, a value the frame holds
/// as a bare object index.
fn frame(code: &Rc<CodeObject>, closure: TaggedValue) -> CallFrame {
    CallFrame {
        pc: 0,
        register_base: 0,
        num_regs: 1,
        closure: Some(ObjectIndex::of(closure).expect("an object")),
        return_reg: 0,
        code: Rc::clone(code),
    }
}

/// A wind record whose thunks and handler are these values.
fn wind(before: TaggedValue, after: TaggedValue, handler: TaggedValue) -> DynamicWindRecord {
    DynamicWindRecord {
        id: 0,
        before,
        after,
        handlers: Rc::from(vec![ExceptionHandler {
            handler,
            stack_depth: 0,
        }]),
    }
}

fn prompt(tag: TaggedValue, handler: TaggedValue) -> PromptFrame {
    PromptFrame {
        tag,
        stack_depth: 0,
        handler,
        dst: 0,
        dynamic_wind_depth: 0,
        exception_handler_depth: 0,
    }
}

fn handler(handler: TaggedValue) -> ExceptionHandler {
    ExceptionHandler {
        handler,
        stack_depth: 0,
    }
}

fn collect(state: &VmState) {
    let mut heap = state.heap.borrow_mut();
    collect_for_tests(&mut heap, &[state]);
}

/// `VmState`, every field, and through it `ExecutionState` (registers,
/// frames, prompts, winds, handlers), the code store's constants, the
/// globals and the step tracer's snapshots. The continuation stores are weak
/// and have a test of their own below.
#[test]
fn every_vm_state_root_is_traced() {
    let globals = Rc::new(Environment::new());
    let shared: SharedHeap = globals.heap().clone();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h: &mut Heap = &mut heap;
    let register = s.pair(h, "ExecutionState.registers");
    let closure = s.object(h, "ExecutionState.frames: CallFrame.closure");
    let tag = s.vector(h, "ExecutionState.prompt_stack: PromptFrame.tag");
    let prompt_handler = s.string(h, "ExecutionState.prompt_stack: PromptFrame.handler");
    let before = s.pair(h, "ExecutionState.dynamic_winds: WindRecord.before");
    let after = s.vector(h, "ExecutionState.dynamic_winds: WindRecord.after");
    let wind_handler = s.string(h, "ExecutionState.dynamic_winds: WindRecord.handlers");
    let installed = s.object(h, "ExecutionState.exception_handlers");
    let escaping = s.pair(h, "VmState.pending_escape");
    let constant = s.vector(h, "VmState.code_store: CodeObject.constants");
    let global = s.string(h, "VmState.globals");
    let saved_global = s.vector(h, "VmState.saved_globals");
    let scratch = s.object(h, "VmState.scratch_args");
    let pre_regs = s.pair(h, "VmState.tracer: StepTracer.pre_regs");
    let pre_all_regs = s.vector(h, "VmState.tracer: StepTracer.pre_all_regs");
    let gc_pending = h.gc_pending_handle();
    drop(heap);
    globals.define("global", global);
    let saved = Rc::new(Environment::with_heap(shared.clone()));
    saved.define("saved", saved_global);

    let code = code_holding(vec![constant]);
    let state = VmState {
        execution: ExecutionState::from_parts(
            vec![register],
            vec![frame(&code, closure)],
            vec![prompt(tag, prompt_handler)],
            vec![wind(before, after, wind_handler)],
            vec![handler(installed)],
        ),
        pending_escape: Some(escaping),
        pending_transfer: false,
        reentry: Vec::new(),
        next_reentry: 1,
        reentry_kept: None,
        code_store: vec![Rc::clone(&code)],
        empty_code: code_holding(Vec::new()),
        free_code_ids: Vec::new(),
        code_units: FxHashMap::default(),
        wind_jump_code: None,
        value_wind_code: None,
        value_cwv_code: None,
        abort_handler_code: None,
        invoke_step_code: None,
        raise_step_code: None,
        force_code: None,
        resume_codes: [None; resume_step::VARIANTS],
        parameter_set: None,
        globals,
        saved_globals: vec![saved],
        heap: shared,
        primitive_registry: Rc::new(PrimitiveRegistry::new()),
        shadowed_primitives: Vec::new(),
        shadowed_controls: 0,
        scratch_args: vec![scratch],
        continuation_store: RefCell::new(FxHashMap::default()),
        delimited_continuation_store: RefCell::new(FxHashMap::default()),
        tracer: Some(Rc::new(RefCell::new(StepTracer::holding(
            vec![pre_regs],
            vec![pre_all_regs],
        )))),
        library_registry: None,
        loader_registry: None,
        fs: Arc::new(patina_core::NativeFs),
        gc: RefCell::new(GcController::from_env()),
        gc_pending,
    };

    collect(&state);
    s.assert_survived(&state.heap.borrow());
}

/// The payloads of both weak continuation stores, every field: each entry's
/// ref object is rooted (in `scratch_args`), so the weak fixpoint traces the
/// payload, and nothing else reaches the values in it.
#[test]
fn every_continuation_snapshot_field_is_traced() {
    let mut state = VmState::new(Rc::new(Environment::new()));
    let shared = state.heap.clone();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h: &mut Heap = &mut heap;
    let full = [
        s.pair(h, "VmContinuation.registers"),
        s.object(h, "VmContinuation.frames: CallFrame.closure"),
        s.vector(h, "VmContinuation.dynamic_winds: WindRecord.before"),
        s.string(h, "VmContinuation.dynamic_winds: WindRecord.after"),
        s.pair(h, "VmContinuation.dynamic_winds: WindRecord.handlers"),
        s.vector(h, "VmContinuation.prompt_stack: PromptFrame.tag"),
        s.string(h, "VmContinuation.prompt_stack: PromptFrame.handler"),
        s.object(h, "VmContinuation.exception_handlers"),
    ];
    let delimited = [
        s.pair(h, "VmDelimitedContinuation.registers"),
        s.object(h, "VmDelimitedContinuation.frames: CallFrame.closure"),
        s.vector(
            h,
            "VmDelimitedContinuation.dynamic_winds: WindRecord.before",
        ),
        s.string(h, "VmDelimitedContinuation.dynamic_winds: WindRecord.after"),
        s.pair(
            h,
            "VmDelimitedContinuation.dynamic_winds: WindRecord.handlers",
        ),
        s.vector(h, "VmDelimitedContinuation.prompt_stack: PromptFrame.tag"),
        s.string(
            h,
            "VmDelimitedContinuation.prompt_stack: PromptFrame.handler",
        ),
        s.object(h, "VmDelimitedContinuation.exception_handlers"),
    ];
    drop(heap);

    let code = code_holding(Vec::new());
    let [
        register,
        closure,
        before,
        after,
        wind_handler,
        tag,
        prompt_handler,
        installed,
    ] = full;
    let full_ref = state.alloc_vm_continuation(VmContinuation {
        frames: vec![frame(&code, closure)],
        dynamic_winds: vec![wind(before, after, wind_handler)],
        prompt_stack: vec![prompt(tag, prompt_handler)],
        exception_handlers: vec![handler(installed)],
        registers: vec![register],
        deliver_reg: 0,
        exit_status: None,
        abort_landing: false,
        reentry: Rc::from(Vec::new()),
    });
    let [
        register,
        closure,
        before,
        after,
        wind_handler,
        tag,
        prompt_handler,
        installed,
    ] = delimited;
    let delimited_ref = state.alloc_vm_delimited_continuation(VmDelimitedContinuation {
        frames: vec![frame(&code, closure)],
        dynamic_winds: vec![wind(before, after, wind_handler)],
        registers: vec![register],
        base_at_capture: 0,
        deliver_reg: Some(0),
        depth_at_capture: 0,
        wind_depth_at_capture: 0,
        handler_depth_at_capture: 0,
        prompt_stack: vec![prompt(tag, prompt_handler)],
        exception_handlers: vec![handler(installed)],
    });
    state.scratch_args = vec![full_ref, delimited_ref];

    collect(&state);
    assert_eq!(state.continuation_store.borrow().len(), 1);
    assert_eq!(state.delimited_continuation_store.borrow().len(), 1);
    s.assert_survived(&state.heap.borrow());
}
