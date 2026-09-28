//! The CLI's diagnostic protocol through both real backends and loading paths.
mod common;

use common::{BOTH_BACKENDS, run_with_deadline};
use patina_runtime::{Diagnostic, DiagnosticKind as K, diagnostic::read_stream};
use std::fs;
use std::path::Path;

fn run(
    dir: &Path,
    backend: &[&str],
    args: &[&str],
    input: Option<&str>,
) -> (Vec<Diagnostic>, String, String, bool) {
    let mut flags = backend.to_vec();
    flags.extend(["--diagnostics-file", "errors.jsonl", "-A", "."]);
    flags.extend_from_slice(args);
    let (stdout, stderr, ok) = run_with_deadline(dir, &flags, input);
    let ds = read_stream(&fs::read_to_string(dir.join("errors.jsonl")).unwrap()).unwrap();
    (ds, stdout, stderr, ok)
}

#[test]
fn error_categories_and_payloads_survive_both_backends_and_library_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bad.scm"), "(define done #t)\n(").unwrap();
    for (name, source) in [
        (
            "reader",
            "(define-library (reader) (import (scheme base)) (include \"bad.scm\"))",
        ),
        (
            "export",
            "(define-library (export) (import (scheme base)) (export missing))",
        ),
        (
            "body",
            "(define-library (body) (import (scheme base)) (begin (error \"boom\")))",
        ),
        (
            "lookup",
            "(define-library (lookup) (import (scheme base)) (begin (define x missing)))",
        ),
        (
            "dependency",
            "(define-library (dependency) (import (absent nested)))",
        ),
        (
            "native",
            "(define-library (native) (include-shared \"native-name\"))",
        ),
        (
            "expand",
            "(define-library (expand) (import (scheme base)) (begin (if)))",
        ),
    ] {
        fs::write(dir.path().join(format!("{name}.sld")), source).unwrap();
    }
    for backend in BOTH_BACKENDS {
        for (program, kind, payload) in [
            (
                "(import (absent direct))",
                K::MissingLibrary,
                Some("absent direct"),
            ),
            (
                "(import (dependency))",
                K::MissingLibrary,
                Some("absent nested"),
            ),
            (
                "|odd name: `λ`|",
                K::UnboundIdentifier,
                Some("odd name: `λ`"),
            ),
            ("(import (reader))", K::Parse, None),
            ("(include \"bad.scm\")", K::Parse, None),
            ("(import (scheme load)) (load \"bad.scm\")", K::Parse, None),
            ("(import (export))", K::Load, None),
            ("(import (body))", K::Load, None),
            ("(import (lookup))", K::Load, None),
            ("(import (native))", K::NativeExtension, Some("native-name")),
            ("(import (expand))", K::Syntax, None),
            ("(1", K::Parse, None),
            ("(include \"missing-include.scm\")", K::Io, None),
            (
                "(import (scheme load)) (load \"missing-load.scm\")",
                K::Io,
                None,
            ),
            ("(import (only (scheme base) absent))", K::Load, None),
            (
                "(import (rename (scheme base) (absent renamed)))",
                K::Load,
                None,
            ),
            ("(error \"boom\")", K::Runtime, None),
            (
                "(import (scheme eval) (scheme repl)) (eval '(import (absent eval)) (interaction-environment))",
                K::MissingLibrary,
                Some("absent eval"),
            ),
            (
                "(import (scheme eval)) (environment '(absent env))",
                K::MissingLibrary,
                Some("absent env"),
            ),
        ] {
            fs::write(dir.path().join("program.scm"), program).unwrap();
            let (ds, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
            assert!(!ok, "{backend:?}: {program}: {stderr}");
            assert_eq!(ds.len(), 1, "{backend:?}: {program}: {stderr}");
            let d = &ds[0];
            assert_eq!(d.kind, kind, "{backend:?}: {program}: {d:?}: {stderr}");
            if let Some(payload) = payload {
                let actual = match kind {
                    K::MissingLibrary => d.library.as_ref().unwrap().join(" "),
                    K::UnboundIdentifier => d.identifier.clone().unwrap(),
                    K::NativeExtension => d.extension.clone().unwrap(),
                    _ => unreachable!(),
                };
                assert_eq!(actual, payload);
            }
            assert!(d.path.is_some(), "{backend:?}: {program}: {d:?}");
        }
    }
}

#[test]
fn keep_going_records_each_uncaught_error_and_cannot_be_hidden_by_exit_zero() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme process-context))\nfirst-missing\nsecond-missing\n(exit 0)\n";
    fs::write(dir.path().join("program.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        for (args, input) in [
            (&["-k", "program.scm"][..], None),
            (&["-k"][..], Some(program)),
        ] {
            let (ds, _, stderr, ok) = run(dir.path(), backend, args, input);
            assert!(!ok, "{stderr}");
            assert_eq!(
                ds.iter()
                    .map(|d| (d.kind, d.identifier.as_deref()))
                    .collect::<Vec<_>>(),
                [
                    (K::UnboundIdentifier, Some("first-missing")),
                    (K::UnboundIdentifier, Some("second-missing"))
                ]
            );
        }
    }
}

#[test]
fn success_and_caught_errors_emit_only_the_header_and_preserve_output() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme write) (scheme eval)) (guard (e (else (display \"caught\"))) (environment '(absent caught))) (display \"Error: Library (fake) not found\" (current-error-port))";
    fs::write(dir.path().join("program.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let (ds, stdout, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
        assert!(ok, "{backend:?}: {stderr}");
        assert!(ds.is_empty(), "{ds:?}");
        assert_eq!(stdout, "caught");
        assert_eq!(stderr, "Error: Library (fake) not found");
        let (ds, stdout, stderr, ok) = run(dir.path(), backend, &["-p", "42"], None);
        assert!(ok, "{stderr}");
        assert!(ds.is_empty());
        assert_eq!(stdout.trim(), "42");
    }
}

#[test]
fn read_failures_in_stdin_and_eval_print_are_structured() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        for (args, input) in [(&["-p", "(1"][..], None), (&[][..], Some("(1\n"))] {
            let (ds, _, stderr, ok) = run(dir.path(), backend, args, input);
            assert!(!ok, "{stderr}");
            assert_eq!(ds.len(), 1);
            assert_eq!(ds[0].kind, K::Parse);
        }
        let (ds, _, _, ok) = run(dir.path(), backend, &["nonexistent.scm"], None);
        assert!(!ok);
        assert_eq!(ds[0].kind, K::Io);
        assert_eq!(ds[0].path.as_deref(), Some("nonexistent.scm"));
    }
}
