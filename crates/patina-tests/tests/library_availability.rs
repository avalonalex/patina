//! #265: every expansion entry point sees the same live library catalogue.
//! Discovery must not execute libraries, freeze paths, or cross interpreters.

mod common;

use patina_interpreter::Interpreter;
use patina_runtime::Backend;
use std::path::PathBuf;

const QUERY: &str = "(cond-expand
    ((and (library (scheme base)) (library (srfi 60))
          (not (library (missing library265)))) #t)
    (else #f))";
const EXTERNAL: &str = "(cond-expand ((library (probe265 available)) #t) (else #f))";

fn run<B: Backend>(interpreter: &Interpreter<B>, program: &str) -> String {
    let value = interpreter
        .eval_program(program)
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    patina_primitives::primitives::io::datum_writer::format_display_tagged(
        value,
        interpreter.backend().global_env().heap(),
    )
}

#[test]
fn program_eval_unquote_and_child_desugarers_share_availability() {
    for program in [
        QUERY.to_string(),
        format!("(let ((x 1)) ((lambda () {QUERY})))"),
        format!("(let-syntax ((q (syntax-rules () ((_) {QUERY})))) (q))"),
        format!("(car `(,{QUERY}))"),
        format!("(import (scheme eval)) (eval '{QUERY} (environment '(scheme base)))"),
        format!("(import (scheme eval)) (eval '(car `(,{QUERY})) (environment '(scheme base)))"),
    ] {
        common::assert_program_eval_to(&program, "#t");
    }
    common::assert_program_eval_to(
        "(cond-expand ((or (library (missing library265)) (library (srfi 33))) #t) (else #f))",
        "#t",
    );
    common::assert_program_eval_to(
        "(cond-expand ((library (missing library265)) #t) (else #f))",
        "#f",
    );
}

fn external_paths<B: Backend>(interpreter: &Interpreter<B>, add_path: impl Fn(PathBuf)) {
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("probe265");
    std::fs::create_dir(&probe).unwrap();
    let marker = dir.path().join("executed.txt");
    std::fs::write(
        probe.join("available.sld"),
        format!(
            r#"(define-library (probe265 available)
            (import (scheme base) (scheme file)) (export answer)
            (begin
              (call-with-output-file "{}" (lambda (port) (write-char #\x port)))
              (define answer 42)))"#,
            marker.display()
        ),
    )
    .unwrap();
    std::fs::write(
        probe.join("part.scm"),
        format!("(define from-include {EXTERNAL})"),
    )
    .unwrap();
    std::fs::write(
        probe.join("decls.scm"),
        "(cond-expand ((library (probe265 available)) (begin (define from-decls #t)))
                      (else (begin (define from-decls #f))))",
    )
    .unwrap();
    for name in ["direct", "through-eval"] {
        std::fs::write(
            probe.join(format!("{name}.sld")),
            format!(
                "(define-library (probe265 {name})
               (import (scheme base)) (export observed)
               (cond-expand ((library (probe265 available)) (begin (define from-decl #t)))
                            (else (begin (define from-decl #f))))
               (include-library-declarations \"decls.scm\")
               (begin
                 (include \"part.scm\")
                 (define observed
                   (list from-decl from-decls from-include {EXTERNAL}
                         (let () {EXTERNAL}) (car `(,{EXTERNAL}))))))"
            ),
        )
        .unwrap();
    }

    assert_eq!(run(interpreter, EXTERNAL), "#f");
    add_path(dir.path().to_path_buf());
    assert_eq!(run(interpreter, EXTERNAL), "#t");
    assert_eq!(
        run(interpreter, "(import (probe265 direct)) observed"),
        "(#t #t #t #t #t #t)"
    );
    // A fresh library loaded by `environment` reaches the VM runtime loader,
    // rather than reusing one already loaded by the backend's import path.
    assert_eq!(
        run(
            interpreter,
            "(import (scheme eval))
         (eval 'observed (environment '(scheme base) '(probe265 through-eval)))"
        ),
        "(#t #t #t #t #t #t)"
    );
    assert!(
        !marker.exists(),
        "availability executed the dependency's body"
    );
    assert_eq!(
        run(interpreter, "(import (probe265 available)) answer"),
        "42"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "x");
    std::fs::remove_file(probe.join("available.sld")).unwrap();
    assert_eq!(
        run(interpreter, EXTERNAL),
        "#t",
        "loaded libraries remain available"
    );
}

#[test]
fn paths_includes_and_both_library_load_routes_use_nonexecuting_discovery() {
    let vm = common::vm_interpreter();
    external_paths(&vm, |path| vm.backend().add_library_search_path(path));
    let tw = common::tree_walker_interpreter();
    external_paths(&tw, |path| tw.backend().add_library_search_path(path));
}

#[test]
fn inline_libraries_are_visible_in_programs_and_library_declarations() {
    common::assert_program_eval_to(
        "(define-library (inline265 first) (import (scheme base))
           (export answer) (begin (define answer 42)))
         (define-library (inline265 second) (import (scheme base))
           (export found)
           (cond-expand ((library (inline265 first)) (begin (define found #t)))
                        (else (begin (define found #f)))))
         (import (inline265 second))
         (list found (cond-expand ((library (inline265 first)) #t) (else #f)))",
        "(#t #t)",
    );
}

#[test]
fn availability_uses_the_interpreters_filesystem_and_is_not_cached() {
    let root = tempfile::tempdir().unwrap();
    // Match the registry's canonical search root (macOS aliases /var).
    let path = root
        .path()
        .canonicalize()
        .unwrap()
        .join("probe265/available.sld");
    let fs = std::sync::Arc::new(patina_core::OverlayFs::new(std::sync::Arc::new(
        patina_core::NativeFs,
    )));
    let vm = Interpreter::new(patina_vm::VmBackend::with_fs(fs.clone()));
    let tw = common::tree_walker_interpreter_with_fs(fs.clone());
    vm.backend()
        .add_library_search_path(root.path().to_path_buf());
    tw.backend()
        .add_library_search_path(root.path().to_path_buf());
    assert_eq!(run(&vm, EXTERNAL), "#f");
    assert_eq!(run(&tw, EXTERNAL), "#f");
    fs.overlay()
        .add_text_file(&path, "(define-library (probe265 available) (export))");
    assert!(!path.exists(), "fixture must be virtual");
    assert_eq!(run(&vm, EXTERNAL), "#t");
    assert_eq!(run(&tw, EXTERNAL), "#t");
    let other_vm = common::vm_interpreter();
    let other_tw = common::tree_walker_interpreter();
    other_vm
        .backend()
        .add_library_search_path(root.path().to_path_buf());
    other_tw
        .backend()
        .add_library_search_path(root.path().to_path_buf());
    assert_eq!(run(&other_vm, EXTERNAL), "#f");
    assert_eq!(run(&other_tw, EXTERNAL), "#f");
}

#[test]
fn standalone_pipeline_uses_its_evaluators_catalogue() {
    use patina_interpreter::{Pipeline, StandardPipeline};
    let pipeline = StandardPipeline::new();
    let env = &pipeline.evaluator().global_env;
    assert_eq!(
        pipeline.eval(QUERY, env).unwrap(),
        patina_core::TaggedValue::TRUE
    );
    assert_eq!(
        pipeline.eval_program(QUERY, env).unwrap(),
        patina_core::TaggedValue::TRUE
    );
}
