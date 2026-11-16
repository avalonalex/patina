//! Compiled as an independent host by scripts/check_embedding_features.sh.
//! Its cfgs are host features, deliberately separate from workspace unification.
#![allow(deprecated)] // Exercise the compatibility APIs as an external caller.

use patina_interpreter::{
    Backend, Environment, EvalError, Interpreter, Owned, OwnedEnvironment, SharedHeap, SourceMap,
    TaggedValue, event, new_shared_heap,
};
use std::cell::RefCell;
use std::rc::Rc;

/// `heap()`'s typed replacements (#831), as a host outside the workspace
/// calls them: the counts, a pending signal cleared, and display notation.
fn replacements<B: Backend>(interpreter: &Interpreter<B>) {
    let before = interpreter.gc_stats();
    let held = interpreter.eval_str_owned("\"held\"").unwrap();
    assert_eq!(interpreter.gc_stats().handles, before.handles + 1);
    let mut charge = interpreter.external_bytes();
    charge.increase(4096);
    assert_eq!(
        interpreter.gc_stats().external_bytes,
        before.external_bytes + 4096
    );
    drop(charge);
    assert!(interpreter.interrupt_handle().post(event::SIGNAL));
    assert!(interpreter.take_interrupt());
    assert!(!interpreter.take_interrupt());
    assert_eq!(interpreter.display_notation(&held), "held");
    let probe = interpreter.heap_probe();
    assert!(!probe.is_gone());
}

// A host-supplied backend needs neither of Patina's concrete backends: a
// global environment, and the heap its values live on. It implements only
// the methods the trait requires and takes every default, the four handle
// methods' included (#839); rustc warns no implementation of a deprecated
// method, which the `deny` pins from outside the workspace.
struct LiteralBackend(Rc<Environment>, SharedHeap);
impl LiteralBackend {
    fn new() -> Self {
        let heap = new_shared_heap();
        Self(Rc::new(Environment::with_heap(&heap)), heap)
    }
}
#[deny(deprecated)]
impl Backend for LiteralBackend {
    type Error = EvalError;
    fn eval(&self, datum: TaggedValue, _: &Rc<Environment>) -> Result<TaggedValue, EvalError> {
        Ok(datum)
    }
    fn global_env(&self) -> &Rc<Environment> {
        &self.0
    }
    fn heap(&self) -> &SharedHeap {
        &self.1
    }
}

// The state at stage 5e: a backend that implements only the handle methods
// (#839), over `LiteralBackend`'s, and whose old methods, required and
// provided alike, are unreachable. The interpreter it serves calls none of
// them.
struct HandleBackend(LiteralBackend);
#[deny(deprecated)]
impl Backend for HandleBackend {
    type Error = EvalError;
    fn eval(&self, _: TaggedValue, _: &Rc<Environment>) -> Result<TaggedValue, EvalError> {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn global_env(&self) -> &Rc<Environment> {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn heap(&self) -> &SharedHeap {
        self.0.heap()
    }
    fn eval_global(&self, _: TaggedValue) -> Result<TaggedValue, EvalError> {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn eval_with_source_map(
        &self,
        _: TaggedValue,
        _: &Rc<Environment>,
        _: &Rc<RefCell<SourceMap>>,
    ) -> Result<TaggedValue, EvalError> {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn define(&self, _: &Rc<Environment>, _: &str, _: TaggedValue) {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn call(&self, _: TaggedValue, _: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
        unreachable!("S1: the interpreter calls the handle methods")
    }
    fn eval_in(
        &self,
        env: &OwnedEnvironment,
        expr: &Owned,
        source_map: Option<&Rc<RefCell<SourceMap>>>,
    ) -> Result<Owned, EvalError> {
        self.0.eval_in(env, expr, source_map)
    }
    fn define_in(&self, env: &OwnedEnvironment, name: &str, value: &Owned) {
        self.0.define_in(env, name, value)
    }
    fn call_owned(&self, f: &Owned, args: &[Owned]) -> Result<Owned, EvalError> {
        self.0.call_owned(f, args)
    }
    fn global_namespace(&self) -> OwnedEnvironment {
        self.0.global_namespace()
    }
}

/// `Backend`'s handle methods (#839), as a host outside the workspace calls
/// them on an identity backend, with nothing deprecated: each answer is the
/// datum or the form the default builds.
#[deny(deprecated)]
fn handle_methods<B: Backend>(interpreter: &Interpreter<B>) {
    let backend = interpreter.backend();
    let before = interpreter.gc_stats().handles;
    let hold = |value| backend.heap().borrow().hold(value);
    let global = backend.global_namespace();
    let value = backend
        .eval_in(&global, &hold(TaggedValue::fixnum(42)), None)
        .unwrap();
    assert_eq!(interpreter.raw_value(&value).as_fixnum(), Some(42));
    backend.define_in(&global, "handle-defined", &hold(TaggedValue::fixnum(43)));
    let defined = interpreter.lookup("handle-defined").expect("bound");
    assert_eq!(interpreter.raw_value(&defined).as_fixnum(), Some(43));
    let form = backend
        .call_owned(
            &hold(TaggedValue::fixnum(5)),
            &[hold(TaggedValue::fixnum(1))],
        )
        .unwrap();
    assert_eq!(interpreter.display_tagged(&form), "('5 '1)");
    drop((global, value, defined, form));
    assert_eq!(interpreter.gc_stats().handles, before);
}

/// The handle methods on a Patina backend: a datum read in the
/// interpreter's context, evaluated, defined in a host environment, and a
/// procedure called (#839).
#[cfg(any(
    feature = "vm",
    feature = "tree-walker",
    feature = "legacy-pipeline",
    feature = "defaults"
))]
#[deny(deprecated)]
fn scheme_handle_methods<B: Backend>(interpreter: &mut Interpreter<B>) {
    use patina_interpreter::Parser;
    let read = |interpreter: &mut Interpreter<B>, source: &str| {
        interpreter.with(|cx| {
            let datum = Parser::new(source).unwrap().parse(cx).unwrap();
            cx.hold(datum)
        })
    };
    let sum = read(interpreter, "(+ 40 2)");
    let define = read(interpreter, "(define y 1)");
    let doubler = read(interpreter, "(lambda (x) (* 2 x))");
    let three = read(interpreter, "3");
    let backend = interpreter.backend();
    let global = backend.global_namespace();
    let value = backend.eval_in(&global, &sum, None).unwrap();
    assert_eq!(interpreter.display_tagged(&value), "42");
    let env = interpreter.new_environment();
    backend.eval_in(&env, &define, None).unwrap();
    let y = interpreter.lookup_in(&env, "y").expect("`y` is bound there");
    assert_eq!(interpreter.display_tagged(&y), "1");
    assert!(interpreter.lookup("y").is_none());
    let doubler = backend.eval_in(&global, &doubler, None).unwrap();
    let six = backend.call_owned(&doubler, &[three]).unwrap();
    assert_eq!(interpreter.display_tagged(&six), "6");
}

#[cfg(any(
    feature = "vm",
    feature = "tree-walker",
    feature = "legacy-pipeline",
    feature = "defaults"
))]
fn check<B: Backend>(mut interpreter: Interpreter<B>)
where
    B::Error: patina_interpreter::HasSourceLocation,
{
    use patina_interpreter::{InterpreterError, format_interpreter_error};
    for input in ["42 (+ 1", "42 #(1", "42 #u8(1", "42 '", "42 #;"] {
        assert!(matches!(
            interpreter.eval_str(input),
            Err(InterpreterError::Parse(_))
        ));
        assert!(matches!(
            interpreter.eval_program(input),
            Err(InterpreterError::Parse(_))
        ));
    }
    for input in ["", "; comment\n", "#;(ignored)"] {
        assert_eq!(
            interpreter.eval_program(input).unwrap(),
            TaggedValue::UNSPECIFIED
        );
    }
    let library = "(define-library (host example)
        (export answer twice) (import (scheme base))
        (begin (define answer 21)
          (define-syntax twice (syntax-rules () ((_ x) (+ x x))))))
        (import (prefix (host example) h:)) (list (h:twice h:answer))";
    let value = interpreter.eval_program(library).unwrap();
    assert_eq!(interpreter.display_tagged(value), "(42)");
    let value = interpreter.eval_str("(values '(a b) \"hi\")").unwrap();
    assert_eq!(interpreter.display_tagged(value), "(a b)\n\"hi\"");
    let value = interpreter
        .eval_program("(define cycle (cons 1 '())) (set-cdr! cycle cycle) cycle")
        .unwrap();
    assert_eq!(interpreter.display_tagged(value), "#0=(1 . #0#)");
    // The handle forms (#605), as a host outside the workspace calls them.
    let held = interpreter
        .eval_program_owned("(define kept (list 'kept 1)) kept")
        .unwrap();
    let kept = interpreter.lookup("kept").expect("`kept` is bound");
    assert_eq!(interpreter.display_tagged(&held), "(kept 1)");
    assert_eq!(interpreter.display_tagged(kept), "(kept 1)");
    assert!(interpreter.raw_value(&held).is_pair());
    for source in [
        "(define x 42)\n(+ x missing)",
        "(define-syntax one (syntax-rules () ((_ x) x)))\n(one 1 2)",
        "(define x 42)\n(+ x",
    ] {
        let (result, map) = interpreter.eval_program_with_source_name(source, "host.scm");
        let rendered = format_interpreter_error(&result.unwrap_err(), &map.borrow());
        assert!(
            rendered.contains("host.scm:2:") && rendered.contains('^'),
            "{rendered}"
        );
    }
    interpreter.set_command_line("host", ["argument".to_owned()]);
    let value = interpreter
        .eval_program("(import (scheme process-context)) (command-line)")
        .unwrap();
    assert_eq!(interpreter.display_tagged(value), "(\"host\" \"argument\")");
    // A host's definition, which code compiled before it calls (#673).
    interpreter
        .eval_program_owned("(define (head p) (car p))")
        .unwrap();
    let replacement = interpreter
        .eval_str_owned("(lambda (p) 'replaced)")
        .unwrap();
    interpreter.define("car", &replacement);
    let head = interpreter.eval_str_owned("(head '(1 2))").unwrap();
    assert_eq!(interpreter.display_tagged(&head), "replaced");
    replacements(&interpreter);
    scheme_handle_methods(&mut interpreter);
    let probe = interpreter.heap_probe();
    drop(interpreter);
    assert!(probe.is_gone());
}

fn main() {
    let host = Interpreter::new(LiteralBackend::new());
    assert_eq!(host.eval_program("40 42").unwrap().as_fixnum(), Some(42));
    assert!(host.eval_program("42 (").is_err());
    // `Backend::define_in`'s default, over `define`'s, which a host-supplied
    // backend takes.
    host.define("answer", TaggedValue::fixnum(42));
    let answer = host.lookup("answer").expect("`answer` is bound");
    assert_eq!(host.raw_value(&answer).as_fixnum(), Some(42));
    replacements(&host);
    handle_methods(&host);
    // The global namespace's handle holds the backend's own global.
    assert!(Rc::ptr_eq(
        &host
            .backend()
            .heap()
            .borrow()
            .held_environment(&host.backend().global_namespace()),
        &host.backend().0
    ));
    // A host backend that implements only the handle methods runs the
    // interpreter's entries, none of which reaches an old method.
    let mut handles = Interpreter::new(HandleBackend(LiteralBackend::new()));
    let value = handles.eval_program_owned("40 42").unwrap();
    assert_eq!(handles.raw_value(&value).as_fixnum(), Some(42));
    let value = handles.eval_str_owned("7").unwrap();
    assert_eq!(handles.raw_value(&value).as_fixnum(), Some(7));
    assert!(handles.eval_program_owned("42 (").is_err());
    handles.define("answer", TaggedValue::fixnum(42));
    let answer = handles.lookup("answer").expect("`answer` is bound");
    assert_eq!(handles.raw_value(&answer).as_fixnum(), Some(42));
    let called = handles.call(&answer, &[]).unwrap();
    assert_eq!(handles.display_tagged(&called), "('42)");
    let env = handles.new_environment();
    let value = handles.eval_str_in(&env, "9").unwrap();
    assert_eq!(handles.raw_value(&value).as_fixnum(), Some(9));
    let value = handles.eval_program_in(&env, "8 9").unwrap();
    assert_eq!(handles.raw_value(&value).as_fixnum(), Some(9));
    handles.define_in(&env, "local", &answer);
    assert!(handles.lookup_in(&env, "local").is_some());
    assert!(handles.lookup("local").is_none());
    handle_methods(&handles);
    replacements(&handles);
    #[cfg(any(feature = "vm", feature = "defaults"))]
    check(patina_interpreter::VmInterpreter::new_vm());
    #[cfg(any(
        feature = "tree-walker",
        feature = "legacy-pipeline",
        feature = "defaults"
    ))]
    check(patina_interpreter::TreeWalkInterpreter::new_tree_walker());
    #[cfg(any(feature = "legacy-pipeline", feature = "defaults"))]
    {
        use patina_interpreter::{Pipeline, SimpleInterpreter, StandardPipeline};
        let simple = SimpleInterpreter::new();
        assert!(simple.eval_str("42 (").is_err());
        assert_eq!(simple.eval_program("").unwrap(), TaggedValue::UNSPECIFIED);
        #[cfg(feature = "legacy-pipeline")]
        {
            let pipeline = StandardPipeline::with_evaluator(patina_interpreter::Evaluator::new());
            // Both crates and the old module paths expose identical types.
            let pipeline: patina_pipeline::standard::StandardPipeline = pipeline;
            let _: patina_pipeline::pipeline::EvaluationStrategy = pipeline.strategy();
            let _: Option<patina_pipeline::error::PipelineError> = None;
            assert_eq!(
                pipeline
                    .eval("42", &pipeline.evaluator().global_env)
                    .unwrap()
                    .as_fixnum(),
                Some(42)
            );
        }
        let pipeline = StandardPipeline::new();
        let child = Rc::new(Environment::with_parent(
            pipeline.evaluator().global_env.clone(),
        ));
        pipeline
            .eval_program("(define host-local (list 'host 42))", &child)
            .unwrap();
        // A collection outside the child, then allocation into what it freed:
        // the heap traces the child for as long as this host holds it (#620).
        pipeline
            .eval_program(
                "(import (patina debug)) (gc) (define keep (list 'a 'b 'c 'd))",
                &pipeline.evaluator().global_env,
            )
            .unwrap();
        assert_eq!(
            pipeline
                .eval("(if (eq? (car host-local) 'host) (car (cdr host-local)) #f)", &child)
                .unwrap()
                .as_fixnum(),
            Some(42)
        );
        assert!(
            pipeline
                .evaluator()
                .global_env
                .get(&pipeline.evaluator().heap, "host-local")
                .is_none()
        );
        assert!(pipeline.eval("42 (", &child).is_err());
        assert_eq!(
            pipeline.eval_program("", &child).unwrap(),
            TaggedValue::UNSPECIFIED
        );
    }
    println!("embedding consumer passed");
}
