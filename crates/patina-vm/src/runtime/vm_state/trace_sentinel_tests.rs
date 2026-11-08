//! Sentinel tests for the VM's root provider (#623).
//!
//! Each builds the record under test by a struct literal, so a new field
//! breaks the test as well as the trace that names it; puts a fresh value in
//! every field that can hold one; roots the VM state and nothing else;
//! collects; and checks every value survived (`patina_core::heap::sentinels`,
//! which names the field of one that did not). The collector is driven
//! directly, through `collect_for_tests`, as in `weak_continuation_tests.rs`.
//!
//! Beside them, the test that the provider reports its slots where they live,
//! by a visitor that writes into each and puts it back (#682); and the
//! tests of tracing by code id (#836), each of which loads a unit, links its
//! table by hand in a namespace it then drops, and puts the unit in one
//! place only, to show that a holder there keeps the table, its records and
//! its namespace, and that the code store alone keeps none of them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::heap::gc::collect_for_tests;
use patina_core::heap::sentinels::{Sentinels, assert_freed};
use patina_core::tagged_value::TaggedValue;
use patina_core::{GcController, Heap, LinkTable};
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
use crate::types::{CallFrame, CodeObjectId, FrameCode};

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
        links: LinkTable::empty(),
        live_closures: Cell::new(0),
    })
}

/// A frame running `code` for the closure `closure`, which the frame holds
/// in a slot.
fn frame(code: &Rc<CodeObject>, closure: TaggedValue) -> CallFrame {
    CallFrame {
        pc: 0,
        register_base: 0,
        num_regs: 1,
        closure: Cell::new(closure),
        return_reg: 0,
        code: FrameCode(Rc::clone(code)),
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
    let shared: SharedHeap = patina_core::heap::new_shared_heap();
    let globals = Rc::new(Environment::with_heap(&shared));
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
    let scratch = s.object(h, "VmState.scratch_args");
    let pre_regs = s.pair(h, "VmState.tracer: StepTracer.pre_regs");
    let pre_all_regs = s.vector(h, "VmState.tracer: StepTracer.pre_all_regs");
    let mutator = Rc::clone(h.mutator());
    let instance = Rc::clone(h.instance());
    drop(heap);
    globals.define(&shared, "global", global);

    let code = code_holding(vec![constant]);
    let state = VmState {
        execution: ExecutionState::from_parts(
            vec![Cell::new(register)],
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
        trace_epoch: Cell::new(0),
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
        mutator,
        instance,
        driver: None,
    };

    collect(&state);
    s.assert_survived(&state.heap.borrow());
}

/// The payloads of both weak continuation stores, every field: each entry's
/// ref object is rooted (in `scratch_args`), so the weak fixpoint traces the
/// payload, and nothing else reaches the values in it.
#[test]
fn every_continuation_snapshot_field_is_traced() {
    let mut state = VmState::on_a_fresh_heap();
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
        registers: vec![Cell::new(register)],
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
        registers: vec![Cell::new(register)],
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

// ----------------------------------------------------------------------------
// Slots (#682)
// ----------------------------------------------------------------------------

/// A visitor that writes a marker into every slot it is shown, remembering by
/// the slot's address what it held, and on its second pass puts that back.
#[derive(Default)]
struct Rewriter {
    /// Each slot shown: what it held, and the marker written over it.
    slots: std::collections::HashMap<*const Cell<TaggedValue>, (TaggedValue, TaggedValue)>,
    restoring: bool,
}

impl patina_core::SlotVisitor for Rewriter {
    fn slot(&mut self, slot: &Cell<TaggedValue>) {
        if self.restoring {
            slot.set(self.slots[&std::ptr::from_ref(slot)].0);
        } else {
            let marker = TaggedValue::fixnum(1_000 + self.slots.len() as i64);
            self.slots
                .insert(std::ptr::from_ref(slot), (slot.get(), marker));
            slot.set(marker);
        }
    }

    // Values by copy, which no visitor writes.
    fn pinned(&mut self, _value: TaggedValue) {}
    fn env(&mut self, _env: &Rc<Environment>) {}
    fn continuation(&mut self, _k: &Rc<patina_core::CpsContinuation>) {}
    fn cont_env(&mut self, _cont_env: &patina_core::cont_value::ContEnv) {}
    fn expr_literals(&mut self, _expr: &patina_core::CpsExpr) {}
    fn code(&mut self, _id: u64) {}
}

impl Rewriter {
    /// The marker `slot` was given.
    fn marker(&self, slot: &Cell<TaggedValue>) -> TaggedValue {
        self.slots
            .get(&std::ptr::from_ref(slot))
            .expect("the slot was reported")
            .1
    }
}

/// The VM reports its registers and its frames' closures where they live,
/// the live ones and a captured continuation's, full and delimited: a
/// visitor that writes into each slot it is shown changes the VM's own
/// state, and puts it back. The other providers, the tree-walker's, the
/// library registry and the step tracer, report every value pinned.
#[test]
fn every_slot_the_vm_reports_is_where_its_value_lives() {
    use patina_core::RootProvider;
    let fixnum = TaggedValue::fixnum;
    let mut state = VmState::on_a_fresh_heap();
    let code = code_holding(Vec::new());
    state.execution = ExecutionState::from_parts(
        vec![Cell::new(fixnum(1)), Cell::new(fixnum(2))],
        vec![frame(&code, fixnum(3))],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    state.alloc_vm_continuation(VmContinuation {
        frames: vec![frame(&code, fixnum(5))],
        dynamic_winds: Vec::new(),
        prompt_stack: Vec::new(),
        exception_handlers: Vec::new(),
        registers: vec![Cell::new(fixnum(4))],
        deliver_reg: 0,
        exit_status: None,
        abort_landing: false,
        reentry: Rc::from(Vec::new()),
    });
    state.alloc_vm_delimited_continuation(VmDelimitedContinuation {
        frames: vec![frame(&code, fixnum(7))],
        dynamic_winds: Vec::new(),
        registers: vec![Cell::new(fixnum(6))],
        base_at_capture: 0,
        deliver_reg: None,
        depth_at_capture: 0,
        wind_depth_at_capture: 0,
        handler_depth_at_capture: 0,
        prompt_stack: Vec::new(),
        exception_handlers: Vec::new(),
    });
    let ids: Vec<u64> = (state.continuation_store.borrow().keys())
        .chain(state.delimited_continuation_store.borrow().keys())
        .copied()
        .collect();
    let values = |state: &VmState| {
        let mut values = Vec::new();
        for_each_slot(state, |slot| values.push(slot.get()));
        values
    };
    let held = values(&state);
    assert_eq!(held, (1..=7).map(fixnum).collect::<Vec<_>>());

    let mut rewriter = Rewriter::default();
    state.trace_roots(&mut rewriter);
    state.trace_weak_ids(&ids, &mut rewriter);
    assert_eq!(rewriter.slots.len(), 7, "every slot, and each once");
    for_each_slot(&state, |slot| assert_eq!(slot.get(), rewriter.marker(slot)));

    rewriter.restoring = true;
    state.trace_roots(&mut rewriter);
    state.trace_weak_ids(&ids, &mut rewriter);
    assert_eq!(values(&state), held);
}

/// Every slot the VM holds a value in, read through the state that owns
/// it: the live registers and frames' closures, and the one full and one
/// delimited capture's.
fn for_each_slot(state: &VmState, mut each: impl FnMut(&Cell<TaggedValue>)) {
    let execution = &state.execution;
    let full = state.continuation_store.borrow();
    let delimited = state.delimited_continuation_store.borrow();
    let full = full.values().next().expect("one full capture");
    let delimited = delimited.values().next().expect("one delimited capture");
    for slot in (execution.registers().iter())
        .chain(execution.frames().iter().map(|f| &f.closure))
        .chain(&full.registers)
        .chain(full.frames.iter().map(|f| &f.closure))
        .chain(&delimited.registers)
        .chain(delimited.frames.iter().map(|f| &f.closure))
    {
        each(slot);
    }
}

// ----------------------------------------------------------------------------
// Link tables, traced by code id (#836)
// ----------------------------------------------------------------------------

/// The two values a unit's linked table keeps: `x`, which the table links,
/// through its entry, the record and its cell; and `y`, which only the
/// namespace binds, through the namespace the table was linked in.
fn linked_values(heap: &mut Heap, s: &mut Sentinels) -> (TaggedValue, TaggedValue) {
    (
        s.pair(heap, "LinkTable.entries: x, through its record and cell"),
        s.vector(heap, "LinkTable.namespace: y, bound only there"),
    )
}

/// Load `code` as a unit of its own, through `load_unit` so that its id
/// resolves, with a link table over `names`, linked in a fresh namespace
/// that binds `x`, and `y`, which the table does not name. The namespace's
/// `Rc` is dropped here, so only the table holds it, and only the code
/// store holds the unit: each test puts the unit where the edge it tests
/// runs from.
fn load_linked(
    state: &mut VmState,
    mut code: CodeObject,
    names: &[&str],
    x: TaggedValue,
    y: TaggedValue,
) -> CodeObjectId {
    code.links = Rc::new(LinkTable::new(
        names.iter().map(|&name| Rc::from(name)).collect(),
    ));
    let id = state.load_unit(code, Vec::new());
    let ns = Rc::new(Environment::with_heap(&state.heap));
    ns.define(&state.heap, "x", x);
    ns.define(&state.heap, "y", y);
    (state.code_object(id).expect("just loaded").links).link_in(&state.heap, &ns);
    id
}

/// A full continuation whose one frame runs `code`.
fn full_capture_of(code: &Rc<CodeObject>) -> VmContinuation {
    VmContinuation {
        frames: vec![frame(code, CallFrame::NO_CLOSURE)],
        dynamic_winds: Vec::new(),
        prompt_stack: Vec::new(),
        exception_handlers: Vec::new(),
        registers: vec![Cell::new(TaggedValue::NULL)],
        deliver_reg: 0,
        exit_status: None,
        abort_landing: false,
        reentry: Rc::from(Vec::new()),
    }
}

/// A delimited continuation whose one frame runs `code`.
fn delimited_capture_of(code: &Rc<CodeObject>) -> VmDelimitedContinuation {
    VmDelimitedContinuation {
        frames: vec![frame(code, CallFrame::NO_CLOSURE)],
        dynamic_winds: Vec::new(),
        registers: vec![Cell::new(TaggedValue::NULL)],
        base_at_capture: 0,
        deliver_reg: None,
        depth_at_capture: 0,
        wind_depth_at_capture: 0,
        handler_depth_at_capture: 0,
        prompt_stack: Vec::new(),
        exception_handlers: Vec::new(),
    }
}

/// A machine state whose registers are `registers` and whose frames are
/// `frames`, with no extent.
fn running(registers: Vec<TaggedValue>, frames: Vec<CallFrame>) -> ExecutionState {
    ExecutionState::from_parts(
        registers.into_iter().map(Cell::new).collect(),
        frames,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

/// (a) A unit whose only holder is a closure of it in a register: marking
/// the closure queues its code id, and the code-id round traces the unit's
/// table. The closure's globals are the machine's, never the namespace the
/// table was linked in, so only the table reaches `x` and `y`.
#[test]
fn a_closure_keeps_its_units_link_table() {
    let mut state = VmState::on_a_fresh_heap();
    let mut s = Sentinels::new(&mut state.heap.borrow_mut());
    let (x, y) = linked_values(&mut state.heap.borrow_mut(), &mut s);
    let id = load_linked(&mut state, super::tests::code(), &["x"], x, y);
    let globals = Rc::clone(state.global_env());
    let closure = (state.heap.borrow_mut()).alloc_vm_closure(id.0, Vec::new(), globals);
    state.execution = running(vec![closure], Vec::new());

    collect(&state);
    s.assert_survived(&state.heap.borrow());
}

/// (b) A unit whose only holder is a frame running it: the frame's code
/// reports its id (`FrameCode`).
#[test]
fn a_frame_keeps_its_units_link_table() {
    let mut state = VmState::on_a_fresh_heap();
    let mut s = Sentinels::new(&mut state.heap.borrow_mut());
    let (x, y) = linked_values(&mut state.heap.borrow_mut(), &mut s);
    let id = load_linked(&mut state, super::tests::code(), &["x"], x, y);
    let code = state.code_object(id).expect("loaded");
    state.execution = running(
        vec![TaggedValue::NULL],
        vec![frame(&code, CallFrame::NO_CLOSURE)],
    );
    drop(code);

    collect(&state);
    s.assert_survived(&state.heap.borrow());
}

/// (c) A unit whose only holder is a frame of a full continuation whose ref
/// is in a register: the frame reports its id when the weak-id round traces
/// the payload, so the code-id round that traces the table is a later one.
/// A unit is loaded first, so that this one's code id is not the
/// continuation ref's (both count from 0) and only the code-id path keeps
/// the table.
#[test]
fn a_full_continuations_frame_keeps_its_units_link_table() {
    let mut state = VmState::on_a_fresh_heap();
    let earlier = state.load_unit(super::tests::code(), Vec::new());
    let mut s = Sentinels::new(&mut state.heap.borrow_mut());
    let (x, y) = linked_values(&mut state.heap.borrow_mut(), &mut s);
    let id = load_linked(&mut state, super::tests::code(), &["x"], x, y);
    let code = state.code_object(id).expect("loaded");
    let continuation = state.alloc_vm_continuation(full_capture_of(&code));
    drop(code);
    let continuation_id = state.heap.borrow().get_vm_continuation_ref(continuation);
    assert_eq!(
        continuation_id,
        Some(earlier.0),
        "the colliding id is the other unit's"
    );
    state.execution = running(vec![continuation], Vec::new());

    collect(&state);
    assert_eq!(state.continuation_store.borrow().len(), 1);
    s.assert_survived(&state.heap.borrow());
}

/// (d) As (c), for a delimited continuation.
#[test]
fn a_delimited_continuations_frame_keeps_its_units_link_table() {
    let mut state = VmState::on_a_fresh_heap();
    let earlier = state.load_unit(super::tests::code(), Vec::new());
    let mut s = Sentinels::new(&mut state.heap.borrow_mut());
    let (x, y) = linked_values(&mut state.heap.borrow_mut(), &mut s);
    let id = load_linked(&mut state, super::tests::code(), &["x"], x, y);
    let code = state.code_object(id).expect("loaded");
    let continuation = state.alloc_vm_delimited_continuation(delimited_capture_of(&code));
    drop(code);
    let continuation_id = (state.heap.borrow()).get_vm_delimited_continuation_ref(continuation);
    assert_eq!(
        continuation_id,
        Some(earlier.0),
        "the colliding id is the other unit's"
    );
    state.execution = running(vec![continuation], Vec::new());

    collect(&state);
    assert_eq!(state.delimited_continuation_store.borrow().len(), 1);
    s.assert_survived(&state.heap.borrow());
}

/// (e) A unit that only the code store holds: the store traces its code's
/// constants and never its table (`CodeObject::links`), so a collection
/// frees what only the table and the namespace it holds reach. The test
/// reads nothing through the namespace afterwards; the table drops it with
/// the machine, which reads no heap word.
#[test]
fn the_code_store_keeps_no_units_link_table() {
    let mut state = VmState::on_a_fresh_heap();
    let (x, y) = {
        let mut heap = state.heap.borrow_mut();
        let x = heap.alloc_pair(TaggedValue::fixnum(0), TaggedValue::NULL);
        let y = heap.alloc_vector(vec![TaggedValue::fixnum(1)]);
        (x, y)
    };
    let id = load_linked(&mut state, super::tests::code(), &["x"], x, y);

    collect(&state);
    let heap = state.heap.borrow();
    assert_freed(&heap, x, "x, which only the unit's table links");
    assert_freed(&heap, y, "y, bound only in the namespace the table holds");
    drop(heap);
    assert!(
        state.code_object(id).is_ok(),
        "the store still holds the unit"
    );
}

/// (f) A placeholder the unit's table links, which only the table reaches:
/// a namespace's placeholder store is never traced, so the placeholder
/// lives because the frame running the unit reports its code id and the
/// table's entry marks it, and the sweep's pass keeps it. A dropped one
/// would give its record and its cell back from the heap's counts.
#[test]
fn a_frame_keeps_a_placeholder_its_units_table_links() {
    let mut state = VmState::on_a_fresh_heap();
    let mut s = Sentinels::new(&mut state.heap.borrow_mut());
    let (x, y) = linked_values(&mut state.heap.borrow_mut(), &mut s);
    let id = load_linked(&mut state, super::tests::code(), &["x", "later"], x, y);
    let code = state.code_object(id).expect("loaded");
    state.execution = running(
        vec![TaggedValue::NULL],
        vec![frame(&code, CallFrame::NO_CLOSURE)],
    );
    drop(code);
    let counts = |state: &VmState| {
        let heap = state.heap.borrow();
        (heap.binding_records(), heap.cells())
    };
    let before = counts(&state);

    collect(&state);
    s.assert_survived(&state.heap.borrow());
    assert_eq!(
        counts(&state),
        before,
        "later's placeholder, its record and cell"
    );
}
