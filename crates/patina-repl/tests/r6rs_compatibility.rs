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
//! switch is an environment variable, so a test that set it in-process would be
//! visible to every other test in the same binary running in parallel. Going
//! through the process boundary is what makes each case independent, and it is
//! also the only way to exercise the CLI flag itself.
//!
//! They live in this crate rather than in `patina-tests` for a mundane but
//! load-bearing reason: `CARGO_BIN_EXE_patina` is only defined for integration
//! tests of the package that declares the binary, and it is the only spelling
//! cargo *guarantees* to have built. Locating the binary by hand from
//! `current_exe()` compiles anywhere and silently tests whatever stale build
//! happens to be on disk. Every case runs on both backends.

mod common;

use common::run_patina;

/// Which reader a case runs under.
#[derive(Clone, Copy)]
enum Dialect {
    /// The default: R7RS only.
    R7rs,
    /// `--allow-r6rs`.
    AllowR6rs,
}

impl Dialect {
    fn flags(self) -> &'static [&'static str] {
        match self {
            Dialect::R7rs => &[],
            Dialect::AllowR6rs => &["--allow-r6rs"],
        }
    }
}

/// Run `patina -p <expr> …` on one backend, returning (stdout, stderr).
///
/// Each expression is a separate `-p`, so each is read as a *top-level* form —
/// which is where a library definition has to sit.
fn run(exprs: &[&str], dialect: Dialect, tree_walker: bool) -> (String, String) {
    let mut args: Vec<&str> = Vec::new();
    if tree_walker {
        args.push("--tree-walker");
    }
    args.extend_from_slice(dialect.flags());
    for expr in exprs {
        args.extend_from_slice(&["-p", expr]);
    }
    let (stdout, stderr, _) = run_patina(std::path::Path::new("."), &args);
    (stdout.trim().to_string(), stderr.trim().to_string())
}

/// Run `exprs` on both backends, handing each result to `check`.
fn on_both_backends(exprs: &[&str], dialect: Dialect, check: impl Fn(&str, &str, &str)) {
    for tree_walker in [false, true] {
        let backend = if tree_walker { "tree-walker" } else { "vm" };
        let (stdout, stderr) = run(exprs, dialect, tree_walker);
        check(backend, &stdout, &stderr);
    }
}

/// Assert the last form of `exprs` evaluates to `expected`, on both backends.
fn assert_evaluates(exprs: &[&str], dialect: Dialect, expected: &str) {
    on_both_backends(exprs, dialect, |backend, stdout, stderr| {
        assert_eq!(
            stdout.lines().last().unwrap_or(""),
            expected,
            "[{backend}] {exprs:?}\nstdout: {stdout:?}\nstderr: {stderr:?}"
        );
    });
}

/// Assert `exprs` is refused on both backends, by a message containing `needle`.
///
/// Both backends, because a refusal that only one reader performs is exactly
/// the divergence worth catching.
fn assert_refused(exprs: &[&str], dialect: Dialect, needle: &str) {
    on_both_backends(exprs, dialect, |backend, stdout, stderr| {
        assert!(
            stderr.contains(needle),
            "[{backend}] {exprs:?} should be refused with a message containing \
             {needle:?}\nstdout: {stdout:?}\nstderr: {stderr:?}"
        );
    });
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
        assert_refused(program, Dialect::AllowR6rs, "Mismatched delimiter");
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
    assert_refused(&["'#vx(1)"], Dialect::AllowR6rs, "Unexpected character");
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
    for program in [
        &["(let ([x 1]) x)"][..],
        &["(bytevector-u8-ref '#vu8(9) 0)"][..],
        &["(import (scheme base (6)))", "'ok"][..],
        &R6RS_LIBRARY[..],
    ] {
        assert_refused(program, Dialect::R7rs, "--allow-r6rs");
    }
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
        assert_refused(&["'{a}"], dialect, "Reserved character");
    }
}

// =============================================================================
// `.sls` discovery
// =============================================================================
//
// R6RS libraries are distributed as `.sls`, so a search path holding one has to
// resolve without being renamed first. These go through `-A` and a real import
// rather than through a resolver method directly: `SchemeLibraryLoader` is the
// resolver an actual load uses, and asserting against the other one would prove
// nothing about whether a program can import the file.

/// Write `files` into a fresh temp directory and return it.
fn library_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(&path, contents).expect("write library file");
    }
    dir
}

/// `patina [--allow-r6rs] -A <dir>… -p <expr>…`, on both backends.
///
/// Search paths are appended in the order given, which is what the ordering
/// case below turns on.
fn assert_evaluates_with_library_dirs(
    dirs: &[&std::path::Path],
    exprs: &[&str],
    dialect: Dialect,
    expected: &str,
) {
    let dirs: Vec<&str> = dirs
        .iter()
        .map(|d| d.to_str().expect("utf-8 temp path"))
        .collect();

    for tree_walker in [false, true] {
        let backend = if tree_walker { "tree-walker" } else { "vm" };
        let mut args: Vec<&str> = Vec::new();
        if tree_walker {
            args.push("--tree-walker");
        }
        args.extend_from_slice(dialect.flags());
        for dir in &dirs {
            args.extend_from_slice(&["-A", dir]);
        }
        for expr in exprs {
            args.extend_from_slice(&["-p", expr]);
        }
        let (stdout, stderr, _) = run_patina(std::path::Path::new("."), &args);
        assert_eq!(
            stdout.trim().lines().last().unwrap_or(""),
            expected,
            "[{backend}] {exprs:?}\nstdout: {stdout:?}\nstderr: {stderr:?}"
        );
    }
}

#[test]
fn a_library_is_found_in_an_sls_file() {
    let dir = library_dir(&[(
        "r6/greet.sls",
        "#!r6rs
         (library (r6 greet)
           (export greeting)
           (import (rnrs base (6)))
           (define greeting 'hello))",
    )]);
    assert_evaluates_with_library_dirs(
        &[dir.path()],
        &["(import (r6 greet))", "greeting"],
        Dialect::AllowR6rs,
        "hello",
    );
}

#[test]
fn an_sld_file_wins_over_an_sls_file_beside_it() {
    // Both extensions in one directory is what a mixed-dialect tree creates;
    // the R7RS spelling is the one Patina is. Under the R7RS reader, so the
    // losing `.sls` would be a parse error if it were opened at all.
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
    assert_evaluates_with_library_dirs(
        &[dir.path()],
        &["(import (dup))", "which"],
        Dialect::R7rs,
        "sld",
    );
}

#[test]
fn an_earlier_search_path_wins_over_a_later_one_whatever_it_holds() {
    // The extension preference is per search path, not across them: a `.sls` in
    // the first directory still beats a `.sld` in the second.
    let first = library_dir(&[(
        "ordered.sls",
        "(library (ordered) (export which) (import (scheme base)) (define which 'first))",
    )]);
    let second = library_dir(&[(
        "ordered.sld",
        "(define-library (ordered) (export which) (import (scheme base))
           (begin (define which 'second)))",
    )]);
    assert_evaluates_with_library_dirs(
        &[first.path(), second.path()],
        &["(import (ordered))", "which"],
        Dialect::AllowR6rs,
        "first",
    );
}
