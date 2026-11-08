//! A host environment is rooted by its handle, on both backends (#620).
//!
//! An environment a host built with `Environment::with_parent`, a frame of
//! the global environment, was reachable from no root: what was bound in it
//! was freed by the next collection that ran outside it, a use after free in
//! a debug build and another object in a release one.
//! `Interpreter::new_environment` answers an `OwnedEnvironment` holding a
//! host namespace, whose parent is the global namespace (#835), which the
//! heap's handle table roots, with everything bound in it, until the handle
//! is dropped. The legacy pipeline has the heap trace the environments its
//! callers give it, frames built with `with_parent`, for as long as they
//! hold them.
//!
//! The VM evaluated in the global environment whatever environment
//! `Backend::eval` was given; it now evaluates where the tree-walker does,
//! running the code as a closure whose globals are the environment, and
//! `eval_str_in` and `eval_program_in` evaluate in a handle's namespace on
//! both backends. The VM evaluates only in a namespace: a raw frame, or a
//! frame under a host namespace, it refuses with an error naming the kind,
//! where the tree-walker evaluates one by name (#835).

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
    interp.gc_stats().collections
}

/// One datum of `source`, read on `heap` as the word `Backend::eval` takes.
fn read(source: &str, heap: &patina_core::SharedHeap) -> TaggedValue {
    let mut parser = Parser::new(source).unwrap();
    patina_core::NoGcScope::with_cx(heap, |cx| parser.parse(cx).unwrap().tagged())
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
    let datum = read(
        "(define kept (list 'kept (vector 4 5 6)))",
        interp.backend().heap(),
    );
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
    let heap = interp.backend().heap().clone();
    let source_map = Rc::new(RefCell::new(SourceMap::new()));
    let eval = |source: &str| {
        let datum = read(source, &heap);
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
    interp
        .global_env()
        .define(interp.backend().heap(), "f", closure);
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

/// What a host namespace means, which is what `Environment::with_parent`'s
/// frame meant: definitions stay in the host namespace, and every other name
/// resolves in the global namespace, a global defined after the host
/// namespace included.
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

/// A host namespace is charged as a parentless namespace is (the owner's
/// ruling (a) of 2026-11-07 on #835's question 2): counted among the
/// heap's namespaces while it lives, charging the heap's external bytes for
/// its tables as they grow, and giving both back when its handle drops. No
/// closure was made in it, so nothing but the handle holds it.
fn charged_as_a_namespace<B: Backend>(interp: Interpreter<B>) {
    interp
        .eval_program_owned("(import (scheme base) (patina debug)) (gc)")
        .unwrap();
    let heap = interp.backend().heap().clone();
    let namespaces = heap.borrow().namespaces();
    let env = interp.new_environment();
    assert_eq!(heap.borrow().namespaces(), namespaces + 1);
    let raw = interp.raw_environment(&env);
    let made = raw.charged_bytes().expect("a host namespace charges");
    let value = interp.eval_str_owned("'bound").unwrap();
    for i in 0..100 {
        interp.define_in(&env, &format!("name-{i}"), &value);
    }
    assert_eq!(raw.local_names(&heap).len(), 100);
    let charged = raw.charged_bytes().unwrap();
    assert!(
        charged > made,
        "{charged} charged after 100 names, {made} made"
    );
    let before = interp.gc_stats().external_bytes;
    drop(raw);
    drop(env);
    assert_eq!(
        interp.gc_stats().external_bytes,
        before - charged,
        "its charge given back when its handle dropped"
    );
    assert_eq!(heap.borrow().namespaces(), namespaces);
    interp.eval_program_owned("(gc)").unwrap();
    assert_eq!(heap.borrow().namespaces(), namespaces);
}

#[test]
fn a_host_namespace_is_charged_as_a_namespace() {
    charged_as_a_namespace(vm_interpreter());
    charged_as_a_namespace(tree_walker_interpreter());
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
    let heap = interp.backend().heap().clone();
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

/// A handle kept past its interpreter holds nothing: teardown drops the
/// table that held the host namespace, and the namespace holds no handle to
/// its heap, so the handle keeps neither the heap nor its interpreter alive
/// (#704).
fn kept_past_its_interpreter<B: Backend>(interp: Interpreter<B>) {
    let env = interp.new_environment();
    let value = interp.eval_str_owned("(list 'held 1 2)").unwrap();
    interp.define_in(&env, "held", value);
    let copy = env.clone();
    let heap = interp.heap_probe();
    assert!(
        !heap.is_gone(),
        "the heap is alive while its interpreter is"
    );
    drop(interp);
    assert!(heap.is_gone(), "the environment kept the heap alive");
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
    let heap = pipeline.evaluator().heap.clone();
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

// ---------------------------------------------------------------------------
// What host code reads and writes, what its closures keep, and what its
// macros introduce, pinned before 4b.6's binding records (#834) changed how
// it is linked (#835's tests PR). Each held at the PR's base and holds under
// the records and under 4b.8's host namespaces (#835).
// ---------------------------------------------------------------------------

/// A `set!` in host code of a name only the global environment binds writes
/// the global binding, and binds nothing in the host environment; a `set!` of
/// a name neither binds is an error.
fn set_writes_the_global_binding<B: Backend>(interp: Interpreter<B>) {
    interp.eval_program_owned("(define g 1)").unwrap();
    let env = interp.new_environment();
    interp
        .eval_program_in(&env, "(define (bump!) (set! g (+ g 1))) (bump!) (bump!)")
        .unwrap();
    assert_eq!(interp.display_tagged(interp.lookup("g").unwrap()), "3");
    let own = interp
        .raw_environment(&env)
        .local_names(interp.backend().heap());
    assert!(
        !own.iter().any(|name| &**name == "g"),
        "the host environment bound `g` of its own: {own:?}"
    );
    assert!(
        interp.eval_program_in(&env, "(set! nowhere 1)").is_err(),
        "a `set!` of a name neither environment binds answered"
    );
}

#[test]
fn a_set_in_a_host_namespace_writes_the_global_binding() {
    set_writes_the_global_binding(vm_interpreter());
    set_writes_the_global_binding(tree_walker_interpreter());
}

/// `(host4b lib)`, defined in the program, exporting `shared`.
const SHARED_LIBRARY: &str = "(import (scheme base) (scheme eval))
    (define-library (host4b lib)
      (export shared)
      (import (scheme base))
      (begin (define shared 'library)))
    (import (host4b lib))";

/// Host code that reads a name the global environment imported reads the
/// global definition that later replaces the import; the library's own
/// binding keeps its value.
fn global_definition_over_an_import<B: Backend>(interp: Interpreter<B>) -> [String; 3] {
    interp.eval_program_owned(SHARED_LIBRARY).unwrap();
    let env = interp.new_environment();
    let before = interp
        .eval_program_in(&env, "(define (f) shared) (f)")
        .unwrap();
    interp
        .eval_program_owned("(define shared 'global)")
        .unwrap();
    let after = interp.eval_str_in(&env, "(f)").unwrap();
    let library = interp
        .eval_str_owned("(eval 'shared (environment '(host4b lib)))")
        .unwrap();
    [before, after, library].map(|value| interp.display_tagged(value))
}

#[test]
fn host_code_sees_a_global_definition_over_an_import() {
    let expected = ["library", "global", "library"];
    assert_eq!(global_definition_over_an_import(vm_interpreter()), expected);
    assert_eq!(
        global_definition_over_an_import(tree_walker_interpreter()),
        expected
    );
}

/// `(host4b late)` exports `late`; `(host4b over)` exports `over`.
const LATE_LIBRARIES: &str = "(import (scheme base))
    (define-library (host4b late)
      (export late)
      (import (scheme base))
      (begin (define late 'imported)))
    (define-library (host4b over)
      (export over)
      (import (scheme base))
      (begin (define over 'imported)))
    (define over 'defined)";

/// Host code that reads a name the global environment does not bind yet
/// sees it once the global environment imports it; and host code that read a
/// global definition sees the import that later replaces it.
fn global_import<B: Backend>(interp: Interpreter<B>) -> [String; 4] {
    interp.eval_program_owned(LATE_LIBRARIES).unwrap();
    let env = interp.new_environment();
    let unbound = interp
        .eval_program_in(
            &env,
            "(define (read-late) (guard (e (#t 'unbound)) late))
             (define (read-over) over)
             (read-late)",
        )
        .unwrap();
    let defined = interp.eval_str_in(&env, "(read-over)").unwrap();
    interp
        .eval_program_owned("(import (host4b late) (host4b over))")
        .unwrap();
    let late = interp.eval_str_in(&env, "(read-late)").unwrap();
    let over = interp.eval_str_in(&env, "(read-over)").unwrap();
    [unbound, late, defined, over].map(|value| interp.display_tagged(value))
}

#[test]
fn host_code_sees_a_global_import() {
    let expected = ["unbound", "imported", "defined", "imported"];
    assert_eq!(global_import(vm_interpreter()), expected);
    assert_eq!(global_import(tree_walker_interpreter()), expected);
}

/// How the host environment comes to define `later`.
#[derive(Clone, Copy, Debug)]
enum HostDefinition {
    /// A program's `define`, by `eval_program_in`.
    Program,
    /// The host's own binding, by `define_in`.
    Host,
}

/// Host code compiled while only the global environment binds `later` reads
/// the host environment's definition once it is made, and the global binding
/// is left as it was.
fn host_definition_after_it_ran<B: Backend>(
    interp: Interpreter<B>,
    how: HostDefinition,
) -> [String; 3] {
    interp.eval_program_owned("(define later 'global)").unwrap();
    let env = interp.new_environment();
    let before = interp
        .eval_program_in(&env, "(define (read-later) later) (read-later)")
        .unwrap();
    match how {
        HostDefinition::Program => {
            interp
                .eval_program_in(&env, "(define later 'local)")
                .unwrap();
        }
        HostDefinition::Host => {
            let local = interp.eval_str_owned("'local").unwrap();
            interp.define_in(&env, "later", &local);
        }
    }
    let after = interp.eval_str_in(&env, "(read-later)").unwrap();
    let global = interp.lookup("later").unwrap();
    [
        interp.display_tagged(before),
        interp.display_tagged(after),
        interp.display_tagged(global),
    ]
}

#[test]
fn host_code_sees_a_host_definition_made_after_it_ran() {
    let expected = ["global", "local", "global"];
    for how in [HostDefinition::Program, HostDefinition::Host] {
        assert_eq!(
            host_definition_after_it_ran(vm_interpreter(), how),
            expected,
            "vm, {how:?}"
        );
        assert_eq!(
            host_definition_after_it_ran(tree_walker_interpreter(), how),
            expected,
            "tree-walker, {how:?}"
        );
    }
}

/// A closure host code made, held by the host, outlives the environment's
/// handle and a collection, and still reads what the environment bound.
fn closure_survives_its_handle<B: Backend>(mut interp: Interpreter<B>) -> String {
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    let closure = interp
        .eval_program_in(&env, "(define x (list 'kept 1)) (lambda () x)")
        .unwrap();
    drop(env);
    let before = collections(&interp);
    interp.eval_program_owned(CHURN).unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    let value = interp.call(&closure, &[]).unwrap();
    interp.display_tagged(&value)
}

#[test]
fn a_closure_made_in_a_host_namespace_survives_its_handle_and_a_collection() {
    assert_eq!(closure_survives_its_handle(vm_interpreter()), "(kept 1)");
    assert_eq!(
        closure_survives_its_handle(tree_walker_interpreter()),
        "(kept 1)"
    );
}

/// A procedure defined in a host environment, which nothing else holds, is
/// freed by the collection after the handle is dropped: a check build reports
/// a read of its word as a use after free.
fn procedure_freed_with_its_environment<B: Backend>(interp: Interpreter<B>) {
    interp.eval_program_owned(PRELUDE).unwrap();
    let env = interp.new_environment();
    interp.eval_program_in(&env, "(define (f) f)").unwrap();
    let f = interp.lookup_in(&env, "f").unwrap();
    let raw = interp.raw_value(&f);
    drop(f);
    interp.eval_program_owned("(gc)").unwrap();
    assert!(
        interp.backend().heap().borrow().is_procedure(raw),
        "`f` is not a procedure while its environment is held"
    );
    drop(env);
    interp.eval_program_owned("(gc)").unwrap();
    let heap = interp.backend().heap().clone();
    let message = panic_message(|| {
        heap.borrow().get_object(raw);
    });
    assert!(message.contains("use-after-free"), "{message}");
}

/// On the VM, the code a dropped host environment's procedure ran is let go
/// with it: as many code objects are loaded after 100 such environments, each
/// dropped and followed by a collection, as after 10.
fn code_let_go_with_its_environments(
    interp: &Interpreter<patina_vm::VmBackend>,
    n: usize,
) -> usize {
    for _ in 0..n {
        let env = interp.new_environment();
        interp.eval_program_in(&env, "(define (f) f) (f)").unwrap();
        drop(env);
        interp.eval_program_owned("(gc)").unwrap();
    }
    interp.backend().loaded_code_objects()
}

#[test]
fn a_procedure_defined_in_a_dropped_host_namespace_is_freed_with_its_unit() {
    if patina_core::heap::GC_CHECK {
        procedure_freed_with_its_environment(vm_interpreter());
        procedure_freed_with_its_environment(tree_walker_interpreter());
    }
    let interp = vm_interpreter();
    interp.eval_program_owned(PRELUDE).unwrap();
    let after_ten = code_let_go_with_its_environments(&interp, 10);
    let after_hundred = code_let_go_with_its_environments(&interp, 90);
    assert_eq!(
        after_hundred, after_ten,
        "code objects loaded after 10 dropped host environments, and after 100"
    );
}

/// `def-counter`, defined, expanded and used in the host environment: its
/// expansion introduces `count`, which the `next!` it defines reaches, from
/// the environment itself and from a body's frame inside it. A use from such
/// a frame is what the desugarer resolves through the definition
/// environment's introduced definitions (`has_introduced_definition`,
/// `introduced_definition`), which read only the chain's root, the global
/// environment; a use in the host environment itself never reaches them.
/// Nothing of the expansion is filed in the global environment: no `count` or
/// `next!` by name, no introduced `count` (the VM's identity, the
/// tree-walker's scoped binding), and no renamed `count` global.
fn macro_introduced_in_the_environment<B: Backend>(interp: Interpreter<B>) -> Vec<String> {
    interp.eval_program_owned("(import (scheme base))").unwrap();
    let env = interp.new_environment();
    interp
        .eval_program_in(
            &env,
            "(define-syntax def-counter
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
    let mut answers: Vec<String> = ["(next!)", "(next!)", "(let () (define unused 0) (next!))"]
        .iter()
        .map(|form| match interp.eval_str_in(&env, form) {
            Ok(value) => interp.display_tagged(value),
            Err(e) => format!("error: {e}"),
        })
        .collect();
    for name in ["count", "next!"] {
        answers.push(format!("global {name}: {}", interp.lookup(name).is_some()));
    }
    let global = interp.global_env();
    answers.push(format!(
        "global introduced count: {}",
        global.has_introduced_definition("count")
    ));
    answers.push(format!(
        "global renamed count: {}",
        global
            .local_names(interp.backend().heap())
            .iter()
            .any(|name| name.starts_with("count "))
    ));
    answers
}

#[test]
fn a_definition_a_macro_introduces_in_a_host_namespace() {
    let expected = [
        "1",
        "2",
        "3",
        "global count: false",
        "global next!: false",
        "global introduced count: false",
        "global renamed count: false",
    ];
    assert_eq!(
        macro_introduced_in_the_environment(vm_interpreter()),
        expected
    );
    assert_eq!(
        macro_introduced_in_the_environment(tree_walker_interpreter()),
        expected
    );
}

/// `(host4b sum)` exports `sum-list`, a macro whose template calls the
/// `fold` the library imports from `(srfi 1)`. The global environment
/// imports `(scheme base)` and nothing of `(srfi 1)`.
const SUM_LIBRARY: &str = "(import (scheme base))
    (define-library (host4b sum)
      (export sum-list)
      (import (scheme base) (srfi 1))
      (begin (define-syntax sum-list (syntax-rules () ((_ l) (fold + 0 l))))))";

/// The program each environment runs after its imports: `sum-list`'s
/// `fold` is the library's, whatever the use site defines afterwards.
const SUM_PROGRAM: &str = "(define (f) (sum-list '(1 2 3))) (define fold 'captured) (f)";

/// A library macro used in a host namespace that imports the binding its
/// template names keeps that binding past a later host definition of the
/// name, as the same program does at top level (#851). The desugarer binds
/// the template's `fold` early to an import alias of the nearest namespace
/// that imports it, the host namespace, before asking the global namespace,
/// which here imports nothing of `(srfi 1)` (the owner's ruling (c) of
/// 2026-11-07 on #835's question 3). The host's own `fold` is its
/// definition all the same.
fn library_macro_keeps_its_import<B: Backend>(interp: Interpreter<B>) -> [String; 3] {
    interp.eval_program_owned(SUM_LIBRARY).unwrap();
    let shown = |result: Result<_, _>| match result {
        Ok(value) => interp.display_tagged(&value),
        Err(e) => format!("error: {e}"),
    };
    let env = interp.new_environment();
    let host = shown(interp.eval_program_in(
        &env,
        &format!("(import (scheme base) (srfi 1) (host4b sum)) {SUM_PROGRAM}"),
    ));
    let host_fold = shown(interp.eval_str_in(&env, "fold"));
    let global =
        shown(interp.eval_program_owned(&format!("(import (srfi 1) (host4b sum)) {SUM_PROGRAM}")));
    [host, host_fold, global]
}

#[test]
fn a_library_macro_in_a_host_namespace_keeps_its_import() {
    let expected = ["6", "captured", "6"];
    assert_eq!(library_macro_keeps_its_import(vm_interpreter()), expected);
    assert_eq!(
        library_macro_keeps_its_import(tree_walker_interpreter()),
        expected
    );
}

/// The same program in a host namespace that imports nothing, after the
/// global namespace imports `(srfi 1)` and `(host4b sum)`: the host
/// namespace holds no import of `fold`'s location, so the desugarer asks the
/// global namespace next, whose import alias keeps the template's `fold`
/// the library's, as before the host-namespace kind (the owner's ruling
/// (c) of 2026-11-07 on #835's question 3). The host's `fold` is its own
/// definition, and the global's is still `(srfi 1)`'s.
fn library_macro_keeps_the_global_import<B: Backend>(interp: Interpreter<B>) -> [String; 3] {
    interp.eval_program_owned(SUM_LIBRARY).unwrap();
    interp
        .eval_program_owned("(import (srfi 1) (host4b sum))")
        .unwrap();
    let shown = |result: Result<_, _>| match result {
        Ok(value) => interp.display_tagged(&value),
        Err(e) => format!("error: {e}"),
    };
    let env = interp.new_environment();
    let host = shown(interp.eval_program_in(&env, SUM_PROGRAM));
    let host_fold = shown(interp.eval_str_in(&env, "fold"));
    let global_fold = shown(interp.eval_str_owned("(procedure? fold)"));
    [host, host_fold, global_fold]
}

#[test]
fn a_library_macro_in_a_host_namespace_falls_back_to_the_global_import() {
    let expected = ["6", "captured", "#t"];
    assert_eq!(
        library_macro_keeps_the_global_import(vm_interpreter()),
        expected
    );
    assert_eq!(
        library_macro_keeps_the_global_import(tree_walker_interpreter()),
        expected
    );
}

/// `(host4b cell)` exports `value` and a procedure that assigns it.
const CELL_LIBRARY: &str = "(import (scheme base) (scheme eval))
    (define-library (host4b cell)
      (export value set-value!)
      (import (scheme base))
      (begin (define value 'first)
             (define (set-value! v) (set! value v))))";

/// An import into a host environment shares the library's location (#834):
/// the library's later assignment is seen there, and a definition there
/// gives the environment a binding of its own, leaving the library's alone.
fn import_shares_the_location<B: Backend>(interp: Interpreter<B>) -> [String; 5] {
    interp.eval_program_owned(CELL_LIBRARY).unwrap();
    let env = interp.new_environment();
    let first = interp
        .eval_program_in(&env, "(import (host4b cell)) value")
        .unwrap();
    let assigned = interp
        .eval_program_in(&env, "(set-value! 'second) value")
        .unwrap();
    let own = interp
        .eval_program_in(&env, "(define value 'host) (set-value! 'third) value")
        .unwrap();
    let library = interp
        .eval_str_owned("(eval 'value (environment '(host4b cell)))")
        .unwrap();
    let global = format!("global value: {}", interp.lookup("value").is_some());
    let [first, assigned, own, library] =
        [first, assigned, own, library].map(|value| interp.display_tagged(value));
    [first, assigned, own, library, global]
}

#[test]
fn an_import_into_a_host_environment_shares_the_librarys_location() {
    let expected = ["first", "second", "host", "third", "global value: false"];
    assert_eq!(import_shares_the_location(vm_interpreter()), expected);
    assert_eq!(
        import_shares_the_location(tree_walker_interpreter()),
        expected
    );
}

/// `(host4b reads)` exports `imported`, which the program imports, and a
/// host environment imports again under another name.
const READS_LIBRARY: &str = "(import (scheme base))
    (define-library (host4b reads)
      (export imported)
      (import (scheme base))
      (begin (define imported 834)))
    (import (host4b reads))
    (define own 13)";

/// What the host reads of an own binding, an import, and an import into the
/// host namespace (#834, critique D2): the value each binding's record leads
/// to, never the record or the cell itself.
fn host_reads<B: Backend>(interp: Interpreter<B>) -> Vec<String> {
    interp.eval_program_owned(READS_LIBRARY).unwrap();
    let env = interp.new_environment();
    interp
        .eval_program_in(
            &env,
            "(import (rename (host4b reads) (imported host-imported)))",
        )
        .unwrap();
    let read = |value: Option<patina_interpreter::Owned>| {
        value.map_or_else(
            || "unbound".to_string(),
            |value| interp.display_tagged(&value),
        )
    };
    vec![
        read(interp.lookup("own")),
        read(interp.lookup("imported")),
        read(interp.lookup_in(&env, "own")),
        read(interp.lookup_in(&env, "imported")),
        read(interp.lookup_in(&env, "host-imported")),
        read(interp.lookup("host-imported")),
    ]
}

#[test]
fn no_namespace_read_answers_a_record() {
    let expected = ["13", "834", "13", "834", "834", "unbound"];
    assert_eq!(host_reads(vm_interpreter()), expected);
    assert_eq!(host_reads(tree_walker_interpreter()), expected);
}

/// The binding records the heap counts, read by a form that calls only
/// procedures, so that expanding it mints no import alias.
const RECORDS: &str = "(cdr (assq 'binding-records (gc-stats)))";

/// A read of a name nothing binds makes no binding record (#834, critique
/// E3): a bare reference the backend misses, a reference from compiled code
/// that misses when it runs, and a host's lookup. Nor does a host's lookup
/// of a name only the global namespace binds, which reads through the host
/// namespace by name and makes no read-through record (#835). No macro is
/// expanded and nothing is defined between the readings, and every unbound
/// name is fresh, so a read that made a record, however late, would show
/// in the count. The host namespace's records are counted with the rest
/// (the owner's ruling (a) of 2026-11-07 on #835's question 2); the names
/// each environment lists are checked as well.
fn read_makes_no_record<B: Backend>(interp: Interpreter<B>, backend: &str) {
    interp
        .eval_program_owned("(import (scheme base) (patina debug))")
        .unwrap();
    let env = interp.new_environment();
    let heap = interp.backend().heap();
    let names = || {
        (
            interp.raw_environment(&env).local_names(heap),
            interp.global_env().local_names(heap),
        )
    };
    let records = || {
        let value = interp.eval_str_owned(RECORDS).unwrap();
        interp.display_tagged(&value)
    };
    // The reading form, read once before the baseline.
    records();
    interp.eval_str_owned("(gc)").unwrap();
    let before = records();
    let listed = names();

    assert!(
        interp.eval_str_owned("never-bound-name-834").is_err(),
        "{backend}: a bare unbound reference answered"
    );
    assert!(
        interp
            .eval_str_owned("((lambda () never-bound-name-834b))")
            .is_err(),
        "{backend}: an unbound reference from compiled code answered"
    );
    assert!(
        interp.lookup_in(&env, "never-bound-name-834c").is_none(),
        "{backend}: a host's lookup of an unbound name answered"
    );
    assert!(
        interp.lookup_in(&env, "car").is_some(),
        "{backend}: a host's lookup of a name only the global binds missed"
    );

    interp.eval_str_owned("(gc)").unwrap();
    assert_eq!(records(), before, "{backend}: a read made a binding record");
    assert_eq!(names(), listed, "{backend}: a miss bound a name");
}

#[test]
fn a_read_makes_no_record() {
    read_makes_no_record(vm_interpreter(), "vm");
    read_makes_no_record(tree_walker_interpreter(), "tree-walker");
}

/// The library a raw frame of the global namespace is given to define:
/// a name no other test loads.
const REFUSED_LIBRARY: &str = "(define-library (host4b refused)
      (export refused-value)
      (import (scheme base))
      (begin (define refused-value 'loaded)))";

/// What each entry answers for three raw frames: one of the global
/// namespace, one of a host namespace, and one of temporaries over the
/// global namespace. Each frame is given `Backend::eval`, an `import` by
/// `eval_with_source_map`, and, held through the deprecated
/// `Heap::hold_environment`, `eval_str_in` and `eval_program_in`; after each
/// entry the interpreter evaluates a global form. Each is then given
/// `Backend::define`, which answers no `Result`. The frame of the global
/// namespace is also given a `define-library`, which a later global
/// `import` asks for. No frame is given a `define` form: the tree-walker
/// binds a frame of temporaries' definitions in the environment above it
/// (#680), where `Backend::define` writes the frame it is given.
#[allow(
    deprecated,
    reason = "builds the raw frames the refusal is for: `with_parent` and \
              `hold_environment` go at stage 5e (#831)"
)]
fn raw_frames<B: Backend>(interp: Interpreter<B>) -> Vec<String> {
    use patina_interpreter::Environment;
    interp.eval_program_owned("(import (scheme base))").unwrap();
    let host = interp.new_environment();
    let heap = interp.backend().heap().clone();
    let shown = |result: Result<TaggedValue, B::Error>| match result {
        Ok(value) => interp.display_tagged(value),
        Err(e) => format!("error: {e}"),
    };
    let shown_owned = |result: Result<_, _>| match result {
        Ok(value) => interp.display_tagged(&value),
        Err(e) => format!("error: {e}"),
    };
    let next = || interp.display_tagged(interp.eval_str_owned("(+ 40 2)").unwrap());
    let frames = [
        Rc::new(Environment::with_parent(interp.global_env())),
        Rc::new(Environment::with_parent(interp.raw_environment(&host))),
        Rc::new(Environment::for_temporaries(interp.global_env())),
    ];
    let mut out = Vec::new();
    for frame in &frames {
        out.push(shown(
            interp.backend().eval(read("(let ((x 7)) x)", &heap), frame),
        ));
        out.push(next());
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        out.push(shown(interp.backend().eval_with_source_map(
            read("(import (srfi 1))", &heap),
            frame,
            &source_map,
        )));
        out.push(next());
        out.push(format!(
            "frame binds iota: {}",
            frame.local_names(&heap).iter().any(|name| name == "iota")
        ));
        out.push(format!(
            "global binds iota: {}",
            interp.lookup("iota").is_some()
        ));
        let held = heap.borrow().hold_environment(Rc::clone(frame));
        out.push(shown_owned(interp.eval_str_in(&held, "(+ 1 2)")));
        out.push(next());
        out.push(shown_owned(
            interp.eval_program_in(&held, "(+ 1 2) (* 2 3)"),
        ));
        out.push(next());
        interp
            .backend()
            .define(frame, "defined-here", TaggedValue::fixnum(5));
        out.push(format!(
            "frame binds defined-here: {}",
            frame
                .local_names(&heap)
                .iter()
                .any(|name| name == "defined-here")
        ));
        out.push(format!(
            "global binds defined-here: {}",
            interp.lookup("defined-here").is_some()
        ));
    }
    out.push(shown(
        interp
            .backend()
            .eval(read(REFUSED_LIBRARY, &heap), &frames[0]),
    ));
    out.push(next());
    out.push(shown_owned(
        interp.eval_program_owned("(import (host4b refused)) refused-value"),
    ));
    out.push(next());
    out
}

/// An answer of [`raw_frames`], with the VM's refusal shortened to the kind
/// it names.
fn refusal_named(answer: String) -> String {
    const REFUSAL: &str = "the VM evaluates only in a namespace, and was given a ";
    match answer.split_once(REFUSAL) {
        Some((before, kind)) if before.starts_with("error: ") => {
            format!("refused: {}", kind.split(':').next().unwrap_or(kind))
        }
        _ => answer,
    }
}

/// The VM evaluates only in a namespace, and refuses a frame with an error
/// naming its kind, not a panic (#835): a raw frame of the global
/// namespace, one of a host namespace, and one of temporaries, by
/// `Backend::eval`, by `eval_with_source_map`, and by `eval_str_in` and
/// `eval_program_in` on a frame held through the deprecated
/// `Heap::hold_environment`. The kind is asked before anything else, so an
/// `import` refused binds nothing, and a `define-library` refused loads
/// nothing: a later global `import` of the library fails. After each
/// refusal the next form evaluates. The tree-walker evaluates each frame by
/// name, as it did before the refusal: the answers below are the base's
/// (e392fe3d), where the VM answered the same. `Backend::define` still
/// writes each frame on both backends, a frame of temporaries included, and
/// binds nothing in the global namespace.
#[test]
fn a_raw_frame_is_refused_by_the_vm() {
    let by_vm: Vec<String> = raw_frames(vm_interpreter())
        .into_iter()
        .map(refusal_named)
        .collect();
    let mut expected = Vec::new();
    for kind in ["frame", "frame", "frame of temporaries"] {
        let refused = format!("refused: {kind}");
        expected.extend(
            [
                refused.as_str(),
                "42",
                &refused,
                "42",
                "frame binds iota: false",
                "global binds iota: false",
                &refused,
                "42",
                &refused,
                "42",
                "frame binds defined-here: true",
                "global binds defined-here: false",
            ]
            .map(String::from),
        );
    }
    expected.extend(["refused: frame", "42"].map(String::from));
    assert_eq!(by_vm.len(), expected.len() + 2, "VM: {by_vm:?}");
    assert_eq!(by_vm[..expected.len()], expected, "VM");
    let library = &by_vm[expected.len()];
    assert!(
        library.starts_with("error: ") && library.contains("Library (host4b refused) not found"),
        "VM: the refused library was loaded: {library}"
    );
    assert_eq!(
        by_vm[expected.len() + 1],
        "42",
        "VM: the form after the library's import"
    );

    let mut expected = Vec::new();
    for _ in 0..3 {
        expected.extend([
            "7",
            "42",
            "#<unspecified>",
            "42",
            "frame binds iota: true",
            "global binds iota: false",
            "3",
            "42",
            "6",
            "42",
            "frame binds defined-here: true",
            "global binds defined-here: false",
        ]);
    }
    expected.extend(["#<unspecified>", "42", "loaded", "42"]);
    assert_eq!(
        raw_frames(tree_walker_interpreter()),
        expected,
        "tree-walker"
    );
}
