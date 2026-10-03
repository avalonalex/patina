//! Sentinel tests for the tree-walker's root providers (#623).
//!
//! Each builds the record under test by a struct literal, so a new field
//! breaks the test as well as the trace that names it; puts a fresh value in
//! every field that can hold one; roots it through its provider and nothing
//! else; collects; and checks every value survived
//! (`patina_core::heap::sentinels`, which names the field of one that did
//! not). The collector is driven directly, through `collect_for_tests`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::heap::gc::collect_for_tests;
use patina_core::heap::sentinels::Sentinels;
use patina_core::{
    CpsContinuation, DynamicWindRecord, Environment, GcController, GcRoots, Heap, SharedHeap,
    TaggedValue,
};
use patina_runtime::{LibraryLoaderRegistry, LibraryRegistry};

use super::super::types::{
    ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult, set_pending_escape,
    take_pending_escape,
};
use super::{EscapeRoots, StepRoots};
use crate::eval::Evaluator;
use crate::eval::debug::DebugConfig;

fn literal(value: TaggedValue) -> Rc<CpsExpr> {
    Rc::new(CpsExpr::new(CpsExprKind::Literal(value)))
}

fn env_holding(heap: &SharedHeap, value: TaggedValue) -> Rc<Environment> {
    let env = Rc::new(Environment::with_heap(heap.clone()));
    env.define("held", value);
    env
}

/// A continuation environment whose one binding holds `value`.
fn cont_env_holding(value: TaggedValue) -> ContEnv {
    ContEnv::new().insert(
        Rc::from("k"),
        ContValue::ForceCache {
            promise: value,
            original_cont: Box::new(ContValue::Halt),
        },
    )
}

/// The three dynamic stacks every step carries, one value in each record.
struct Stacks {
    prompt_stack: Vec<PromptFrame>,
    dynamic_winds: Vec<DynamicWindRecord>,
    exception_handlers: Vec<ExceptionHandler>,
}

fn stacks(heap: &mut Heap, s: &mut Sentinels, step: &'static str) -> Stacks {
    let label = |field: &str| format!("{step}.{field}");
    Stacks {
        prompt_stack: vec![PromptFrame {
            id: 0,
            tag: None,
            handler: s.pair(heap, label("prompt_stack: PromptFrame.handler")),
            cont: ContValue::CallWithValuesConsumer {
                consumer: s.vector(heap, label("prompt_stack: PromptFrame.cont")),
                original_cont: Box::new(ContValue::Halt),
            },
            wind_depth: 0,
            trampoline: 0,
            handler_depth: 0,
        }],
        dynamic_winds: vec![DynamicWindRecord {
            id: 0,
            before: s.string(heap, label("dynamic_winds: WindRecord.before")),
            after: s.object(heap, label("dynamic_winds: WindRecord.after")),
            handlers: Rc::from(vec![ExceptionHandler {
                handler: s.pair(heap, label("dynamic_winds: WindRecord.handlers")),
            }]),
        }],
        exception_handlers: vec![ExceptionHandler {
            handler: s.vector(heap, label("exception_handlers")),
        }],
    }
}

/// Every `StepResult` variant, every field, through `StepRoots`, with the
/// entry expression's literals as well.
#[test]
fn every_step_result_field_is_traced() {
    let env = Rc::new(Environment::new());
    let shared = env.heap().clone();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h: &mut Heap = &mut heap;

    let entry = s.pair(h, "StepRoots.expr");
    let done = s.vector(h, "StepResult::Done");

    let continue_expr = s.string(h, "StepResult::Continue.expr");
    let continue_env = s.object(h, "StepResult::Continue.env");
    let continue_cont_env = s.pair(h, "StepResult::Continue.cont_env");
    let continue_stacks = stacks(h, &mut s, "StepResult::Continue");

    let invoke_cont = s.vector(h, "StepResult::InvokeContinuation.cont");
    let invoke_value = s.string(h, "StepResult::InvokeContinuation.value");
    let invoke_env = s.object(h, "StepResult::InvokeContinuation.env");
    let invoke_cont_env = s.pair(h, "StepResult::InvokeContinuation.cont_env");
    let invoke_stacks = stacks(h, &mut s, "StepResult::InvokeContinuation");

    let apply_proc = s.vector(h, "StepResult::ApplyProc.proc");
    let apply_arg = s.string(h, "StepResult::ApplyProc.args");
    let apply_cont = s.object(h, "StepResult::ApplyProc.cont");
    let apply_env = s.pair(h, "StepResult::ApplyProc.env");
    let apply_cont_env = s.vector(h, "StepResult::ApplyProc.cont_env");
    let apply_stacks = stacks(h, &mut s, "StepResult::ApplyProc");
    drop(heap);

    let entry_expr = literal(entry);
    let steps = [
        StepResult::Done(done),
        StepResult::Continue {
            expr: literal(continue_expr),
            env: env_holding(&shared, continue_env),
            cont_env: cont_env_holding(continue_cont_env),
            prompt_stack: continue_stacks.prompt_stack,
            dynamic_winds: continue_stacks.dynamic_winds,
            exception_handlers: continue_stacks.exception_handlers,
        },
        StepResult::InvokeContinuation {
            cont: ContValue::DynamicWindAfterDone {
                result_value: invoke_cont,
                original_cont: Box::new(ContValue::Halt),
            },
            value: invoke_value,
            env: env_holding(&shared, invoke_env),
            cont_env: cont_env_holding(invoke_cont_env),
            prompt_stack: invoke_stacks.prompt_stack,
            dynamic_winds: invoke_stacks.dynamic_winds,
            exception_handlers: invoke_stacks.exception_handlers,
        },
        StepResult::ApplyProc {
            proc: apply_proc,
            args: vec![apply_arg],
            cont: ContValue::DynamicWindAfterDone {
                result_value: apply_cont,
                original_cont: Box::new(ContValue::Halt),
            },
            env: env_holding(&shared, apply_env),
            cont_env: cont_env_holding(apply_cont_env),
            prompt_stack: apply_stacks.prompt_stack,
            dynamic_winds: apply_stacks.dynamic_winds,
            exception_handlers: apply_stacks.exception_handlers,
        },
    ];
    let roots: Vec<StepRoots<'_>> = steps
        .iter()
        .enumerate()
        .map(|(i, step)| StepRoots {
            step,
            expr: (i == 0).then_some(&*entry_expr),
        })
        .collect();
    let providers: Vec<&dyn GcRoots> = roots.iter().map(|r| r as &dyn GcRoots).collect();

    let mut heap = shared.borrow_mut();
    collect_for_tests(&mut heap, &providers);
    s.assert_survived(&heap);
}

/// The value and continuation parked between `set_pending_escape` and
/// `take_pending_escape`, through `EscapeRoots`.
#[test]
fn the_pending_escape_is_traced() {
    let env = Rc::new(Environment::new());
    let shared = env.heap().clone();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let value = s.pair(&mut heap, "PENDING_ESCAPE value");
    let held = s.vector(&mut heap, "PENDING_ESCAPE continuation");
    drop(heap);

    set_pending_escape(
        value,
        Rc::new(CpsContinuation {
            body: literal(TaggedValue::NULL),
            param: Rc::from("v"),
            env: env_holding(&shared, held),
            boundary: None,
            trampoline: 0,
            crosses_callback: false,
            dynamic_winds: vec![],
            prompt_stack: vec![],
            exception_handlers: vec![],
            captured_cont_env: ContEnv::new(),
            resume: None,
        }),
    );
    let mut heap = shared.borrow_mut();
    collect_for_tests(&mut heap, &[&EscapeRoots]);
    let parked = take_pending_escape();
    s.assert_survived(&heap);
    assert!(parked.is_some());
}

/// `Evaluator`, every field: its global environment is its one root.
#[test]
fn the_evaluator_roots_its_global_environment() {
    let global_env = Rc::new(Environment::new());
    let shared = global_env.heap().clone();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let global = s.pair(&mut heap, "Evaluator.global_env");
    let gc_pending = heap.gc_pending_handle();
    drop(heap);
    global_env.define("global", global);

    let evaluator = Evaluator {
        global_env,
        debug: Rc::new(DebugConfig::new()),
        library_registry: Rc::new(RefCell::new(LibraryRegistry::new())),
        loader_registry: Rc::new(RefCell::new(LibraryLoaderRegistry::new())),
        primitive_registry: patina_primitives::PrimitiveRegistry::new(),
        fs: Arc::new(patina_core::NativeFs),
        gc: RefCell::new(GcController::from_env()),
        gc_pending,
        bootstrap_error: None,
    };
    let mut heap = shared.borrow_mut();
    collect_for_tests(&mut heap, &[&evaluator]);
    s.assert_survived(&heap);
}
