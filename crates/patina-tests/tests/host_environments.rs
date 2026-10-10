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
//!
//! The VM evaluated in the global environment whatever environment
//! `Backend::eval` was given; it now evaluates where the tree-walker does,
//! running the code as a closure whose globals are the environment, and
//! `eval_str_in` and `eval_program_in` evaluate in a handle's environment on
//! both backends.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter, Parser, SourceMap, TaggedValue};
use std::cell::RefCell;
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

/// #620's second program: a definition the program makes in the host
/// environment through `Backend::eval`, across a program that collects
/// outside it.
fn defined_across_a_collection<B: Backend>(interp: Interpreter<B>) {
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

#[test]
fn a_definition_made_in_a_host_environment_survives_a_program_that_collects() {
    defined_across_a_collection(vm_interpreter());
    defined_across_a_collection(tree_walker_interpreter());
}

/// #620's last table: through `Backend::eval`, or `eval_with_source_map`, a
/// datum evaluated in a host environment reads its bindings, closes over
/// them, and defines in it alone, on both backends.
fn evaluated_in_the_environment<B: Backend>(
    interp: Interpreter<B>,
    with_source_map: bool,
) -> [String; 4] {
    let env = interp.new_environment();
    let raw = interp.raw_environment(&env);
    interp.define_in(&env, "x", TaggedValue::fixnum(99));
    interp.eval_program_owned("(define x 1)").unwrap();
    let heap = interp.global_env().heap().clone();
    let source_map = Rc::new(RefCell::new(SourceMap::new()));
    let eval = |source: &str| {
        let datum = Parser::new_with_heap(source, heap.clone())
            .unwrap()
            .parse()
            .unwrap();
        let backend = interp.backend();
        let value = if with_source_map {
            backend.eval_with_source_map(datum, &raw, &source_map)
        } else {
            backend.eval(datum, &raw)
        };
        value.unwrap()
    };
    let x = interp.display_tagged(eval("x"));
    let closure = eval("(lambda () x)");
    interp.global_env().define("f", closure);
    let called = interp.display_tagged(interp.eval_str_owned("(f)").unwrap());
    eval("(define y 42)");
    let in_env = interp.display_tagged(interp.lookup_in(&env, "y").unwrap());
    let global = format!("{:?}", interp.lookup("y").map(|y| interp.display_tagged(y)));
    [x, called, in_env, global]
}

#[test]
fn backend_eval_evaluates_in_the_environment_it_is_given() {
    let expected = ["99", "99", "42", "None"];
    for with_source_map in [false, true] {
        assert_eq!(
            evaluated_in_the_environment(vm_interpreter(), with_source_map),
            expected,
            "VM, with a source map: {with_source_map}"
        );
        assert_eq!(
            evaluated_in_the_environment(tree_walker_interpreter(), with_source_map),
            expected,
            "tree-walker, with a source map: {with_source_map}"
        );
    }
}

/// The handle forms evaluate in a host environment: its definitions and
/// imports stay in it, a collection while its code runs keeps what it
/// defines, an error leaves it usable, and a continuation escapes from it.
fn evaluated_by_the_handle_forms<B: Backend>(interp: Interpreter<B>) -> Vec<String> {
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    let show = |value| interp.display_tagged(value);
    let before = collections(&interp);
    let kept = interp
        .eval_program_in(
            &env,
            &format!("(define kept (list 'kept (vector 4 5 6))) {CHURN} kept"),
        )
        .unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    let imported = interp
        .eval_program_in(&env, "(import (srfi 1)) (iota 3)")
        .unwrap();
    let failed = interp.eval_str_in(&env, "(car 5)").is_err();
    let escaped = interp
        .eval_str_in(&env, "(call/cc (lambda (k) (+ 1 (k 5))))")
        .unwrap();
    vec![
        show(kept),
        show(imported),
        format!("{failed}"),
        show(escaped),
        format!("{}", interp.lookup("kept").is_none()),
        format!("{}", interp.lookup("iota").is_none()),
        format!("{}", interp.lookup_in(&env, "iota").is_some()),
    ]
}

#[test]
fn the_handle_forms_evaluate_in_the_host_environment() {
    let expected = [
        "(kept #(4 5 6))",
        "(0 1 2)",
        "true",
        "5",
        "true",
        "true",
        "true",
    ];
    assert_eq!(evaluated_by_the_handle_forms(vm_interpreter()), expected);
    assert_eq!(
        evaluated_by_the_handle_forms(tree_walker_interpreter()),
        expected
    );
}

/// A program that redefines a primitive's name in a host environment
/// reaches the code compiled there, and leaves the global environment's
/// binding as it was.
fn primitive_redefined_in_the_environment<B: Backend>(interp: Interpreter<B>) -> [String; 2] {
    let env = interp.new_environment();
    let local = interp
        .eval_program_in(
            &env,
            "(define (f x) (car x)) (define car (lambda (x) 'replaced)) (f '(1 2))",
        )
        .unwrap();
    let global = interp.eval_str_owned("(car '(1 2))").unwrap();
    [interp.display_tagged(local), interp.display_tagged(global)]
}

#[test]
fn a_primitive_redefined_in_a_host_environment_stays_in_it() {
    let expected = ["replaced", "1"];
    assert_eq!(
        primitive_redefined_in_the_environment(vm_interpreter()),
        expected
    );
    assert_eq!(
        primitive_redefined_in_the_environment(tree_walker_interpreter()),
        expected
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

/// A macro the global environment defined, whose expansions reference a
/// definition its own earlier expansion introduced (`count`, under that
/// expansion's scopes): used in a host environment, it still reaches that
/// definition, which the global environment holds, and a host binding of the
/// same spelling does not capture it.
fn introduced_global_reached_from_the_environment<B: Backend>(
    interp: Interpreter<B>,
) -> [String; 4] {
    interp
        .eval_program_owned(
            "(import (scheme base))
             (define-syntax def-counter
               (syntax-rules ()
                 ((_ next)
                  (begin
                    (define count 0)
                    (define-syntax next
                      (syntax-rules ()
                        ((_) (begin (set! count (+ count 1)) count))))))))
             (def-counter next!)",
        )
        .unwrap();
    let env = interp.new_environment();
    let first = interp.eval_str_in(&env, "(next!)").unwrap();
    interp.eval_program_in(&env, "(define count 100)").unwrap();
    let second = interp.eval_str_in(&env, "(next!)").unwrap();
    let global = interp.eval_str_owned("(next!)").unwrap();
    let host_count = interp.lookup_in(&env, "count").unwrap();
    [first, second, global, host_count].map(|value| interp.display_tagged(value))
}

#[test]
fn a_host_environment_reaches_what_a_global_expansion_introduced() {
    let expected = ["1", "2", "3", "100"];
    assert_eq!(
        introduced_global_reached_from_the_environment(vm_interpreter()),
        expected
    );
    assert_eq!(
        introduced_global_reached_from_the_environment(tree_walker_interpreter()),
        expected
    );
}
