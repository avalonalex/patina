//! #435: imports are consumed during expansion, including top-level splices.
//! Each program needs a fresh environment so a prior import cannot hide a
//! missing import. The shared helpers run every case on both backends.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]
mod common;

use common::{assert_program_eval_error, assert_program_eval_to};

// #546: keep the same import set as written, passed through a pattern variable,
// and introduced by a template. Every call below gets a fresh interpreter.
fn import_forms(set: &str) -> [String; 3] {
    [
        format!("(import {set})"),
        format!(
            "(define-syntax splice (syntax-rules () ((_ form) form)))
             (splice (import {set}))"
        ),
        format!(
            "(define-syntax introduce (syntax-rules () ((_) (import {set}))))
             (introduce)"
        ),
    ]
}

#[test]
fn macro_import_modifiers_select_only_the_requested_bindings() {
    for (set, present, absent) in [
        (
            "(only (srfi 151) bitwise-and)",
            "bitwise-and",
            "bitwise-ior",
        ),
        (
            "(except (srfi 151) bitwise-ior)",
            "bitwise-and",
            "bitwise-ior",
        ),
        ("(prefix (srfi 151) p:)", "p:bitwise-and", "bitwise-and"),
        (
            "(rename (srfi 151) (bitwise-and and-bits))",
            "and-bits",
            "bitwise-and",
        ),
        (
            "(rename (prefix (except (only (srfi 151) bitwise-and bitwise-ior) bitwise-ior) p:)
                     (p:bitwise-and and-bits))",
            "and-bits",
            "p:bitwise-ior",
        ),
    ] {
        for form in import_forms(set) {
            let program = format!("(import (scheme base)) {form}");
            assert_program_eval_to(&format!("{program} ({present} 12 10)"), "8");
            assert_program_eval_error(&format!("{program} {absent}"));
        }
    }
}

#[test]
fn macro_import_modifiers_preserve_shared_library_locations() {
    let library = "(define-library (audit mutable-import)
                     (import (scheme base))
                     (export value update!)
                     (begin (define value 1)
                            (define (update! n) (set! value n))))";
    for (set, name) in [
        ("(only (audit mutable-import) value)", "value"),
        ("(except (audit mutable-import) update!)", "value"),
        ("(prefix (audit mutable-import) p:)", "p:value"),
        ("(rename (audit mutable-import) (value renamed))", "renamed"),
        (
            "(rename (prefix (except (only (audit mutable-import) value update!)
                                     update!) p:)
                     (p:value renamed))",
            "renamed",
        ),
    ] {
        for form in import_forms(set) {
            assert_program_eval_to(
                &format!(
                    "(import (scheme base)) {library}
                     (import (only (audit mutable-import) update!))
                     {form} (update! 42) {name}"
                ),
                "42",
            );
        }
    }
}

#[test]
fn malformed_macro_import_modifiers_report_the_offending_grammar() {
    for (set, message) in [
        ("(only)", "only requires an import set"),
        ("(except)", "except requires an import set"),
        (
            "(prefix (scheme cxr))",
            "prefix requires exactly 2 arguments",
        ),
        ("(rename)", "rename requires an import set"),
        ("(only (scheme cxr) 42)", "only identifiers must be symbols"),
        (
            "(except (scheme cxr) 42)",
            "except identifiers must be symbols",
        ),
        ("(prefix (scheme cxr) 42)", "prefix must be a symbol"),
        (
            "(rename (scheme cxr) (42 third))",
            "rename old name must be a symbol",
        ),
        (
            "(rename (scheme cxr) (caddr 42))",
            "rename new name must be a symbol",
        ),
        (
            "(rename (scheme cxr) (caddr third extra))",
            "rename pair must have exactly 2 elements",
        ),
    ] {
        for form in import_forms(set) {
            let program = format!("(import (scheme base)) {form}");
            for (backend, result) in [
                ("VM", common::try_eval_program_vm(&program)),
                (
                    "tree-walker",
                    common::try_eval_program_tree_walker(&program),
                ),
            ] {
                let error = result.expect_err(&program);
                assert!(
                    error.contains(message),
                    "{backend}: expected {message:?}, got {error:?}\n{program}"
                );
            }
        }
    }
}

#[test]
fn top_level_sequences_install_imports() {
    for form in [
        "(begin (import (srfi 151)))",
        "(begin 0 (begin (import (srfi 151)) 1))",
        "(cond-expand (else (import (srfi 151)) 0))",
        "(define-syntax add-import (syntax-rules () ((_) (begin (import (srfi 151)) 0)))) (add-import)",
        "(import (srfi 188)) (splicing-let-syntax () (import (srfi 151)) 0)",
        "(import (srfi 188)) (splicing-letrec-syntax () (begin (import (srfi 151))) 0)",
        "(import (rename (scheme base) (begin sequence))) (sequence (import (srfi 151)))",
        "(import (only (patina internal syntax) import)) (begin (import (only (srfi 151) bitwise-and)))",
    ] {
        assert_program_eval_to(
            &format!("(import (scheme base)) {form} (bitwise-and 6 3)"),
            "2",
        );
    }
}

#[test]
fn imported_macros_expand_later_forms_in_the_same_sequence() {
    for form in [
        "(begin (import (srfi 8)) (receive (a b) (values 1 2) (+ a b)))",
        "(cond-expand (else (import (srfi 8)) (receive (a b) (values 1 2) (+ a b))))",
        "(import (srfi 188)) (splicing-let-syntax () (import (srfi 8)) (receive (a b) (values 1 2) (+ a b)))",
        "(define-syntax splice (syntax-rules () ((_ x ...) (begin x ...)))) (splice (import (srfi 8)) (receive (a b) (values 1 2) (+ a b)))",
    ] {
        assert_program_eval_to(&format!("(import (scheme base)) {form}"), "3");
    }
}

#[test]
fn eval_installs_imports_before_expanding_the_rest() {
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme repl))
         (eval '(begin (import (srfi 8))
                       (receive (a b) (values 1 2) (+ a b)))
               (interaction-environment))",
        "3",
    );
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme repl))
         (eval '(cond-expand (else (import (srfi 151)) 0))
               (interaction-environment))
         (bitwise-and 6 3)",
        "2",
    );
}

#[test]
fn include_include_ci_and_load_preserve_spliced_imports() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forms.scm");
    std::fs::write(
        &path,
        "(import (srfi 8))\n(define result (receive (a b) (values 1 2) (+ a b)))",
    )
    .unwrap();
    for include in ["include", "include-ci"] {
        assert_program_eval_to(
            &format!(
                "(import (scheme base)) ({include} {:?}) result",
                path.to_str().unwrap()
            ),
            "3",
        );
    }
    std::fs::write(
        &path,
        "(begin (import (srfi 8)) (define result (receive (a b) (values 1 2) (+ a b))))",
    )
    .unwrap();
    assert_program_eval_to(
        &format!(
            "(import (scheme base) (scheme load)) (load {:?}) result",
            path.to_str().unwrap()
        ),
        "3",
    );
}

#[test]
fn expression_imports_and_expand_are_errors_including_under_eval() {
    for form in [
        "(if #f (import (srfi 151)) 0)",
        "(lambda () (import (srfi 151)) 0)",
        "(list (import (srfi 151)))",
        "(define x (import (srfi 151)))",
        "`(,(import (srfi 151)))",
        "(let-syntax () (import (srfi 151)) 0)",
        "(expand '(when #t 1))",
        "(list 1 (expand '(when #t 1)))",
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {form}"));
        assert_program_eval_to(
            &format!(
                "(import (scheme base) (scheme eval) (scheme repl))
                       (guard (e (else 'caught))
                         (eval '{form} (interaction-environment)) 'missed)"
            ),
            "caught",
        );
    }
}

#[test]
fn missing_and_malformed_spliced_imports_are_not_discarded() {
    for form in [
        "(begin (import (patina no-such-library-435)) 0)",
        "(cond-expand (else (import (only (scheme base) no-such-export-435)) 0))",
        "(begin (import (rename (scheme base) (no-such-export-435 x))) 0)",
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {form}"));
        assert_program_eval_to(
            &format!(
                "(import (scheme base) (scheme eval) (scheme repl))
                       (guard (e (else 'caught))
                         (eval '{form} (interaction-environment)) 'missed)"
            ),
            "caught",
        );
    }
}

#[test]
fn import_recognition_follows_the_binding_and_ignores_quoted_data() {
    assert_program_eval_to(
        "(import (scheme base)) (let ((import (lambda (x) x))) (import 42))",
        "42",
    );
    assert_program_eval_to(
        "(import (scheme base)) '(begin (import (missing library)))",
        "(begin (import (missing library)))",
    );
    assert_program_eval_to(
        "(import (scheme base)) (cond-expand (no-such-feature (import (missing library))) (else 42))",
        "42",
    );
}

#[test]
fn library_body_imports_work_through_both_loading_paths() {
    // Body imports are an extension: ordinary declaration imports remain the
    // normal library interface. Bind the keyword explicitly for these probes.
    for body in ["(import (srfi 8))", "(begin (import (srfi 8)) 0)"] {
        let library = format!(
            "(define-library (audit body-import)
               (import (scheme base) (only (patina internal syntax) import))
               (export result)
               (begin {body} (define result (receive (a b) (values 1 2) (+ a b)))))"
        );
        assert_program_eval_to(
            &format!("(import (scheme base)) {library} (import (audit body-import)) result"),
            "3",
        );
    }
    // File libraries are lazy, so environment reaches the VM's separate
    // runtime loader rather than the inline-definition loader above.
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = dir.path().join("audit");
    std::fs::create_dir(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("runtime.sld"),
        "(define-library (audit runtime)
           (import (scheme base) (only (patina internal syntax) import))
           (export result)
           (begin (begin (import (srfi 8)) 0)
                  (define result (receive (a b) (values 1 2) (+ a b)))))",
    )
    .unwrap();
    let source =
        "(import (scheme base) (scheme eval)) (eval 'result (environment '(audit runtime)))";
    let vm = common::vm_interpreter();
    vm.backend()
        .add_library_search_path(dir.path().to_path_buf());
    assert_eq!(vm.eval_program(source).unwrap().as_fixnum(), Some(3));
    let tw = common::tree_walker_interpreter();
    tw.backend()
        .add_library_search_path(dir.path().to_path_buf());
    assert_eq!(tw.eval_program(source).unwrap().as_fixnum(), Some(3));
}

#[test]
fn spliced_eval_import_preserves_initializer_exception_context() {
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = dir.path().join("audit");
    std::fs::create_dir(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("raises.sld"),
        "(define-library (audit raises) (import (scheme base)) (export result)
           (begin (define result (raise-continuable 'initializing))))",
    )
    .unwrap();
    let source = "(import (scheme base) (scheme eval) (scheme repl))
      (with-exception-handler
        (lambda (e) (if (eq? e 'initializing) 17 (raise e)))
        (lambda () (eval '(begin (import (audit raises)) (+ result 25))
                        (interaction-environment))))";
    let escape = "(import (scheme base) (scheme eval) (scheme repl))
      (define marker 42)
      (guard (e ((eq? e 'initializing) marker) (else (raise e)))
        (eval '(begin (import (audit raises)) (set! marker 0))
              (interaction-environment))
        0)";
    // Use a cold library for each case: one handler returns into initialization,
    // the other escapes it and must skip the rest of the enclosing eval.
    for program in [source, escape] {
        let vm = common::vm_interpreter();
        vm.backend()
            .add_library_search_path(dir.path().to_path_buf());
        assert_eq!(vm.eval_program(program).unwrap().as_fixnum(), Some(42));
        let tw = common::tree_walker_interpreter();
        tw.backend()
            .add_library_search_path(dir.path().to_path_buf());
        assert_eq!(tw.eval_program(program).unwrap().as_fixnum(), Some(42));
    }
}

#[test]
fn backend_boundaries_refuse_unhandled_ir() {
    use patina_core::{CoreExpr, CoreExprKind, TaggedValue};
    use std::rc::Rc;
    let interp = common::tree_walker_interpreter();
    let evaluator = interp.evaluator();
    for kind in [
        CoreExprKind::Import {
            import_sets: vec![],
        },
        CoreExprKind::Expand {
            expr: Rc::new(CoreExpr::new(CoreExprKind::Literal(TaggedValue::TRUE))),
        },
    ] {
        let expr = CoreExpr::new(CoreExprKind::Begin(vec![CoreExpr::new(kind)]));
        assert!(patina_vm::compiler::compile(&expr).is_err());
        assert!(
            patina_tree_walker::eval::eval_cps(&expr, evaluator.global_env.clone(), evaluator)
                .is_err()
        );
    }
}
