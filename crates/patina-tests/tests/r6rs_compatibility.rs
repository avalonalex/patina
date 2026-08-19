//! Reading R6RS source.
//!
//! Patina is an R7RS implementation and stays one; what these tests pin is the
//! narrow set of R6RS *surface syntax* it can be asked to accept, so that R6RS
//! libraries and programs can be read at all. Each case is syntax R7RS 7.1.1
//! reserves, so reading it widens the accepted language without changing the
//! meaning of any conforming R7RS program.
//!
//! It is off by default and `--allow-r6rs` / `PATINA_ALLOW_R6RS` turns it on —
//! see `patina-frontend`'s `dialect` module for why that is one switch rather
//! than an inferred per-file mode.
//!
//! **These tests run the binary rather than the in-process helpers.** The
//! switch is an environment variable, so a test that set it in-process would
//! be visible to every other test in the same binary running in parallel.
//! Going through the process boundary is what makes each case independent, and
//! it is also the only way to exercise the CLI flag itself. Every case runs on
//! both backends.

use std::process::Command;

/// The `patina` binary, which sits beside this test binary.
fn patina() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop(); // out of deps/
    path.pop();
    path.push("patina");
    path
}

/// Which reader a case runs under.
#[derive(Clone, Copy)]
enum Dialect {
    /// The default: R7RS only.
    R7rs,
    /// `--allow-r6rs`.
    AllowR6rs,
}

/// Run `patina -p <expr> …` on one backend, returning (stdout, stderr).
///
/// Each expression is a separate `-p`, so each is read as a *top-level* form —
/// which is where a library definition has to sit. `PATINA_ALLOW_R6RS` is
/// cleared explicitly so an ambient setting cannot decide a test's outcome.
fn run(exprs: &[&str], dialect: Dialect, tree_walker: bool) -> (String, String) {
    let mut cmd = Command::new(patina());
    cmd.env_remove("PATINA_ALLOW_R6RS");
    if tree_walker {
        cmd.arg("--tree-walker");
    }
    if let Dialect::AllowR6rs = dialect {
        cmd.arg("--allow-r6rs");
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

/// Assert the last form of `exprs` evaluates to `expected`, on both backends.
fn assert_evaluates(exprs: &[&str], dialect: Dialect, expected: &str) {
    for tree_walker in [false, true] {
        let backend = if tree_walker { "tree-walker" } else { "vm" };
        let (stdout, stderr) = run(exprs, dialect, tree_walker);
        assert_eq!(
            stdout.lines().last().unwrap_or(""),
            expected,
            "[{backend}] {exprs:?}\nstdout: {stdout:?}\nstderr: {stderr:?}"
        );
    }
}

/// Assert `exprs` is refused, on both backends, by a message naming the switch.
fn assert_refused_without_the_switch(exprs: &[&str]) {
    for tree_walker in [false, true] {
        let backend = if tree_walker { "tree-walker" } else { "vm" };
        let (stdout, stderr) = run(exprs, Dialect::R7rs, tree_walker);
        assert!(
            stderr.contains("--allow-r6rs"),
            "[{backend}] {exprs:?} should be refused with a message naming the switch\n\
             stdout: {stdout:?}\nstderr: {stderr:?}"
        );
    }
}

// =============================================================================
// Square brackets
// =============================================================================

#[test]
fn brackets_stand_in_for_parentheses() {
    assert_evaluates(&["(let ([x 1] [y 2]) (+ x y))"], Dialect::AllowR6rs, "3");
    assert_evaluates(
        &["(cond [(= 1 2) 'no] [else 'yes])"],
        Dialect::AllowR6rs,
        "yes",
    );
    assert_evaluates(
        &["(let loop ([i 0]) (if (= i 3) i (loop (+ i 1))))"],
        Dialect::AllowR6rs,
        "3",
    );
}

#[test]
fn brackets_delimit_the_token_before_them() {
    // `[x 1]` must end the `1` at the bracket rather than reading `1]` as a
    // malformed number — the reason the lexer's delimiter set had to widen.
    assert_evaluates(&["(let ([x 1])x)"], Dialect::AllowR6rs, "1");
    assert_evaluates(&["(list 'a[quote b])"], Dialect::AllowR6rs, "(a b)");
}

#[test]
fn a_bracket_may_close_only_a_bracket() {
    // Reading brackets without checking their shape would let a typo through
    // silently. Gauche, Chez, Racket and Guile all reject these.
    for program in [&["(let ([x 1)]) x)"], &["(let ((x 1]) x)"]] {
        let (_, stderr) = run(program, Dialect::AllowR6rs, false);
        assert!(
            stderr.contains("Mismatched delimiter"),
            "{program:?} should be refused as a mismatch, got {stderr:?}"
        );
    }
}

#[test]
fn brackets_are_not_identifier_characters() {
    // Reading brackets splits `a[b]` into three tokens where it used to be one
    // symbol. Nothing conforming is lost — a bracket is neither an `<initial>`
    // nor a `<subsequent>` — and the symbol is still writable with bars.
    assert_evaluates(
        &["(symbol->string '|a[b]|)"],
        Dialect::AllowR6rs,
        "\"a[b]\"",
    );
}

// =============================================================================
// Bytevector syntax
// =============================================================================

#[test]
fn r6rs_bytevector_syntax_is_read() {
    assert_evaluates(
        &["(bytevector-u8-ref '#vu8(1 2 3) 1)"],
        Dialect::AllowR6rs,
        "2",
    );
    assert_evaluates(
        &["(equal? '#vu8(1 2 3) '#u8(1 2 3))"],
        Dialect::AllowR6rs,
        "#t",
    );
}

#[test]
fn a_bare_hash_v_is_not_a_bytevector() {
    let (_, stderr) = run(&["'#vx(1)"], Dialect::AllowR6rs, false);
    assert!(!stderr.is_empty(), "#vx( should not be read");
}

// =============================================================================
// Library version references
// =============================================================================

#[test]
fn an_import_may_carry_a_version_reference() {
    // R6RS §7.1 puts the version last and makes it a list. Patina has no
    // version resolution, so the reference is discarded rather than matched:
    // `(scheme base (6))` and `(scheme base)` name the same library.
    for version in ["(6)", "((>= 6))", "(or (6) (7))", "()"] {
        let import = format!("(import (scheme base {version}))");
        assert_evaluates(&[&import, "(+ 1 2)"], Dialect::AllowR6rs, "3");
    }
}

#[test]
fn a_version_reference_does_not_swallow_a_name_part() {
    // `(srfi 1)` ends in a fixnum and `(scheme base)` in a symbol; neither is
    // a list, so neither is mistaken for a version.
    assert_evaluates(
        &["(import (scheme base) (srfi 1))", "(first '(1 2))"],
        Dialect::AllowR6rs,
        "1",
    );
}

// =============================================================================
// The R6RS `library` form
// =============================================================================

/// A library written the R6RS way: bare body forms after the clauses, where
/// R7RS wraps them in `(begin …)`.
const R6RS_LIBRARY: [&str; 3] = [
    "(library (my math)
       (export square)
       (import (scheme base))
       (define (square x) (* x x)))",
    "(import (my math))",
    "(square 7)",
];

#[test]
fn an_r6rs_library_form_defines_a_library() {
    assert_evaluates(&R6RS_LIBRARY, Dialect::AllowR6rs, "49");
}

#[test]
fn an_r6rs_library_body_may_hold_several_forms() {
    // The body is one `begin` however many forms it has, so definitions in it
    // can see each other. Read back through a procedure rather than through a
    // mutated variable — see `library_loading.rs`'s
    // `an_imported_variable_is_a_stale_copy_of_its_binding`.
    assert_evaluates(
        &[
            "(library (my counter)
               (export bump peek)
               (import (scheme base))
               (define count 0)
               (define (bump) (set! count (+ count 1)))
               (define (peek) count))",
            "(import (my counter))",
            "(bump)",
            "(bump)",
            "(peek)",
        ],
        Dialect::AllowR6rs,
        "2",
    );
}

#[test]
fn a_library_with_no_body_is_still_a_library() {
    assert_evaluates(
        &[
            "(library (my empty) (export) (import (scheme base)))",
            "(import (my empty))",
            "'ok",
        ],
        Dialect::AllowR6rs,
        "ok",
    );
}

// =============================================================================
// R7RS is the default, and is unaffected
// =============================================================================

#[test]
fn every_extension_is_refused_by_default() {
    assert_refused_without_the_switch(&["(let ([x 1]) x)"]);
    assert_refused_without_the_switch(&["(bytevector-u8-ref '#vu8(9) 0)"]);
    assert_refused_without_the_switch(&["(import (scheme base (6)))", "'ok"]);
    assert_refused_without_the_switch(&R6RS_LIBRARY);
}

#[test]
fn r7rs_is_unaffected_by_either_setting() {
    // The R7RS spellings of both reader extensions, a bracket in the two
    // places it is an ordinary character, and `define-library`.
    for dialect in [Dialect::R7rs, Dialect::AllowR6rs] {
        assert_evaluates(&["(let ((x 1)) x)"], dialect, "1");
        assert_evaluates(&["(bytevector-u8-ref '#u8(9) 0)"], dialect, "9");
        assert_evaluates(&[r"(char->integer #\[)"], dialect, "91");
        assert_evaluates(&[r"(char->integer #\])"], dialect, "93");
        assert_evaluates(&[r#"(string-length "[a]")"#], dialect, "3");
        assert_evaluates(
            &[
                "(define-library (my r7)
                   (export twice)
                   (import (scheme base))
                   (begin (define (twice x) (* 2 x))))",
                "(import (my r7))",
                "(twice 21)",
            ],
            dialect,
            "42",
        );
    }
}

#[test]
fn curly_braces_are_reserved_under_either_setting() {
    // R7RS reserves `{ }` too, but no dialect Patina cares about spends them,
    // so they stay an error rather than being read along with the brackets.
    for dialect in [Dialect::R7rs, Dialect::AllowR6rs] {
        let (_, stderr) = run(&["'{a}"], dialect, false);
        assert!(
            stderr.contains("Reserved character"),
            "curly braces should stay reserved, got {stderr:?}"
        );
    }
}
