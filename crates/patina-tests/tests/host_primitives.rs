//! A host registers its own primitives, on both backends (#710).
//!
//! `Interpreter::register_primitive` and `register_resumable` add a host's
//! primitive to the machine's registry and export it from a library of the
//! host's, which a program imports to call it. A primitive takes the context
//! the machine makes for its call, as a built-in one does; one that calls a
//! procedure is resumable, and asks the machine to make the call.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_core::{Cx, Value};
use patina_interpreter::{
    ApplyContext, Arity, Backend, EvalError, HostPrimitives, Interpreter, PrimClass, PrimSpec,
    RegisterError, Step,
};

fn collections<B: Backend>(interp: &Interpreter<B>) -> u64 {
    interp.heap().borrow().gc_collections()
}

fn fixnum(value: Value<'_>, who: &str) -> Result<i64, EvalError> {
    value
        .as_fixnum()
        .ok_or_else(|| EvalError::TypeError(format!("{who}: expected a fixnum")))
}

/// `(host-add a b)`.
fn add<'gc>(_: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    Ok(Value::fixnum(
        fixnum(args[0], "host-add")? + fixnum(args[1], "host-add")?,
    ))
}

/// `(host-make-list n fill)`: a list it allocates.
fn make_list<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let n = fixnum(args[0], "host-make-list")?;
    let fill = args[1];
    Ok(cx.list_from_iter((0..n).map(|_| fill)))
}

/// `(host-call-twice f)`: `(list (f 1) (f 2))`, each call made by the
/// machine. Each step's state is a fresh list, `(f)` and then `(f first)`,
/// so a continuation re-entered into the first call resumes from its own.
fn call_twice_start<'gc>(
    cx: &mut Cx<'gc>,
    _: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    let f = args[0];
    let state = cx.list_from_iter([f]);
    Ok(Step::Call {
        callee: f,
        args: [Value::fixnum(1)].into_iter().collect(),
        state,
    })
}

fn call_twice_resume<'gc>(
    cx: &mut Cx<'gc>,
    _: &dyn ApplyContext,
    state: Value<'gc>,
    result: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    let f = cx.car(state);
    let rest = cx.cdr(state);
    if rest.is_null() {
        let state = cx.list_from_iter([f, result]);
        return Ok(Step::Call {
            callee: f,
            args: [Value::fixnum(2)].into_iter().collect(),
            state,
        });
    }
    let first = cx.car(rest);
    Ok(Step::Done(cx.list_from_iter([first, result])))
}

fn spec(f: patina_interpreter::Prim, arity: Arity, class: PrimClass) -> PrimSpec {
    PrimSpec { f, arity, class }
}

fn a_host_primitive<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) -> String {
    interp
        .register_primitive(
            &["host", "math"],
            "host-add",
            spec(add, Arity::Exact(2), PrimClass::Leaf),
        )
        .unwrap();
    let sum = interp
        .eval_program_owned("(import (scheme base) (host math)) (host-add 40 2)")
        .unwrap();
    interp.display_tagged(&sum)
}

#[test]
fn a_host_primitive_is_called_through_its_library() {
    assert_eq!(a_host_primitive(vm_interpreter()), "42");
    assert_eq!(a_host_primitive(tree_walker_interpreter()), "42");
}

fn an_allocating_leaf<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) -> String {
    interp
        .register_primitive(
            &["host", "lists"],
            "host-make-list",
            spec(make_list, Arity::Exact(2), PrimClass::Leaf),
        )
        .unwrap();
    let before = collections(&interp);
    let total = interp
        .eval_program_owned(
            "(import (scheme base) (host lists))
             (define (go i acc)
               (if (= i 20000) acc (go (+ i 1) (+ acc (length (host-make-list 100 i))))))
             (go 0 0)",
        )
        .unwrap();
    assert!(collections(&interp) > before, "the program did not collect");
    interp.display_tagged(&total)
}

#[test]
fn a_host_leaf_that_allocates_runs_among_collections() {
    assert_eq!(an_allocating_leaf(vm_interpreter()), "2000000");
    assert_eq!(an_allocating_leaf(tree_walker_interpreter()), "2000000");
}

/// Runs `call-twice` with a callback that captures a continuation on its
/// first call; the program re-enters it once the primitive has returned.
const REENTRY: &str = "(import (scheme base) (host control))
    (define (scheme-call-twice f) (let* ((a (f 1)) (b (f 2))) (list a b)))
    (define (run call-twice)
      (let* ((k #f)
             (n 0)
             (r (call-twice
                  (lambda (x) (call/cc (lambda (c) (if (= x 1) (set! k c)) (* x 100)))))))
        (set! n (+ n 1))
        (if (= n 1) (k 7) (list r n))))
    (list (run host-call-twice) (run scheme-call-twice))";

fn a_resumable_reentered<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) -> String {
    interp
        .register_resumable(
            &["host", "control"],
            "host-call-twice",
            Arity::Exact(1),
            call_twice_start,
            call_twice_resume,
        )
        .unwrap();
    let answer = interp.eval_program_owned(REENTRY).unwrap();
    interp.display_tagged(&answer)
}

#[test]
fn a_resumable_host_primitive_resumes_when_its_callback_is_reentered() {
    // The re-entered first call answers 7, and the primitive carries on to
    // the second, as the same procedure written in Scheme does (#471).
    let expected = "(((7 200) 2) ((7 200) 2))";
    assert_eq!(a_resumable_reentered(vm_interpreter()), expected);
    assert_eq!(a_resumable_reentered(tree_walker_interpreter()), expected);
}

/// `(force n)`, a host's: `2n`.
fn double<'gc>(_: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    Ok(Value::fixnum(2 * fixnum(args[0], "force")?))
}

fn a_host_force<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) -> String {
    interp
        .register_primitive(
            &["host", "lazy"],
            "force",
            spec(double, Arity::Exact(1), PrimClass::Leaf),
        )
        .unwrap();
    let answer = interp
        .eval_program_owned(
            "(import (scheme base) (host lazy) (prefix (scheme lazy) s:))
             (list (force 21) (s:force (s:delay 5)))",
        )
        .unwrap();
    interp.display_tagged(&answer)
}

#[test]
fn a_host_primitive_named_as_a_control_primitive_is_the_hosts() {
    // A machine picks its control primitives by the binding they came from,
    // so the host's `force` is called as itself, and `(scheme lazy)`'s still
    // forces.
    assert_eq!(a_host_force(vm_interpreter()), "(42 5)");
    assert_eq!(a_host_force(tree_walker_interpreter()), "(42 5)");
}

fn refusals<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) {
    let add = || spec(add, Arity::Exact(2), PrimClass::Leaf);
    let library = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| part.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        interp.register_primitive(&["scheme", "base"], "host-add", add()),
        Err(RegisterError::Reserved(library(&["scheme", "base"])))
    );
    interp
        .register_primitive(&["host", "once"], "host-add", add())
        .unwrap();
    assert_eq!(
        interp.register_primitive(&["host", "once"], "host-add", add()),
        Err(RegisterError::AlreadyExported {
            library: library(&["host", "once"]),
            name: "host-add".to_string(),
        })
    );
    interp
        .eval_program_owned(
            "(define-library (host made) (export y) (import (scheme base)) (begin (define y 1)))",
        )
        .unwrap();
    assert_eq!(
        interp.register_primitive(&["host", "made"], "host-add", add()),
        Err(RegisterError::NotTheHosts(library(&["host", "made"])))
    );
}

#[test]
fn a_host_registers_only_in_a_library_of_its_own() {
    refusals(vm_interpreter());
    refusals(tree_walker_interpreter());
}

fn a_later_registration<B: Backend + HostPrimitives>(mut interp: Interpreter<B>) -> String {
    interp
        .register_primitive(
            &["host", "late"],
            "host-add",
            spec(add, Arity::Exact(2), PrimClass::Leaf),
        )
        .unwrap();
    interp
        .eval_program_owned("(import (scheme base) (host late)) (host-add 1 2)")
        .unwrap();
    interp
        .register_primitive(
            &["host", "late"],
            "host-make-list",
            spec(make_list, Arity::Exact(2), PrimClass::Leaf),
        )
        .unwrap();
    let answer = interp
        .eval_program_owned("(import (host late)) (host-make-list (host-add 1 1) 'x)")
        .unwrap();
    interp.display_tagged(&answer)
}

#[test]
fn a_later_registration_reaches_a_later_import() {
    assert_eq!(a_later_registration(vm_interpreter()), "(x x)");
    assert_eq!(a_later_registration(tree_walker_interpreter()), "(x x)");
}
