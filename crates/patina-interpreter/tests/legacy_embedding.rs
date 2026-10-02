//! Regression cases for the convenience API drift in #595.
#![cfg(feature = "legacy-pipeline")]
#![allow(deprecated)]
#![allow(
    clippy::disallowed_methods,
    reason = "a test crate: it evaluates through `Interpreter::eval_*` as an embedder does, so it \
              allows the re-entry rule (#622) once, as the test crates do in their Cargo.toml"
)]

use patina_interpreter::{SimpleInterpreter, TaggedValue, TreeWalkInterpreter};

#[test]
fn single_expression_rejects_an_incomplete_suffix() {
    for input in ["42 (+ 1", "42 #(1", "42 #u8(1", "42 '", "42 #;"] {
        assert!(
            TreeWalkInterpreter::new_tree_walker()
                .eval_str(input)
                .is_err()
        );
        assert!(SimpleInterpreter::new().eval_str(input).is_err(), "{input}");
    }
}

#[test]
fn empty_program_returns_unspecified() {
    for input in ["", "; comment\n", "#;(ignored)"] {
        assert_eq!(
            SimpleInterpreter::new().eval_program(input).unwrap(),
            TaggedValue::UNSPECIFIED
        );
    }
}

#[test]
fn inline_library_is_loaded_before_its_import() {
    let source = "(define-library (embedding example)
        (export answer) (import (scheme base)) (begin (define answer 42)))
        (import (embedding example)) answer";
    assert_eq!(
        SimpleInterpreter::new()
            .eval_program(source)
            .unwrap()
            .as_fixnum(),
        Some(42)
    );
}

#[test]
fn compatibility_adapters_keep_their_error_categories_and_execution_order() {
    use patina_interpreter::{Pipeline, PipelineError, StandardPipeline};
    let simple = SimpleInterpreter::new();
    let pipeline = StandardPipeline::new();
    let env = pipeline.evaluator().global_env.clone();
    let malformed_macro = "(define-syntax one (syntax-rules () ((_ x) x))) (one 1 2)";
    for result in [
        simple.eval_program(malformed_macro),
        pipeline.eval_program(malformed_macro, &env),
    ] {
        assert!(
            matches!(result, Err(PipelineError::Desugaring(_))),
            "{result:?}"
        );
    }
    for result in [simple.eval_str("missing"), pipeline.eval("missing", &env)] {
        assert!(
            matches!(result, Err(PipelineError::Evaluation(_))),
            "{result:?}"
        );
    }
    let source = "(define ran-before-error 42) (";
    for result in [
        simple.eval_program(source),
        pipeline.eval_program(source, &env),
    ] {
        assert!(
            matches!(result, Err(PipelineError::Frontend(_))),
            "{result:?}"
        );
    }
    assert_eq!(
        simple
            .global_env()
            .get("ran-before-error")
            .unwrap()
            .as_fixnum(),
        Some(42)
    );
    assert_eq!(env.get("ran-before-error").unwrap().as_fixnum(), Some(42));
    let source = "(define should-not-run 42) (";
    assert!(simple.eval_str(source).is_err());
    assert!(pipeline.eval(source, &env).is_err());
    assert!(simple.global_env().get("should-not-run").is_none());
    assert!(env.get("should-not-run").is_none());
}
