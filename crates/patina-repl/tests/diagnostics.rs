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

#[test]
fn reader_errors_show_scheme_tokens_and_caret_context() {
    let dir = tempfile::tempdir().unwrap();
    // Exact diagnostic snapshots, shared by scripts, stdin, and both backends.
    for (program, stdout, message, line, column, width, text, opening) in [
        (
            "(import (scheme base) (scheme write))\n(display 1)\n(display 2))\n",
            "12",
            "Unexpected token: )",
            3,
            12,
            1,
            "(display 2))",
            None,
        ),
        (
            "(quote (1 . 2 3))\n",
            "",
            "Unexpected token: 3",
            1,
            15,
            1,
            "(quote (1 . 2 3))",
            Some((1, 8)),
        ),
        (
            "#(1 2 . 3)\n",
            "",
            "Unexpected token: .",
            1,
            7,
            1,
            "#(1 2 . 3)",
            Some((1, 1)),
        ),
        (
            "\"a\\q\"\n",
            "",
            "Invalid escape sequence in string: \\q",
            1,
            3,
            2,
            "\"a\\q\"",
            None,
        ),
        (
            "(display 12abc)\n",
            "",
            "Invalid syntax: Invalid number: 12abc",
            1,
            10,
            5,
            "(display 12abc)",
            None,
        ),
        (
            "(import (scheme base) (scheme write))\n(display \"\\x41 abc\")\n(display 1)\n(newline)\n; a comment; with a semicolon\n",
            "",
            "Invalid escape sequence in string: \\x41 (missing semicolon)",
            2,
            11,
            4,
            "(display \"\\x41 abc\")",
            None,
        ),
        (
            "(import (scheme base) (scheme write))\n(write '|\\x41 abc|)\n(display 1)\n(newline)\n; a comment; with a semicolon\n",
            "",
            "Invalid escape sequence in identifier: \\x41 (missing semicolon)",
            2,
            10,
            4,
            "(write '|\\x41 abc|)",
            None,
        ),
        (
            "(list 1__000)\n",
            "",
            "Invalid syntax: Invalid numeric separator in: 1__000",
            1,
            7,
            6,
            "(list 1__000)",
            None,
        ),
        (
            "(list #x#e_ff)\n",
            "",
            "Invalid syntax: Invalid numeric separator in: #x#e_ff",
            1,
            7,
            7,
            "(list #x#e_ff)",
            None,
        ),
        (
            "(list 1_000abc)\n",
            "",
            "Invalid syntax: Invalid number: 1_000abc",
            1,
            7,
            8,
            "(list 1_000abc)",
            None,
        ),
        (
            "(foo #\\bogus)\n",
            "",
            "Invalid character literal",
            1,
            6,
            7,
            "(foo #\\bogus)",
            None,
        ),
        (
            "{\n",
            "",
            "Reserved character (R7RS): {. Reserved for future extensions",
            1,
            1,
            1,
            "{",
            None,
        ),
        (
            "(quote (1\n #99#))\n",
            "",
            "Undefined datum label: #99#",
            2,
            2,
            4,
            " #99#))",
            None,
        ),
        (
            "(#0=a\n #0=b)\n",
            "",
            "Duplicate datum label: #0=",
            2,
            2,
            3,
            " #0=b)",
            None,
        ),
        (
            "#!fold-case\n  )\n",
            "",
            "Unexpected token: )",
            2,
            3,
            1,
            "  )",
            None,
        ),
        (
            "(import (scheme write))\n(display \"before\")\n  #!Fold_Case ABC\n",
            "before",
            "Unknown reader directive: #!Fold_Case",
            3,
            3,
            11,
            "  #!Fold_Case ABC",
            None,
        ),
        (
            "#!fold-case\n  #!no-fold-caseABC DEF\n",
            "",
            "Unknown reader directive: #!no-fold-caseABC",
            2,
            3,
            17,
            "  #!no-fold-caseABC DEF",
            None,
        ),
    ] {
        for ending in ["\n", "\r\n", "\r"] {
            let program = program.replace('\n', ending);
            fs::write(dir.path().join("program.scm"), &program).unwrap();
            for backend in BOTH_BACKENDS {
                for (args, input, source) in [
                    (&["program.scm"][..], None, "program.scm"),
                    (&[][..], Some(program.as_str()), "<stdin>"),
                ] {
                    let (ds, output, stderr, ok) = run(dir.path(), backend, args, input);
                    let mut expected = format!(
                        "Error: {message}\n  at {source}:{line}:{column}\n{line:>4} | {text}\n{}{}\n",
                        " ".repeat(7 + column - 1),
                        "^".repeat(width),
                    );
                    if let Some((line, column)) = opening {
                        expected.push_str(&format!("  opened at {source}:{line}:{column}\n"));
                    }
                    assert!(!ok, "{program}");
                    assert_eq!(output, stdout, "{backend:?}, {source}, {program}");
                    assert_eq!(stderr, expected, "{backend:?}, {source}, {program}");
                    assert_eq!(ds.len(), 1);
                    assert_eq!(ds[0].kind, K::Parse);
                    assert_eq!(ds[0].path.as_deref(), Some(source));
                }
            }
        }
    }
}

#[test]
fn runtime_errors_and_mixed_endings_quote_the_correct_line() {
    let dir = tempfile::tempdir().unwrap();
    for ending in ["\n", "\r\n", "\r"] {
        for backend in BOTH_BACKENDS {
            // The tree-walker's primitive type errors currently omit source
            // locations even for LF. Unbound calls carry them on both.
            let bad = if backend.is_empty() {
                "  (car 5)"
            } else {
                "  (missing)"
            };
            let program = format!("(import (scheme base)){ending}; λ{ending}{bad}{ending}");
            fs::write(dir.path().join("program.scm"), &program).unwrap();
            for (args, input, source) in [
                (&["program.scm"][..], None, "program.scm"),
                (&[][..], Some(program.as_str()), "<stdin>"),
            ] {
                let (_, _, stderr, ok) = run(dir.path(), backend, args, input);
                assert!(!ok);
                let column = if backend.is_empty() { 3 } else { 4 };
                assert!(
                    stderr.contains(&format!(
                        "at {source}:3:{column}\n   3 | {bad}\n{}^",
                        " ".repeat(7 + column - 1)
                    )),
                    "{backend:?}: {stderr}"
                );
                assert!(
                    !stderr.contains('\r'),
                    "the excerpt contains no line-ending controls"
                );
            }
        }
    }
    let program = "(import (scheme base))\r\n; comment\r\n\r; another\r  #\\bogus";
    fs::write(dir.path().join("program.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        for (args, input, source) in [
            (&["program.scm"][..], None, "program.scm"),
            (&[][..], Some(program), "<stdin>"),
        ] {
            let (_, _, stderr, ok) = run(dir.path(), backend, args, input);
            assert!(!ok);
            assert!(
                stderr.contains(&format!(
                    "at {source}:5:3\n   5 |   #\\bogus\n         ^^^^^^^"
                )),
                "{backend:?}: {stderr}"
            );
        }
    }
}

#[test]
fn loading_errors_point_into_the_file_being_read() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bad.scm"), "; first line\n  #\\bogus\n").unwrap();
    fs::write(dir.path().join("broken.sld"), "; first line\n  #\\bogus\n").unwrap();
    for (name, declaration) in [
        ("included", "include"),
        ("folded", "include-ci"),
        ("declarations", "include-library-declarations"),
    ] {
        fs::write(
            dir.path().join(format!("{name}.sld")),
            format!("(define-library ({name}) (import (scheme base)) ({declaration} \"bad.scm\"))"),
        )
        .unwrap();
    }
    for backend in BOTH_BACKENDS {
        for (program, file) in [
            ("(include \"bad.scm\")", "bad.scm"),
            ("(include-ci \"bad.scm\")", "bad.scm"),
            ("(import (scheme load)) (load \"bad.scm\")", "bad.scm"),
            ("(import (broken))", "broken.sld"),
            ("(import (included))", "bad.scm"),
            ("(import (folded))", "bad.scm"),
            ("(import (declarations))", "bad.scm"),
        ] {
            fs::write(dir.path().join("program.scm"), program).unwrap();
            let (ds, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
            assert!(!ok, "{backend:?}: {program}");
            assert!(stderr.contains("Invalid character literal"), "{stderr}");
            assert!(
                stderr.contains(&format!("{file}:2:3\n   2 |   #\\bogus\n         ^^^^^^^")),
                "{backend:?}: {program}: {stderr}"
            );
            assert!(!stderr.contains("Lexer error:"), "{stderr}");
            assert_eq!(ds.len(), 1);
            assert_eq!(ds[0].kind, K::Parse);
            assert!(ds[0].path.as_ref().unwrap().ends_with(file), "{:?}", ds[0]);
        }
    }
}

#[test]
fn malformed_booleans_are_read_errors_in_programs_and_ports() {
    let dir = tempfile::tempdir().unwrap();
    for text in ["#tfoo", "#true1", "#fasle", "#false-x", "#t#f", "#TRUE1"] {
        let program = format!("(import (scheme write))\n(display \"before\")\n(list {text})\n");
        fs::write(dir.path().join("program.scm"), &program).unwrap();
        fs::write(dir.path().join("datum.dat"), text).unwrap();
        // The Scheme read procedure must raise a read-error object before
        // returning any boolean prefix, for string ports and file ports.
        let reader = format!(
            r#"(import (scheme base) (scheme read) (scheme write) (scheme file))
(for-each
 (lambda (port)
   (write (guard (e (else (read-error? e))) (read port) #f))
   (close-input-port port))
 (list (open-input-string "{text}") (open-input-file "datum.dat")))
"#
        );
        fs::write(dir.path().join("read.scm"), reader).unwrap();
        for backend in BOTH_BACKENDS {
            for (args, input, source) in [
                (&["program.scm"][..], None, "program.scm"),
                (&[][..], Some(program.as_str()), "<stdin>"),
            ] {
                let (ds, stdout, stderr, ok) = run(dir.path(), backend, args, input);
                assert!(!ok, "{backend:?}: {text}");
                assert_eq!(stdout, "before");
                assert_eq!(
                    stderr,
                    format!(
                        "Error: Invalid boolean literal: {text}\n  at {source}:3:7\n   3 | (list {text})\n             {}\n",
                        "^".repeat(text.len())
                    )
                );
                assert_eq!(ds.len(), 1);
                assert_eq!(ds[0].kind, K::Parse);
            }
            let (ds, stdout, stderr, ok) = run(dir.path(), backend, &["read.scm"], None);
            assert!(ok, "{backend:?}: {text}: {stderr}");
            assert_eq!(stdout, "#t#t", "{backend:?}: {text}: {stderr}");
            assert!(ds.is_empty());
        }
    }
}

#[test]
fn unknown_directives_are_read_errors_on_string_file_and_stdin_ports() {
    let dir = tempfile::tempdir().unwrap();
    for text in ["#!fold_case", "#!no-fold-caseABC", "#!unknown", "#!"] {
        // The first read succeeds despite lookahead reaching the bad
        // directive. The next read must raise a Scheme read-error object.
        let data = format!("#!fold-case Before {text}\nAfter");
        fs::write(dir.path().join("datum.dat"), &data).unwrap();
        let reader = format!(
            r#"(import (scheme base) (scheme read) (scheme write) (scheme file))
(for-each
 (lambda (port)
   (write (read port))
   (write (guard (e (else (read-error? e))) (read port) #f))
   (close-input-port port))
 (list (open-input-string "{data}")
       (open-input-file "datum.dat")
       (current-input-port)))
"#
        );
        fs::write(dir.path().join("read.scm"), reader).unwrap();
        for backend in BOTH_BACKENDS {
            let (ds, stdout, stderr, ok) = run(dir.path(), backend, &["read.scm"], Some(&data));
            assert!(ok, "{backend:?}: {text}: {stderr}");
            assert_eq!(
                stdout,
                "before#t".repeat(3),
                "{backend:?}: {text}: {stderr}"
            );
            assert!(ds.is_empty());
            assert!(stderr.is_empty());
        }
    }
}

#[test]
fn numeric_separators_work_in_source_without_r6rs_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut program = "(import (scheme base) (scheme write))\n".to_owned();
    let cases = [
        ("1_000", "1000"),
        ("#xAB_CD", "43981"),
        ("#e1.2_5", "5/4"),
        ("1e1_0", "1e10"),
        ("#e1_0+2_0i", "#e10+20i"),
        ("#e+1_0i", "#e+10i"),
        ("#e1.2_5+2_0i", "#e1.25+20i"),
        ("1_0@0_0", "10@0"),
    ];
    for (text, ordinary) in cases {
        program.push_str(&format!(
            "(let ((n {text})) (write (and (= n {ordinary}) (eq? (exact? n) (exact? {ordinary})))))\n"
        ));
    }
    fs::write(dir.path().join("program.scm"), &program).unwrap();
    for backend in BOTH_BACKENDS {
        for (args, input) in [
            (&["program.scm"][..], None),
            (&[][..], Some(program.as_str())),
        ] {
            let (ds, stdout, stderr, ok) = run(dir.path(), backend, args, input);
            assert!(ok, "{backend:?}: {stderr}");
            assert_eq!(stdout, "#t".repeat(cases.len()));
            assert!(ds.is_empty());
        }
    }
}

#[test]
fn identifier_spans_survive_loading_compilation_and_macro_reordering() {
    let dir = tempfile::tempdir().unwrap();
    let body = "(define (f x)\n  (+ x undefined-thing))\n";
    fs::write(dir.path().join("body.scm"), body).unwrap();
    fs::write(dir.path().join("probe.sld"),
        "(define-library (probe)\n  (export f)\n  (import (scheme base))\n  (include \"body.scm\"))\n").unwrap();
    fs::write(dir.path().join("inline.sld"),
        "(define-library (inline)\n  (export f)\n  (import (scheme base))\n  (begin\n    (define (f x)\n      (+ x undefined-thing))))\n").unwrap();
    fs::write(
        dir.path().join("init.sld"),
        "(define-library (init)\n  (import (scheme base))\n  (begin\n    (+ 1 undefined-thing)))\n",
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        for (program, file, line, column, quoted_line) in [
            (
                "(import (init))",
                "init.sld",
                4,
                10,
                "    (+ 1 undefined-thing)))",
            ),
            (
                "(import (scheme base))\n(define (f x)\n  (+ x undefined-thing))\n(f 1)",
                "program.scm",
                3,
                8,
                "  (+ x undefined-thing))",
            ),
            (
                "(import (scheme base))\n(include \"body.scm\")\n(f 1)",
                "body.scm",
                2,
                8,
                "  (+ x undefined-thing))",
            ),
            (
                "(import (scheme base) (scheme load) (scheme repl))\n(load \"body.scm\" (interaction-environment))\n(f 1)",
                "body.scm",
                2,
                8,
                "  (+ x undefined-thing))",
            ),
            (
                "(import (scheme base) (probe))\n(f 1)",
                "body.scm",
                2,
                8,
                "  (+ x undefined-thing))",
            ),
            (
                "(import (scheme base) (inline))\n(f 1)",
                "inline.sld",
                6,
                12,
                "      (+ x undefined-thing))))",
            ),
        ] {
            fs::write(dir.path().join("program.scm"), program).unwrap();
            let (_, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
            assert!(!ok, "{backend:?}: {program}");
            assert!(
                stderr.contains(&format!("{file}:{line}:{column}")),
                "{backend:?}: {stderr}"
            );
            assert!(
                stderr.contains(&format!(
                    "{line:>4} | {quoted_line}\n{}{}",
                    " ".repeat(7 + column - 1),
                    "^".repeat(15)
                )),
                "{backend:?}: {stderr}"
            );
        }
        for (program, line, column, width, expansion) in [
            ("(set! absent 1)", 1, 7, 6, false),
            ("(list #0=absent #0#)", 1, 10, 6, false),
            (
                "(define-syntax bad (syntax-rules () ((_ x) (if #t unknown x))))\n(bad 42)",
                1,
                51,
                7,
                true,
            ),
            ("(if #f absent absent)", 1, 15, 6, false),
            (
                "(define-syntax backwards (syntax-rules () ((_ a b) (if #t b a))))\n(backwards absent absent)",
                2,
                19,
                6,
                true,
            ),
            ("(let ((x 1)) (+ x absent))", 1, 19, 6, true),
            ("(begin\r\n  (list 'λ |a\\x62;sent|))", 2, 12, 12, false),
        ] {
            let (_, _, stderr, ok) = run(dir.path(), backend, &["-p", program], None);
            assert!(!ok, "{backend:?}: {program}");
            assert!(
                stderr.contains(&format!("<eval>:{line}:{column}")),
                "{backend:?}: {stderr}"
            );
            assert!(
                stderr.contains(&format!(
                    "\n{}{}\n",
                    " ".repeat(7 + column - 1),
                    "^".repeat(width)
                )),
                "{backend:?}: {stderr}"
            );
            assert_eq!(stderr.contains("macro expansion"), expansion, "{stderr}");
        }
    }
}

#[test]
fn macro_expansion_chains_belong_to_each_invocation() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        for (program, expected) in [
            (
                "(define-syntax bad (syntax-rules () ((_ x) (if #t unknown x))))
                 (define (f) (bad 42)) (define (g) (bad 43)) (f)",
                "  macro expansion: bad",
            ),
            (
                "(define-syntax bad (syntax-rules () ((_ x) (if #t unknown x))))
                 (define-syntax outer (syntax-rules () ((_ x) (bad x))))
                 (define (f) (bad 42)) (outer 43)",
                "  macro expansion chain: outer → bad",
            ),
            (
                "(define-syntax peel (syntax-rules ()
                   ((_) absent) ((_ x rest ...) (peel rest ...))))
                 (peel 1 2)",
                "  macro expansion chain: peel → peel → peel",
            ),
        ] {
            let (_, _, stderr, ok) = run(dir.path(), backend, &["-p", program], None);
            assert!(!ok, "{backend:?}: {program}");
            assert_eq!(
                stderr.lines().last(),
                Some(expected),
                "{backend:?}: {stderr}"
            );
        }
    }
}

#[test]
fn program_annotations_do_not_escape_as_scheme_data() {
    let dir = tempfile::tempdir().unwrap();
    let program = "(import (scheme base) (scheme write) (scheme read))\n\
        (define-syntax identity (syntax-rules () ((_ x) x)))\n\
        (define cycle (identity '#0=(a . #0#)))\n\
        (define vector-cycle (identity '#0=#(a #0#)))\n\
        (define shared-tail (list '(a . #0=(tail)) '(b . #0#)))\n\
        (write (list (symbol? 'a) (eq? 'a (read (open-input-string \"a\")))\n\
          (symbol? (vector-ref '#(a) 0)) (symbol? (vector-ref #(a) 0))\n\
          (eq? cycle (cdr cycle)) (symbol? (car cycle))\n\
          (eq? vector-cycle (vector-ref vector-cycle 1)) (symbol? (vector-ref vector-cycle 0))\n\
          (eq? (cdr (car shared-tail)) (cdr (cadr shared-tail)))))";
    fs::write(dir.path().join("program.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let (_, stdout, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
        assert!(ok, "{backend:?}: {stderr}");
        assert_eq!(stdout, "(#t #t #t #t #t #t #t #t #t)");
    }
}
