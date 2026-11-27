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
            (
                "(set! |odd set!: `λ`| 1)",
                K::UnboundIdentifier,
                Some("odd set!: `λ`"),
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
            "Invalid syntax: Invalid number \"12abc\": expected end of number at character 3, found 'a'",
            1,
            12,
            1,
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
            "Invalid syntax: Invalid number \"1__000\": expected a single underscore between digits at character 2, found '_'",
            1,
            8,
            1,
            "(list 1__000)",
            None,
        ),
        (
            "(list #x#e_ff)\n",
            "",
            "Invalid syntax: Invalid number \"#x#e_ff\": expected a single underscore between digits at character 5, found '_'",
            1,
            11,
            1,
            "(list #x#e_ff)",
            None,
        ),
        (
            "(list 1_000abc)\n",
            "",
            "Invalid syntax: Invalid number \"1_000abc\": expected end of number at character 6, found 'a'",
            1,
            12,
            1,
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
            // A macro another macro's expansion defined, used by a later
            // form: its template keeps the defining expansion (#612).
            (
                "(define-syntax def-bad (syntax-rules ()
                   ((_ name) (define-syntax name
                               (syntax-rules () ((_ x) (if #t unknown x)))))))
                 (def-bad bad)
                 (bad 42)",
                "  macro expansion chain: def-bad → bad",
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

/// The 100th `eval` of one quoted datum reports an error inside a `case`
/// clause as the first did (#612): an expansion's chain once grew on the
/// datum's pairs with every `eval`. Since a list is located by its head
/// (#792), a quoted datum carries no location, decision 11's loss: neither
/// report has an expansion chain, and each names the `eval` call that ran
/// the code on the VM, one call in `run`, and nothing on the tree-walker.
#[test]
fn an_error_in_the_hundredth_eval_of_a_datum_reports_as_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut program = String::from(
        "(import (scheme base) (scheme eval) (scheme repl))\n\
         (define form '(case 3 ((1 2) 'a) ((3) (missing 5)) (else 'c)))\n\
         (define (run) (eval form (interaction-environment)))\n",
    );
    program.push_str(&"(run)\n".repeat(100));
    fs::write(dir.path().join("program.scm"), program).unwrap();
    for backend in BOTH_BACKENDS {
        let (_, _, stderr, ok) = run(dir.path(), backend, &["-k", "program.scm"], None);
        assert!(!ok, "{backend:?}: {stderr}");
        let mut reports: Vec<String> = Vec::new();
        for line in stderr.lines() {
            if line.starts_with("Error:") {
                reports.push(String::new());
            }
            if let Some(report) = reports.last_mut() {
                report.push_str(line);
                report.push('\n');
            }
        }
        assert_eq!(reports.len(), 100, "{backend:?}: {stderr}");
        let located = if backend.is_empty() {
            reports[0].contains("  at program.scm:3:15\n")
        } else {
            !reports[0].contains("  at ")
        };
        assert!(
            located && reports[0].contains("missing") && !reports[0].contains("macro expansion"),
            "{backend:?}: {}",
            reports[0]
        );
        assert_eq!(reports[99], reports[0], "{backend:?}");
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

/// That `stderr` reports `source:line:column`, quotes that line of
/// `program` with `carets` carets under the reported span, and then ends
/// with the expansion `chain` line, or has none.
fn assert_located(
    stderr: &str,
    source: &str,
    program: &str,
    (line, column, carets): (usize, usize, usize),
    chain: Option<&str>,
) {
    let lines: Vec<&str> = stderr.lines().collect();
    let at = format!("  at {source}:{line}:{column}");
    let i = lines
        .iter()
        .position(|l| *l == at)
        .unwrap_or_else(|| panic!("no `{at}` in:\n{stderr}"));
    let text = program.lines().nth(line - 1).unwrap();
    assert_eq!(
        lines.get(i + 1).copied(),
        Some(format!("{line:>4} | {text}").as_str()),
        "{stderr}"
    );
    assert_eq!(
        lines.get(i + 2).copied(),
        Some(format!("{}{}", " ".repeat(7 + column - 1), "^".repeat(carets)).as_str()),
        "{stderr}"
    );
    match chain {
        Some(chain) => assert_eq!(lines.last().copied(), Some(chain), "{stderr}"),
        None => assert!(!stderr.contains("macro expansion"), "{stderr}"),
    }
}

/// That `stderr` reports no location and no expansion chain.
fn assert_unlocated(stderr: &str) {
    assert!(
        stderr.contains("Error:")
            && !stderr.contains("  at ")
            && !stderr.contains("macro expansion"),
        "{stderr}"
    );
}

/// Each case's program: `(import (scheme base))`, then one form per line.
fn program_of(lines: &[&str]) -> String {
    format!("(import (scheme base))\n{}\n", lines.join("\n"))
}

/// Where the VM reports a run-time error, and what the excerpt marks (#787).
/// GC stage 4c moves provenance from a table keyed by raw bits onto identifiers,
/// and from 4c.6 a list is located by its head identifier (#792). Each case
/// says whether 4c keeps it or which decision on #782 loses it, and what a
/// lost one reports: nothing, or the call that ran its procedure. The
/// tree-walker reports no location for a run-time error, so these are the
/// VM's.
#[test]
fn run_time_errors_are_located_at_their_forms_on_the_vm() {
    let dir = tempfile::tempdir().unwrap();
    for (lines, at, chain) in [
        // Kept: an application form in a procedure body.
        (&["(define (f) (car 5))", "(f)"][..], Some((2, 13, 7)), None),
        // Kept: a subform a macro substitutes, under the use's chain.
        (
            &[
                "(define-syntax wrap (syntax-rules () ((_ e) (let ((t 1)) e))))",
                "(define (k) (wrap (car 5)))",
                "(k)",
            ][..],
            Some((3, 19, 7)),
            Some("  macro expansion chain: wrap → let"),
        ),
        // Kept (decision 16): a form a template introduces reports the
        // call site.
        (
            &[
                "(define-syntax bad (syntax-rules () ((_ x) (car x))))",
                "(define (h) (bad 5))",
                "(h)",
            ][..],
            Some((3, 13, 7)),
            Some("  macro expansion: bad"),
        ),
        // Kept (decision 16): the same for a dotted template's form, inside
        // a lambda the template builds, where no fallback reaches it (#792).
        (
            &[
                "(define-syntax bad2 (syntax-rules () ((_ x . r) (lambda () (car x . r)))))",
                "(define k (bad2 5))",
                "(k)",
            ][..],
            Some((3, 11, 8)),
            Some("  macro expansion: bad2"),
        ),
        // Kept: an identifier-headed form inside a lambda-headed one.
        (
            &["((lambda (v) (vector-ref v 9)) (vector))"][..],
            Some((2, 14, 16)),
            None,
        ),
        // Kept: a vector literal as an argument; the application is located.
        (&["(vector-ref #(1 2) 9)"][..], Some((2, 1, 21)), None),
        // Lost (decision 12): a head that is a call. Not `(g)`'s `3:2`.
        (&["(define (g) 5)", "((g) 1)"][..], None, None),
        // Lost (decision 12): a head that is a vector literal. The
        // procedure has no location left, and the call that ran it reports.
        (
            &["(define (f) (#(1 2) 0))", "(f)"][..],
            Some((3, 1, 3)),
            None,
        ),
        // Lost (decision 12): a head that is a string literal, the same.
        (
            &["(define (f) (\"ab\" 0))", "(f)"][..],
            Some((3, 1, 3)),
            None,
        ),
    ] {
        let program = program_of(lines);
        fs::write(dir.path().join("program.scm"), &program).unwrap();
        let (_, _, stderr, ok) = run(dir.path(), &[], &["program.scm"], None);
        assert!(!ok, "{program}");
        match at {
            Some(at) => assert_located(&stderr, "program.scm", &program, at, chain),
            None => assert_unlocated(&stderr),
        }
    }
}

/// What each form 4c.6 leaves without a location reports (#792): lost, never
/// misattributed. A report names the failing form, one that encloses it, or
/// the call that ran it, or nothing; never a neighbour, as the VM's code map
/// did before when a call with no location took the last one before it.
/// Each row is a program, then its VM answer and its tree-walker answer: a
/// location with its carets, or none. The tree-walker locates no run-time
/// error, so most rows read none there; a desugar error and the `eval`
/// cases are on both. The expansion chain is the last line where a row
/// names one, and absent elsewhere.
#[test]
fn forms_left_without_a_location_report_no_other_form() {
    type At = Option<(usize, usize, usize)>;
    let base = "(import (scheme base))";
    let eval = "(import (scheme base) (scheme eval) (scheme repl))";
    let rows: &[(&[&str], At, At, Option<&str>)] = &[
        // A head that is a call, at top level, in a body and in a call.
        (&[base, "(define (g) 5)", "((g) 1)"], None, None, None),
        (
            &[base, "(define (g) 5)", "(define (k) ((g) 1))", "(k)"],
            None,
            None,
            None,
        ),
        (
            &[base, "(define (g) 5)", "(define (k) (list ((g) 1)))", "(k)"],
            Some((3, 13, 14)),
            None,
            None,
        ),
        // A lambda head with the wrong arity, and a quoted head.
        (
            &[base, "(define (k) ((lambda (v) v)))", "(k)"],
            None,
            None,
            None,
        ),
        (&[base, "(define (k) ('car 5))", "(k)"], None, None, None),
        // A string, vector or number head as a procedure's body: the
        // procedure has no location left, and its caller reports.
        (
            &[base, "(define (f) (\"ab\" 0))", "(f)"],
            Some((3, 1, 3)),
            None,
            None,
        ),
        (
            &[base, "(define (f) (#(1 2) 0))", "(f)"],
            Some((3, 1, 3)),
            None,
            None,
        ),
        (
            &[base, "(define (k) (1 2))", "(k)"],
            Some((3, 1, 3)),
            None,
            None,
        ),
        // A template's form whose head is no identifier, inside a lambda the
        // template builds, and inside a call it builds (kept).
        (
            &[
                base,
                "(define-syntax thunk0 (syntax-rules () ((_ f) (lambda () ((f))))))",
                "(define (g) 5)",
                "(define k (thunk0 g))",
                "(k)",
            ],
            None,
            None,
            None,
        ),
        (
            &[
                base,
                "(define-syntax call0 (syntax-rules () ((_ f) (list ((f))))))",
                "(define (g) 5)",
                "(define (k) (call0 g))",
                "(k)",
            ],
            Some((4, 13, 9)),
            None,
            Some("  macro expansion: call0"),
        ),
        // One head for two lists, through a datum label: neither list has a
        // location, nor takes the other's.
        (
            &[base, "(define (k) (#0=car '(1)) (#0# 6))", "(k)"],
            None,
            None,
            None,
        ),
        // A desugar error in a form whose head is a call: the enclosing
        // definition, on both backends.
        (
            &[base, "(define (g) 5)", "(define (k) ((g) . 1))"],
            Some((3, 1, 22)),
            Some((3, 1, 22)),
            None,
        ),
        // Quoted data given to `eval` (decision 11), and data built at run
        // time that holds a quoted list, a vector or a string: the `eval`
        // call on the VM, nothing on the tree-walker, and no chain.
        (
            &[
                eval,
                "(define form '(case 3 ((1 2) 'a) ((3) (missing 5)) (else 'c)))",
                "(eval form (interaction-environment))",
            ],
            Some((3, 1, 37)),
            None,
            None,
        ),
        (
            &[
                eval,
                "(define form '(let ((x 5)) (car x)))",
                "(eval form (interaction-environment))",
            ],
            Some((3, 1, 37)),
            None,
            None,
        ),
        (
            &[
                eval,
                "(define form '(if))",
                "(eval form (interaction-environment))",
            ],
            Some((3, 1, 37)),
            None,
            None,
        ),
        (
            &[
                eval,
                "(define (g) 5)",
                "(eval (list '(g) 1) (interaction-environment))",
            ],
            Some((3, 1, 46)),
            None,
            None,
        ),
        (
            &[
                eval,
                "(eval (list 'vector-ref '#(1 2) 9) (interaction-environment))",
            ],
            Some((2, 1, 61)),
            None,
            None,
        ),
        (
            &[
                eval,
                "(eval (list 'string-ref \"ab\" 9) (interaction-environment))",
            ],
            Some((2, 1, 58)),
            None,
            None,
        ),
        // A quasiquote's call with no location beside a call that has one:
        // the enclosing `(list …)`, not `(car '(1))`.
        (
            &[base, "(define (f) (list (car '(1)) `(1 ,@5 2)))", "(f)"],
            Some((2, 13, 28)),
            None,
            None,
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (lines, vm, tree_walker, chain) in rows {
        let program = format!("{}\n", lines.join("\n"));
        fs::write(dir.path().join("program.scm"), &program).unwrap();
        for (backend, at) in BOTH_BACKENDS.iter().zip([vm, tree_walker]) {
            let (_, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
            assert!(!ok, "{backend:?}: {program}");
            match at {
                Some(at) => assert_located(&stderr, "program.scm", &program, *at, *chain),
                None => assert_unlocated(&stderr),
            }
        }
    }
}

/// Where both backends report an expansion error or an unbound reference,
/// which the tree-walker locates as the VM does (#787): the cases GC stage
/// 4c must keep, each saying why.
#[test]
fn expansion_errors_and_references_are_located_on_both_backends() {
    let dir = tempfile::tempdir().unwrap();
    for backend in BOTH_BACKENDS {
        for (lines, at, chain) in [
            // Kept: an unbound reference in a procedure body.
            (&["(define (f) (nope 5))", "(f)"][..], (2, 14, 4), None),
            // Kept (decision 16): a reference a template introduces reports
            // the template's text.
            (
                &[
                    "(define-syntax bad (syntax-rules () ((_ x) (nope x))))",
                    "(define (h) (bad 5))",
                    "(h)",
                ][..],
                (2, 45, 4),
                Some("  macro expansion: bad"),
            ),
            // Kept (decision 16): a desugar error in a macro's output reports
            // the call site.
            (
                &[
                    "(define-syntax bad (syntax-rules () ((_ x) (if))))",
                    "(define (h) (bad 5))",
                ][..],
                (3, 13, 7),
                Some("  macro expansion: bad"),
            ),
            // Kept: a reference inside a form whose head is a call.
            (
                &["(define (g) 5)", "(define (k) ((g) nope2))", "(k)"][..],
                (3, 18, 5),
                None,
            ),
            // Kept: `,x` outside a quasiquote, whose head 4c.6 makes a
            // written identifier.
            (&["(define x 1)", "  ,x"][..], (3, 3, 2), None),
            // Kept, as `,x`.
            (&["(define x (list 1))", "  ,@x"][..], (3, 3, 3), None),
        ] {
            let program = program_of(lines);
            fs::write(dir.path().join("program.scm"), &program).unwrap();
            let (_, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
            assert!(!ok, "{backend:?}: {program}");
            assert_located(&stderr, "program.scm", &program, at, chain);
        }
    }
}

/// A location whose text the input stream has dropped keeps its line and
/// column and quotes nothing: the program is read from standard input, and
/// its last form comes after more than `SOURCE_TEXT_KEPT` (1 MiB) of text.
/// Standard input quotes the template's line until then, and a file read
/// whole keeps it. Decision 17 on #782 recommends keeping this answer, and
/// 4c.5's compaction test starts from it (#787).
///
/// Data built at run time and given to `eval` reports no location of its own
/// and no expansion chain: on the VM the `eval` call that ran it, and on the
/// tree-walker nothing. Before #792 the VM reported nothing here, because
/// the program's `''a` evaluated to the reader's own `(quote a)` pair, whose
/// location reached the `eval`ed code's map; a quote abbreviation's head is
/// a written identifier now, so quote stripping copies the pair. Decision
/// 11's quoted datum reports the same
/// (`an_error_in_the_hundredth_eval_of_a_datum_reports_as_the_first`).
#[test]
fn a_dropped_line_keeps_its_location_and_built_data_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let short = program_of(&[
        "(define-syntax bad (syntax-rules () ((_ x) (nope x))))",
        "(bad 5)",
    ]);
    let mut program = program_of(&["(define-syntax bad (syntax-rules () ((_ x) (nope x))))"]);
    program.push_str(&format!("#t ;{}\n", "x".repeat(1000)).repeat(1100));
    program.push_str("(bad 5)\n");
    fs::write(dir.path().join("program.scm"), &program).unwrap();
    let built = "(import (scheme base) (scheme eval) (scheme repl))\n\
        (eval (list 'case 3 (list (list 1 2) ''a) (list (list 3) (list 'missing 5)) (list 'else ''c))\n      \
        (interaction-environment))\n";
    fs::write(dir.path().join("built.scm"), built).unwrap();
    for backend in BOTH_BACKENDS {
        let (_, _, stderr, ok) = run(dir.path(), backend, &[], Some(&short));
        assert!(!ok, "{backend:?}");
        assert_located(
            &stderr,
            "<stdin>",
            &short,
            (2, 45, 4),
            Some("  macro expansion: bad"),
        );
        let (_, _, stderr, ok) = run(dir.path(), backend, &[], Some(&program));
        assert!(!ok, "{backend:?}");
        let lines: Vec<&str> = stderr.lines().collect();
        let at = lines
            .iter()
            .position(|l| *l == "  at <stdin>:2:45")
            .unwrap_or_else(|| panic!("{backend:?}: {stderr}"));
        assert_eq!(
            &lines[at + 1..],
            ["  macro expansion: bad"],
            "{backend:?}: {stderr}"
        );
        let (_, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
        assert!(!ok, "{backend:?}");
        assert_located(
            &stderr,
            "program.scm",
            &program,
            (2, 45, 4),
            Some("  macro expansion: bad"),
        );
        let (_, _, stderr, ok) = run(dir.path(), backend, &["built.scm"], None);
        assert!(!ok, "{backend:?}");
        assert!(stderr.contains("missing"), "{backend:?}: {stderr}");
        if backend.is_empty() {
            assert_located(&stderr, "built.scm", built, (2, 1, 1), None);
        } else {
            assert_unlocated(&stderr);
        }
    }
}

/// A name the reader wrote inside a macro definition that another macro's
/// expansion carries reports the chain of both (#791): `nope` in `bad`'s
/// template, written inside `(wrap …)`, reports `wrap → bad`, on both
/// backends, as it did when the expansion's stamp rewrote the reader's
/// location in place. Since #791 an identifier holds its location, and the
/// stamp gives such a name a copy with the extended chain rather than
/// writing an identifier another context made.
#[test]
fn a_name_written_in_a_nested_macro_definition_keeps_both_chains() {
    let dir = tempfile::tempdir().unwrap();
    let program = program_of(&[
        "(define-syntax wrap (syntax-rules () ((_ form) form)))",
        "(wrap (define-syntax bad (syntax-rules () ((_) (nope)))))",
        "(bad)",
    ]);
    fs::write(dir.path().join("program.scm"), &program).unwrap();
    for backend in BOTH_BACKENDS {
        let (_, _, stderr, ok) = run(dir.path(), backend, &["program.scm"], None);
        assert!(!ok, "{backend:?}");
        assert_located(
            &stderr,
            "program.scm",
            &program,
            (3, 49, 4),
            Some("  macro expansion chain: wrap → bad"),
        );
    }
}

/// Compaction keeps an entry a live identifier carries (decision 17 on
/// #782, #791): a program on standard input defines `g` through
/// `def-getter` on its first lines, then 1,100 lines of a thousand
/// characters each, which drop those lines' text, with a collection every
/// hundred lines, then calls `(g)`. The error reports `<stdin>:1:94` with
/// `def-getter → g`, with no excerpt, or reports no location; never another
/// one, which a reused entry would give.
#[test]
fn a_compacted_stream_never_misattributes_an_old_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut program = String::from(
        "(define-syntax def-getter (syntax-rules () ((_ get) (define-syntax get (syntax-rules () ((_) nope))))))\n\
         (def-getter g)\n\
         (import (scheme base) (patina debug))\n",
    );
    for i in 0..1100 {
        program.push_str(&format!(
            "(define v{i} (list 'a 'b)) ;{}\n",
            "x".repeat(1000)
        ));
        if i % 100 == 99 {
            program.push_str("(gc)\n");
        }
    }
    program.push_str("(g)\n");
    for backend in BOTH_BACKENDS {
        let (_, _, stderr, ok) = run(dir.path(), backend, &[], Some(&program));
        assert!(!ok, "{backend:?}");
        assert!(stderr.contains("nope"), "{backend:?}: {stderr}");
        let lines: Vec<&str> = stderr.lines().collect();
        match lines.iter().position(|l| l.starts_with("  at ")) {
            None => {}
            Some(at) => {
                assert_eq!(lines[at], "  at <stdin>:1:94", "{backend:?}: {stderr}");
                assert_eq!(
                    &lines[at + 1..],
                    ["  macro expansion chain: def-getter → g"],
                    "{backend:?}: {stderr}"
                );
            }
        }
    }
}
