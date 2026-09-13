//! An import set bounds what a program can name — at the top level too.
//!
//! The VM used to bind the entire primitive registry into globals by short
//! name, so a program importing only `(scheme base)` could still call `cadddr`
//! from `(scheme cxr)` or `bitwise-and` from `(srfi 151)`. The hole was
//! specific to registered primitives at the top level: libraries enforced
//! their imports all along (`sld_file_loading.rs` covers that half), and so
//! did the tree-walker — which is why these run on both backends: the point
//! is that they now agree. History in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.

mod common;
use common::{assert_program_eval_error, assert_program_eval_to, eval_program as eval};

/// `case-lambda` is unavailable until `(scheme case-lambda)` is imported.
///
/// Migrated from `case_lambda.rs`, where it was named
/// `test_case_lambda_empty_clause_list` and believed to assert that
/// `(case-lambda)` with no clauses is an error. It is not — with the library
/// imported that returns a procedure matching no call. The row passed because
/// it ran without the import, so the property it actually held is this one.
/// #193's migration is what surfaced the difference.
#[test]
fn case_lambda_needs_its_import() {
    // Unbound without the import. Asserted as "fails", not by matching the
    // diagnostic: the backends word it differently — `Undefined variable:
    // case-lambda` on the tree-walker, ``unbound variable: `case-lambda` `` on
    // the VM — which is the same reason `test_unimported_names_do_not_resolve`
    // below does not match text either. What keeps this row honest about
    // *which* failure it sees is the pair below: with the import, the same
    // expression is a procedure, so the failure here can only be resolution.
    assert_program_eval_error("(import (scheme base)) (case-lambda)");

    // With the import it is a procedure — including with *no* clauses, which is
    // the fact the old name got wrong. `(case-lambda)` expands to
    // `(lambda args (error …))` (lib/scheme/case-lambda.sld), so it accepts
    // every call and raises on each, rather than being an error to write.
    assert_program_eval_to(
        "(import (scheme case-lambda)) (procedure? (case-lambda))",
        "#t",
    );
    assert_program_eval_to(
        "(import (scheme base) (scheme case-lambda))
         (guard (e (#t 'raised)) ((case-lambda)))",
        "raised",
    );
}

/// Names a program never imported must not resolve, whether they are backed by
/// Rust primitives or by Scheme.
#[test]
fn test_unimported_names_do_not_resolve() {
    // Asserted as "fails on both backends" rather than by catching it: whether
    // an unbound variable is a catchable Scheme condition or a hard error is
    // itself a backend divergence, pre-existing and unrelated to imports.
    for expr in [
        "(cadddr (list 1 2 3 4))",  // (scheme cxr)
        "(bitwise-and 12 10)",      // (srfi 151)
        "(bit-count 12)",           // (srfi 151)
        "(arithmetic-shift 1 10)",  // (srfi 151)
        "(list-sort < (list 2 1))", // (srfi 132)
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {expr}"));
    }
}

/// The same names must still work once their library *is* imported — the point
/// is scoping, not removal.
#[test]
fn test_importing_the_library_makes_them_resolve() {
    assert_eq!(
        eval("(import (scheme base) (scheme cxr)) (cadddr (list 1 2 3 4))"),
        "4"
    );
    assert_eq!(
        eval("(import (scheme base) (srfi 151)) (list (bitwise-and 12 10) (bit-count 12))"),
        "(8 2)"
    );
    assert_eq!(
        eval("(import (scheme base) (srfi 132)) (list-sort < (list 3 1 2))"),
        "(1 2 3)"
    );
}

/// A fresh top level is still usable without importing anything: the bootstrap
/// defines `(scheme base)`'s exports, which is what it always meant to define.
#[test]
fn test_the_default_baseline_still_works() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(car (list 9 8))"), "9");
    assert_eq!(eval("(map (lambda (x) (* x x)) (list 1 2 3))"), "(1 4 9)");
}

/// `only` and `prefix` at the top level select and rename what a program can
/// name — the positive half of this file's rule, on both backends.
///
/// From `import_test.rs` (#193 Phase 2), which built a `TreeWalkInterpreter`
/// by hand and so never ran these on the VM; that file is deleted. They are
/// Rust rather than suite rows because each program's *only* import is the
/// set under test, which a shared `.scm` file — importing `(scheme base)`
/// whole at its top — cannot isolate.
#[test]
fn test_only_and_prefix_select_and_rename_at_the_top_level() {
    assert_program_eval_to("(import (scheme base)) (+ 1 2 3)", "6");
    assert_program_eval_to("(import (only (scheme base) + - *)) (* 2 3 4)", "24");
    assert_program_eval_to("(import (prefix (scheme base) s:)) (s:+ 10 20)", "30");
}

/// #211: datum I/O belongs to (scheme read)/(scheme write), including when
/// (scheme base) is selected or renamed by an import set.
#[test]
fn datum_io_is_not_exported_by_base() {
    for name in ["read", "write", "display"] {
        for program in [
            format!("(import (scheme base)) {name}"),
            format!("(import (only (scheme base) {name})) {name}"),
            format!("(import (except (scheme base) +)) {name}"),
            format!("(import (prefix (scheme base) b:)) b:{name}"),
            format!("(import (rename (scheme base) ({name} io))) io"),
        ] {
            assert_program_eval_error(&program);
        }
    }
}

/// The default environment bootstraps from base, so it follows the same rule.
#[test]
fn datum_io_needs_an_import_in_the_default_environment() {
    for name in ["read", "write", "display"] {
        assert_program_eval_error(name);
    }
}

#[test]
fn datum_io_resolves_from_its_standard_libraries() {
    assert_program_eval_to(
        r#"(import (scheme base) (only (scheme read) read))
           (read (open-input-string "42"))"#,
        "42",
    );
    for name in ["write", "display"] {
        let expected = if name == "write" {
            r#""\"hello\"""#
        } else {
            r#""hello""#
        };
        assert_program_eval_to(
            &format!(
                r#"(import (scheme base) (rename (only (scheme write) {name}) ({name} out)))
                   (let ((port (open-output-string)))
                     (out "hello" port)
                     (get-output-string port))"#
            ),
            expected,
        );
    }
    assert_program_eval_to(
        "(import (scheme base))
         (list (procedure? read-char) (procedure? write-char)
               (procedure? read-string) (procedure? write-string))",
        "(#t #t #t #t)",
    );
}
