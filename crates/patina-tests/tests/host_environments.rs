//! A host environment is rooted by its handle, on both backends (#620).
//!
//! An environment a host built with `Environment::with_parent` was reachable
//! from no root: what was bound in it was freed by the next collection that
//! ran outside it, a use after free in a debug build and another object in a
//! release one. `Interpreter::new_environment` answers an `OwnedEnvironment`,
//! a child of the global environment that the heap's handle table roots,
//! with everything bound in it, until the handle is dropped. The legacy
//! pipeline has the heap trace the environments its callers give it for as
//! long as they hold them.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter, Parser, TreeWalkInterpreter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// Defines `churn`, which allocates enough to collect on its own.
const PRELUDE: &str = "(import (scheme base) (patina debug))
    (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))";

/// Collects, by allocation and then by `(gc)`, outside any host environment,
/// and allocates into the slots that frees, so that a freed value reads as
/// another.
const CHURN: &str = "(churn 200000) (gc) (define keep (list 'a 'b 'c 'd))";

fn collections<B: Backend>(interp: &Interpreter<B>) -> u64 {
    interp.global_env().heap().borrow().gc_collections()
}

/// The message of the panic `f` raises.
fn panic_message(f: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(f)).expect_err("a panic");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .unwrap_or_default(),
    }
}

/// #620's first program: a value the host binds in its environment, which
/// nothing else holds, across a program that collects.
fn bound_across_a_collection<B: Backend>(interp: Interpreter<B>) -> String {
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    let value = interp
        .eval_str_owned("(list 'held (vector 1 2 3))")
        .unwrap();
    interp.define_in(&env, "held", value);
    let before = collections(&interp);
    interp.eval_program_owned(CHURN).unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    interp.display_tagged(interp.lookup_in(&env, "held").unwrap())
}

#[test]
fn a_value_bound_in_a_host_environment_survives_a_program_that_collects() {
    let expected = "(held #(1 2 3))";
    assert_eq!(bound_across_a_collection(vm_interpreter()), expected);
    assert_eq!(
        bound_across_a_collection(tree_walker_interpreter()),
        expected
    );
}

/// #620's second program on the tree-walker, which evaluates in the
/// environment `Backend::eval` is given: a definition the program makes in
/// the host environment, across a program that collects outside it.
#[test]
fn a_definition_made_in_a_host_environment_survives_a_program_that_collects() {
    let interp: TreeWalkInterpreter = tree_walker_interpreter();
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    let datum = Parser::new_with_heap(
        "(define kept (list 'kept (vector 4 5 6)))",
        interp.global_env().heap().clone(),
    )
    .unwrap()
    .parse()
    .unwrap();
    interp
        .backend()
        .eval(datum, &interp.raw_environment(&env))
        .unwrap();
    assert!(
        interp.lookup("kept").is_none(),
        "defined in the global environment"
    );
    let before = collections(&interp);
    interp.eval_program_owned(CHURN).unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    assert_eq!(
        interp.display_tagged(interp.lookup_in(&env, "kept").unwrap()),
        "(kept #(4 5 6))"
    );
}

/// What `Environment::with_parent` makes: definitions stay in the child,
/// and every other name resolves in the global environment, a global defined
/// after the child included.
fn a_child_of_the_global_environment<B: Backend>(interp: Interpreter<B>) {
    let env = interp.new_environment();
    interp.eval_program_owned("(define later 'global)").unwrap();
    let later =
        |interp: &Interpreter<B>| interp.display_tagged(interp.lookup_in(&env, "later").unwrap());
    assert_eq!(later(&interp), "global");
    let local = interp.eval_str_owned("'local").unwrap();
    interp.define_in(&env, "later", &local);
    assert_eq!(later(&interp), "local");
    assert_eq!(
        interp.display_tagged(interp.lookup("later").unwrap()),
        "global"
    );
    assert!(interp.lookup_in(&env, "no-such-name").is_none());
}

#[test]
fn a_host_environment_is_a_child_of_the_global_environment() {
    a_child_of_the_global_environment(vm_interpreter());
    a_child_of_the_global_environment(tree_walker_interpreter());
}

/// Once the handle is dropped, a value bound only in its environment is
/// freed by the next collection: a check build reports a read of it as a
/// use after free.
fn reclaimed_once_dropped<B: Backend>(interp: Interpreter<B>) {
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    let value = interp.eval_str_owned("(list 'only-here 1 2)").unwrap();
    let raw = interp.raw_value(&value);
    interp.define_in(&env, "only", value);
    interp.eval_program_owned("(gc)").unwrap();
    assert_eq!(interp.display_tagged(raw), "(only-here 1 2)");
    let heap = interp.global_env().heap().clone();
    drop(env);
    interp.eval_program_owned("(gc)").unwrap();
    let message = panic_message(|| {
        heap.borrow().get_pair(raw);
    });
    assert!(message.contains("use-after-free"), "{message}");
}

#[test]
fn a_value_bound_only_in_a_dropped_environment_is_reclaimed() {
    if !patina_core::heap::GC_CHECK {
        return;
    }
    reclaimed_once_dropped(vm_interpreter());
    reclaimed_once_dropped(tree_walker_interpreter());
}

#[test]
fn an_environment_handle_used_with_another_interpreter_is_refused() {
    let vm = vm_interpreter();
    let tree_walker = tree_walker_interpreter();
    let env = vm.new_environment();
    let message = panic_message(|| {
        tree_walker.lookup_in(&env, "car");
    });
    assert!(message.contains("used with heap"), "{message}");
    let foreign = tree_walker.eval_str_owned("'foreign").unwrap();
    let message = panic_message(|| vm.define_in(&env, "foreign", &foreign));
    assert!(message.contains("used with heap"), "{message}");
    // Refused, not spoiled: the interpreter that made it still uses it.
    assert!(vm.lookup_in(&env, "car").is_some());
}

/// The table holds the environment, which holds the heap, which holds the
/// table: teardown breaks that cycle, so a handle kept past its interpreter
/// keeps nothing alive.
fn kept_past_its_interpreter<B: Backend>(interp: Interpreter<B>) {
    let env = interp.new_environment();
    let value = interp.eval_str_owned("(list 'held 1 2)").unwrap();
    interp.define_in(&env, "held", value);
    let copy = env.clone();
    let heap = Rc::downgrade(interp.global_env().heap());
    drop(interp);
    assert!(
        heap.upgrade().is_none(),
        "the environment kept the heap alive"
    );
    drop(env);
    let again = copy.clone();
    drop(copy);
    drop(again);
}

#[test]
fn an_environment_handle_kept_past_its_interpreter_holds_nothing() {
    kept_past_its_interpreter(vm_interpreter());
    kept_past_its_interpreter(tree_walker_interpreter());
}

/// The legacy pipeline evaluates in an environment its caller holds by its
/// own `Rc`; the heap traces it for as long as the caller holds it.
#[test]
#[allow(deprecated)]
fn the_legacy_pipeline_keeps_the_environments_it_is_given() {
    use patina_interpreter::{Environment, Pipeline, StandardPipeline};
    let pipeline = StandardPipeline::new();
    let global = pipeline.evaluator().global_env.clone();
    let child = Rc::new(Environment::with_parent(global.clone()));
    pipeline
        .eval_program("(define host-local (list 'host (vector 7 8)))", &child)
        .unwrap();
    let heap = global.heap().clone();
    let before = heap.borrow().gc_collections();
    pipeline
        .eval_program(&format!("{PRELUDE} {CHURN}"), &global)
        .unwrap();
    assert!(
        heap.borrow().gc_collections() > before,
        "the program did not collect"
    );
    let value = pipeline.eval("host-local", &child).unwrap();
    let display = patina_core::format_tagged(value, &heap.borrow());
    assert_eq!(display, "(host #(7 8))");
}
