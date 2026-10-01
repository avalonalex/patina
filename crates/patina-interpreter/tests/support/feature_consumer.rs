//! Compiled as an independent host by scripts/check_embedding_features.sh.
//! Its cfgs are host features, deliberately separate from workspace unification.
#![allow(deprecated)] // Exercise the compatibility APIs as an external caller.

use patina_interpreter::{Backend, Environment, EvalError, Interpreter, TaggedValue};
use std::rc::Rc;

// A host-supplied backend needs neither of Patina's concrete backends.
struct LiteralBackend(Rc<Environment>);
impl Backend for LiteralBackend {
    type Error = EvalError;
    fn eval(&self, datum: TaggedValue, _: &Rc<Environment>) -> Result<TaggedValue, EvalError> {
        Ok(datum)
    }
    fn global_env(&self) -> &Rc<Environment> {
        &self.0
    }
}

#[cfg(any(
    feature = "vm",
    feature = "tree-walker",
    feature = "legacy-pipeline",
    feature = "defaults"
))]
fn check<B: Backend>(interpreter: Interpreter<B>)
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
}

fn main() {
    let host = Interpreter::new(LiteralBackend(Rc::new(Environment::new())));
    assert_eq!(host.eval_program("40 42").unwrap().as_fixnum(), Some(42));
    assert!(host.eval_program("42 (").is_err());
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
            .eval_program("(define host-local 42)", &child)
            .unwrap();
        assert_eq!(
            pipeline.eval("host-local", &child).unwrap().as_fixnum(),
            Some(42)
        );
        assert!(pipeline.evaluator().global_env.get("host-local").is_none());
        assert!(pipeline.eval("42 (", &child).is_err());
        assert_eq!(
            pipeline.eval_program("", &child).unwrap(),
            TaggedValue::UNSPECIFIED
        );
    }
    println!("embedding consumer passed");
}
