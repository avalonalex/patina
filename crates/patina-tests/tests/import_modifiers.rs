//! Fixed cross-context import cases for #592; these are regression seeds for
//! #589, not a second generator. Every case runs on both backends.

mod common;

use common::{try_eval_program_tree_walker, try_eval_program_vm};

type Runner = fn(&str) -> Result<String, String>;
const BACKENDS: [(&str, Runner); 2] = [
    ("vm", try_eval_program_vm),
    ("tree-walker", try_eval_program_tree_walker),
];

const LIBRARY: &str = r#"
(define-library (modifier source)
  (export x y bump! read-x twice (rename if choose) (rename x alias-x))
  (import (scheme base))
  (begin
    (define x 10)
    (define y 20)
    (define (bump!) (set! x (+ x 1)))
    (define (read-x) x)
    (define-syntax twice (syntax-rules () ((_ expr) (+ expr expr))))))
"#;

fn contexts(set: &str, probe: &str, dir: &tempfile::TempDir) -> Vec<(&'static str, String)> {
    let path = dir.path().join("imports.scm");
    std::fs::write(&path, format!("(import {set}) (define answer {probe})")).unwrap();
    let path = path.to_str().unwrap();
    vec![
        ("program", format!("(import {set}) {probe}")),
        (
            "library",
            format!(
                "(define-library (modifier consumer)
               (export answer) (import (scheme base) {set})
               (begin (define answer {probe})))
             (import (modifier consumer)) answer"
            ),
        ),
        (
            "eval",
            format!("(eval '(begin (import {set}) {probe}) (interaction-environment))"),
        ),
        ("load", format!("(load {path:?}) answer")),
        (
            "environment",
            format!("(eval '{probe} (environment '(scheme base) '{set}))"),
        ),
    ]
}

fn check_contexts(set: &str, probe: &str, expected: Result<&str, &str>) {
    let dir = tempfile::tempdir().unwrap();
    for (context, body) in contexts(set, probe, &dir) {
        let program = format!(
            "(import (scheme base) (scheme eval) (scheme load) (scheme repl))\n{LIBRARY}\n{body}"
        );
        for (backend, run) in BACKENDS {
            let result = run(&program);
            match expected {
                Ok(value) => assert_eq!(result.as_deref(), Ok(value), "{backend}/{context}: {set}"),
                Err(name) => {
                    let error = result.expect_err(&format!("{backend}/{context}: {set}"));
                    assert!(
                        error.contains(name) && error.contains("not found"),
                        "{backend}/{context}: wrong rejection: {error}"
                    );
                }
            }
        }
    }
}

#[test]
fn except_ignores_unknown_names_in_every_context() {
    check_contexts(
        "(except (modifier source) no-such-export no-such-export)",
        "(list x y)",
        Ok("(10 20)"),
    );
    check_contexts("(except (only (modifier source) x) y)", "x", Ok("10"));
}

#[test]
fn modifier_selection_and_syntax_agree_across_contexts() {
    for (set, probe, expected) in [
        ("(only (modifier source) x y x)", "(list x y)", "(10 20)"),
        ("(except (modifier source) y y)", "x", "10"),
        ("(only (modifier source))", "42", "42"),
        ("(except (modifier source))", "x", "10"),
        ("(rename (modifier source))", "x", "10"),
        (
            "(prefix (only (modifier source) x twice choose) p:)",
            "(p:choose #t (p:twice p:x) 0)",
            "20",
        ),
        (
            "(rename (only (modifier source) x y twice) (x y) (y x) (twice again))",
            "(list x y (again x))",
            "(20 10 40)",
        ),
        (
            "(rename (modifier source) (x renamed))",
            "(list renamed y (twice y))",
            "(10 20 40)",
        ),
        ("(rename (only (scheme base) +) (+ *))", "(* 6 7)", "13"),
    ] {
        check_contexts(set, probe, Ok(expected));
    }
}

#[test]
fn nested_modifiers_keep_exported_locations_and_macro_bindings() {
    check_contexts(
        "(only (prefix (rename (except (modifier source) y absent) (x tally)) p:)
           p:tally p:alias-x p:bump! p:read-x p:twice p:choose)",
        "(begin (p:bump!)
           (list p:tally p:alias-x (p:read-x)
                 (let ((+ -)) (p:twice p:tally)) (p:choose #t 1 0)))",
        Ok("(11 11 11 22 1)"),
    );
}

#[test]
fn only_and_rename_reject_unknown_names_at_each_modifier_boundary() {
    for (set, missing) in [
        ("(only (modifier source) x absent)", "absent"),
        ("(rename (modifier source) (absent other))", "absent"),
        ("(only (prefix (modifier source) p:) x)", "x"),
        ("(rename (except (modifier source) x) (x other))", "x"),
        ("(only (rename (modifier source) (x other)) x)", "x"),
        ("(prefix (only (modifier source) absent) p:)", "absent"),
    ] {
        check_contexts(set, "42", Err(missing));
    }
}

#[test]
fn modifier_rebinding_invalidates_already_compiled_primitive_calls() {
    let dir = tempfile::tempdir().unwrap();
    let import = "(import (rename (only (scheme base) +) (+ *)))";
    let path = dir.path().join("rebind.scm");
    std::fs::write(&path, import).unwrap();
    for rebind in [
        import.to_string(),
        format!("(eval '{import} (interaction-environment))"),
        format!("(load {:?})", path.to_str().unwrap()),
    ] {
        common::assert_program_eval_to(
            &format!(
                "(import (scheme eval) (scheme repl) (scheme load))
                       (define (op a b) (* a b)) (op 6 7)
                       {rebind} (op 6 7)"
            ),
            "13",
        );
    }
}

#[test]
fn a_failed_only_keeps_its_existing_partial_install_behavior() {
    // Preserve error-path behavior while consolidating: a final `only` installs
    // in written order until a missing name, but an inner failure never reaches
    // the destination outside its modifier. This is not rollback of loading.
    for (set, expected) in [
        ("(only (modifier source) x absent y)", "(10 0)"),
        ("(except (only (modifier source) x absent y) y)", "(0 0)"),
    ] {
        common::assert_program_eval_to(
            &format!(
                "(import (scheme eval) (scheme repl)) {LIBRARY}
                       (define x 0) (define y 0)
                       (guard (e (#t (list x y)))
                         (eval '(import {set}) (interaction-environment)))"
            ),
            expected,
        );
    }
}
