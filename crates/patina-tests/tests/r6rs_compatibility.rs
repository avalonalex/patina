//! Reading R6RS source.
//!
//! Patina is an R7RS implementation and stays one; what these tests pin is the
//! narrow set of R6RS *surface syntax* it accepts so that R6RS libraries and
//! programs can be read at all. Each case is syntax R7RS 7.1.1 reserves, so
//! accepting it widens the accepted language without changing the meaning of
//! any conforming R7RS program — the same trade recorded for the bare `@`
//! token.
//!
//! Every helper here runs both backends.

mod common;

use common::{assert_eval_to, assert_program_eval_error, assert_program_eval_to};
use patina_tree_walker::Evaluator;

// =============================================================================
// Square brackets
// =============================================================================

#[test]
fn brackets_stand_in_for_parentheses() {
    assert_eval_to("(let ([x 1] [y 2]) (+ x y))", "3");
    assert_eval_to("(cond [(= 1 2) 'no] [else 'yes])", "yes");
    assert_eval_to("(let loop ([i 0]) (if (= i 3) i (loop (+ i 1))))", "3");
}

#[test]
fn brackets_delimit_the_token_before_them() {
    // `[x 1]` must end the `1` at the bracket rather than reading `1]` as a
    // malformed number — the reason the lexer's delimiter set had to widen.
    assert_eval_to("(let ([x 1])x)", "1");
    assert_eval_to("(list 'a[quote b])", "(a b)");
}

#[test]
fn a_bracket_may_close_only_a_bracket() {
    // Accepting brackets without checking their shape would let a typo through
    // silently. Gauche, Chez, Racket and Guile all reject these.
    assert_program_eval_error("(let ([x 1)]) x)");
    assert_program_eval_error("(let ((x 1]) x)");
}

#[test]
fn brackets_are_not_identifier_characters() {
    // The widening splits `a[b]` into three tokens where it used to be one
    // symbol. Nothing conforming is lost — a bracket is neither an `<initial>`
    // nor a `<subsequent>` — and the symbol is still writable with bars.
    assert_eval_to("(symbol->string '|a[b]|)", "\"a[b]\"");
}

#[test]
fn bracket_character_literals_are_unaffected() {
    assert_eval_to(r"(char->integer #\[)", "91");
    assert_eval_to(r"(char->integer #\])", "93");
}

#[test]
fn brackets_inside_strings_are_ordinary_characters() {
    assert_eval_to(r#"(string-length "[a]")"#, "3");
}

#[test]
fn curly_braces_are_still_reserved() {
    // R7RS reserves `{ }` too, but no dialect Patina cares about spends them,
    // so they stay an error rather than being widened along with the brackets.
    assert_program_eval_error("(display {a})");
}

// =============================================================================
// Bytevector syntax
// =============================================================================

#[test]
fn r6rs_bytevector_syntax_is_read() {
    assert_eval_to("(bytevector-u8-ref '#vu8(1 2 3) 1)", "2");
    assert_eval_to("(equal? '#vu8(1 2 3) '#u8(1 2 3))", "#t");
}

#[test]
fn a_bare_hash_v_is_still_an_error() {
    assert_program_eval_error("(display '#vx(1))");
}

// =============================================================================
// Library version references
// =============================================================================

#[test]
fn an_import_may_carry_a_version_reference() {
    // R6RS §7.1 puts the version last and makes it a list. Patina has no
    // version resolution, so the reference is discarded rather than matched:
    // `(scheme base (6))` and `(scheme base)` name the same library.
    assert_program_eval_to("(import (scheme base (6)))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base ((>= 6))))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base (or (6) (7))))\n(+ 1 2)", "3");
    assert_program_eval_to("(import (scheme base ()))\n(+ 1 2)", "3");
}

#[test]
fn a_version_reference_does_not_swallow_a_name_part() {
    // `(srfi 1)` ends in a fixnum and `(scheme base)` in a symbol; neither is
    // a list, so neither is mistaken for a version.
    assert_program_eval_to("(import (scheme base) (srfi 1))\n(first '(1 2))", "1");
}

// =============================================================================
// The R6RS `library` form
// =============================================================================

#[test]
fn an_r6rs_library_form_defines_a_library() {
    // R6RS §7.1 spells the body as bare forms after the export and import
    // clauses, where R7RS wraps it in `(begin …)`.
    assert_program_eval_to(
        "(library (my math)
           (export square)
           (import (scheme base))
           (define (square x) (* x x)))
         (import (my math))
         (square 7)",
        "49",
    );
}

#[test]
fn an_r6rs_library_may_carry_a_version_and_brackets() {
    assert_program_eval_to(
        "(library (my boxed (1 0))
           (export unwrap)
           (import (scheme base (6)))
           (define (unwrap p) (let ([v (car p)]) v)))
         (import (my boxed))
         (unwrap '(11 . 12))",
        "11",
    );
}

#[test]
fn an_r6rs_library_body_may_hold_several_forms() {
    // The body is one `begin` however many forms it has, so definitions in it
    // can see each other. Read back through a procedure rather than through
    // the mutated variable itself — see `library_loading.rs`'s
    // `an_imported_variable_is_a_stale_copy_of_its_binding`.
    assert_program_eval_to(
        "(library (my counter)
           (export bump peek)
           (import (scheme base))
           (define count 0)
           (define (bump) (set! count (+ count 1)))
           (define (peek) count))
         (import (my counter))
         (bump)
         (bump)
         (peek)",
        "2",
    );
}

#[test]
fn a_library_with_no_body_is_still_a_library() {
    assert_program_eval_to(
        "(library (my empty)
           (export)
           (import (scheme base)))
         (import (my empty))
         'ok",
        "ok",
    );
}

#[test]
fn define_library_is_unchanged() {
    assert_program_eval_to(
        "(define-library (my r7)
           (export twice)
           (import (scheme base))
           (begin (define (twice x) (* 2 x))))
         (import (my r7))
         (twice 21)",
        "42",
    );
}

// =============================================================================
// `.sls` discovery
// =============================================================================

/// A library search path holding the named files, kept alive by the returned
/// `TempDir`.
fn library_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(&path, contents).expect("write library file");
    }
    dir
}

#[test]
fn a_library_is_found_in_an_sls_file() {
    // R6RS libraries are distributed as `.sls`, so a search path holding one
    // has to resolve without being renamed first.
    let dir = library_dir(&[(
        "r6/greet.sls",
        "#!r6rs
         (library (r6 greet)
           (export greeting)
           (import (scheme base))
           (define greeting 'hello))",
    )]);

    let eval = Evaluator::new();
    eval.add_library_search_path(dir.path().to_path_buf());
    let lib = eval
        .load_library(&["r6".to_string(), "greet".to_string()])
        .expect("(r6 greet) should resolve from its .sls file");
    assert_eq!(lib.name, vec!["r6", "greet"]);
}

#[test]
fn an_sld_file_wins_over_an_sls_file_beside_it() {
    // Both extensions in one directory is the case a mixed-dialect tree
    // creates; the R7RS spelling is the one Patina is.
    let dir = library_dir(&[
        (
            "dup.sld",
            "(define-library (dup) (export which) (import (scheme base))
               (begin (define which 'sld)))",
        ),
        (
            "dup.sls",
            "(library (dup) (export which) (import (scheme base)) (define which 'sls))",
        ),
    ]);

    let eval = Evaluator::new();
    eval.add_library_search_path(dir.path().to_path_buf());
    let path = eval
        .find_library_file(&["dup".to_string()])
        .expect("(dup) should resolve");
    assert_eq!(
        path.extension().and_then(|e| e.to_str()),
        Some("sld"),
        "the .sld file should win over the .sls beside it"
    );
}

#[test]
fn an_earlier_search_path_wins_over_a_later_one_whatever_it_holds() {
    // The extension preference is per search path, not across them: a `.sls`
    // in the first directory still beats a `.sld` in the second.
    let first = library_dir(&[(
        "ordered.sls",
        "(library (ordered) (export which) (import (scheme base)) (define which 'first))",
    )]);
    let second = library_dir(&[(
        "ordered.sld",
        "(define-library (ordered) (export which) (import (scheme base))
           (begin (define which 'second)))",
    )]);

    let eval = Evaluator::new();
    eval.add_library_search_path(first.path().to_path_buf());
    eval.add_library_search_path(second.path().to_path_buf());
    let path = eval
        .find_library_file(&["ordered".to_string()])
        .expect("(ordered) should resolve");
    // Compared by file name: the temp dir's own path and the resolved one
    // differ by macOS's /private symlink, so `starts_with` would not hold
    // even when the right file was found.
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("ordered.sls"),
        "expected the first search path to win, got {}",
        path.display()
    );
}

// =============================================================================
// PATINA_STRICT_R7RS
// =============================================================================

/// The strict switch is process-global (an environment variable read by the
/// frontend), so these cases go through the binary rather than in-process,
/// where a parallel test would see another's setting.
mod strict {
    use std::process::Command;

    fn patina() -> std::path::PathBuf {
        // The integration test binary sits beside the one under test.
        let mut path = std::env::current_exe().expect("test binary path");
        path.pop(); // deps/
        path.pop();
        path.push("patina");
        path
    }

    /// Run `patina -p <expr> …`, strictly or not, returning (stdout, stderr).
    ///
    /// Each expression is a separate `-p`, so each is read as a *top-level*
    /// form — which is where a library definition has to sit.
    fn eval(exprs: &[&str], strict: bool) -> (String, String) {
        let mut cmd = Command::new(patina());
        if strict {
            cmd.arg("--strict-r7rs");
        }
        for expr in exprs {
            cmd.args(["-p", expr]);
        }
        let out = cmd.output().expect("failed to run patina");
        (
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        )
    }

    /// Every extension, as a program and the value its last form has when the
    /// syntax is read.
    const R6RS_SYNTAX: [(&[&str], &str); 4] = [
        (&["(let ([x 1]) x)"], "1"),
        (&["(bytevector-u8-ref '#vu8(9) 0)"], "9"),
        (&["(import (scheme base (6)))", "'ok"], "ok"),
        (
            &[
                "(library (s t) (export v) (import (scheme base)) (define v 5))",
                "(import (s t))",
                "v",
            ],
            "5",
        ),
    ];

    #[test]
    fn the_extensions_are_read_by_default() {
        for (program, expected) in R6RS_SYNTAX {
            let (stdout, stderr) = eval(program, false);
            assert!(
                stdout.lines().last() == Some(expected),
                "{program:?} should evaluate to {expected}; got stdout {stdout:?} stderr {stderr:?}"
            );
        }
    }

    #[test]
    fn strict_mode_rejects_every_extension() {
        for (program, _) in R6RS_SYNTAX {
            let (stdout, stderr) = eval(program, true);
            assert!(
                stderr.contains("PATINA_STRICT_R7RS"),
                "{program:?} should be refused with a message naming the setting, \
                 got stdout {stdout:?} stderr {stderr:?}"
            );
        }
    }

    #[test]
    fn strict_mode_leaves_r7rs_alone() {
        // The point of the switch is that conforming programs are unaffected,
        // including the R7RS spellings of the two reader extensions.
        for (expr, expected) in [
            ("(let ((x 1)) x)", "1"),
            ("(bytevector-u8-ref '#u8(9) 0)", "9"),
            (r"(char->integer #\[)", "91"),
            (r#"(string-length "[a]")"#, "3"),
        ] {
            let (stdout, stderr) = eval(&[expr], true);
            assert_eq!(
                stdout, expected,
                "{expr} is R7RS and must survive strict mode; stderr: {stderr}"
            );
        }
    }
}
