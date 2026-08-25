//! Original test cases for the families of defects that Larceny's R7RS test
//! suites surfaced (Track L §L5.3, 2026-08-24).
//!
//! The suites themselves are LGPL and are not vendored — they run from a
//! reference checkout via `scripts/run_larceny_tests.sh`. Every program here
//! is written from scratch to exhibit the same *family* of problem, so the
//! repo carries its own MIT-licensed reproduction of each. The map from
//! family to the upstream test cases that found it is
//! `scheme_tests/reports/larceny_triage.md` (disposable once the queue is
//! empty); the durable record is Track L PRD §6.
//!
//! Pinning follows the crate's conventions:
//! - a wrong answer both backends agree on is asserted *as-is*, with a message
//!   saying what to do when it converges (the test fails when the bug is
//!   fixed, which is the point);
//! - a backend divergence goes through `assert_divergence`, or explicit
//!   per-backend assertions when the broken side returns a value rather than
//!   failing;
//! - a crash or hang cannot be asserted at all, so those carry the *correct*
//!   expectation under `#[ignore]` with the reason; run them with
//!   `cargo test -p patina-tests --test larceny_families -- --ignored` when
//!   working on the fix, and drop the attribute when they pass.

mod common;

use common::{
    ErrorClass, On, assert_divergence, assert_program_eval_error, assert_program_eval_to,
    eval_program, eval_program_tree_walker, eval_program_vm, scratch_path,
};
use tempfile::TempDir;

const TRIAGE: &str = "scheme_tests/reports/larceny_triage.md";

// ---------------------------------------------------------------------------
// Family 1 — a nested `include` resolves against the wrong directory
// ---------------------------------------------------------------------------

/// `outer.scm` (included by absolute path) includes `sub/middle.scm` by
/// absolute path, and `middle.scm` includes `"leaf.scm"` relatively. Every
/// implementation that runs Larceny's `base` suite resolves that last one
/// beside `middle.scm`; Patina used to look in the first file the source map
/// happened to yield, then the cwd, and find nothing.
///
/// Fixed 2026-08-24: the desugarer keeps a stack of include directories.
#[test]
fn a_nested_include_resolves_relative_to_the_including_file() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let middle = scratch_path(&dir, "sub/middle.scm");
    std::fs::write(
        dir.path().join("outer.scm"),
        format!("(include \"{middle}\")"),
    )
    .expect("write outer");
    std::fs::write(&middle, "(include \"leaf.scm\")").expect("write middle");
    std::fs::write(
        dir.path().join("sub/leaf.scm"),
        "(define leaf-value 'found)",
    )
    .expect("write leaf");

    let program = format!(
        "(include \"{}\") leaf-value",
        scratch_path(&dir, "outer.scm")
    );
    assert_program_eval_to(&program, "found");
}

// ---------------------------------------------------------------------------
// Family 2 — `equal?` does not terminate on circular structures (R7RS 6.1)
// ---------------------------------------------------------------------------

/// Two distinct cyclic lists with the same unrolling: period 2 against
/// period 4. `(equal? a a)` is fine because `eq?` short-circuits; this one
/// loops forever, so it cannot be pinned as a running test.
#[test]
#[ignore = "hangs: equal? recurses without a visited set — see larceny_triage.md family 2"]
fn equal_terminates_on_two_distinct_cyclic_lists() {
    assert_program_eval_to(
        "(define a (list 1 2))       (set-cdr! (cdr a) a)
         (define b (list 1 2 1 2))   (set-cdr! (cdddr b) b)
         (equal? a b)",
        "#t",
    );
}

/// The vector shape of the same defect overflows the Rust stack instead of
/// hanging, which is why Larceny's `read` suite dies rather than stalling.
#[test]
#[ignore = "stack overflow: equal? recurses without a visited set — see larceny_triage.md family 2"]
fn equal_terminates_on_two_distinct_cyclic_vectors() {
    assert_program_eval_to(
        "(define (cyc) (let ((v (vector 1 #f))) (vector-set! v 1 v) v))
         (equal? (cyc) (cyc))",
        "#t",
    );
}

// ---------------------------------------------------------------------------
// Family 3 — `delay-force` is not iterative (R7RS 7.3)
// ---------------------------------------------------------------------------

/// The reason `delay-force` exists is that a chain of them runs in bounded
/// space. Ours recurses per link and overflows at a hundred thousand.
#[test]
#[ignore = "stack overflow: force recurses into a delay-force chain — see larceny_triage.md family 3"]
fn a_long_delay_force_chain_runs_in_bounded_space() {
    assert_program_eval_to(
        "(import (scheme lazy))
         (define (count-down n)
           (if (= n 0) (delay 'done) (delay-force (count-down (- n 1)))))
         (force (count-down 100000))",
        "done",
    );
}

// ---------------------------------------------------------------------------
// Family 4 — VM: a discarded call to `values` poisons the next
//            `call-with-values`
// ---------------------------------------------------------------------------

/// `values` called with one argument in a non-tail position, result thrown
/// away; the next `call-with-values` whose producer returns a plain value
/// sees the stale one on the VM.
///
/// Explicit per-backend assertions rather than `assert_divergence`, because
/// the VM returns a wrong value instead of failing.
#[test]
fn a_discarded_values_call_does_not_leak_into_call_with_values() {
    const PROGRAM: &str = "(define (call1 f) (f 42))
                           (call1 values)
                           (call-with-values (lambda () 'fresh) (lambda xs xs))";
    assert_eq!(
        eval_program_tree_walker(PROGRAM),
        "(fresh)",
        "the tree-walker matches chibi; if this changed, it regressed"
    );
    assert_eq!(
        eval_program_vm(PROGRAM),
        "(42)",
        "\n[vm] NO LONGER DIVERGES — the stale values state is gone.\n\
         Replace both assertions with assert_program_eval_to(PROGRAM, \"(fresh)\") \
         and update {TRIAGE} family 4."
    );
}

// ---------------------------------------------------------------------------
// Family 5 — tree-walker: SRFI 1's n-ary procedures raise a wrong-arity error
// ---------------------------------------------------------------------------

/// `zip` is `(apply map list list1 more-lists)` inside `srfi-1-reference.scm`,
/// against SRFI 1's own n-ary `map`. The same `apply` shape written at top
/// level against `(scheme base)`'s `map` works on the tree-walker, so the
/// fault is in applying the library-internal definition.
#[test]
fn srfi_1_zip_with_two_lists_on_the_tree_walker() {
    assert_divergence(
        "(import (scheme list))
         (zip '(1 2 3) '(4 5 6))",
        On::Vm,
        "((1 4) (2 5) (3 6))",
        ErrorClass::AtRuntime,
        TRIAGE,
    );
}

// ---------------------------------------------------------------------------
// Family 6 — Unicode case mapping beyond the one-to-one table
// ---------------------------------------------------------------------------

/// Fixed 2026-08-24: the *simple* Unicode mappings (R7RS 6.6) — the full one
/// where it is a single character, the tabled simple one where it expands
/// (İ → i, ᾀ → ᾈ), the character itself where there is none (ß); the `-ci`
/// comparisons compare simple foldings (ẞ folds to ß), `string-ci=?` full
/// foldings.
#[test]
fn case_mapping_of_characters_without_a_single_character_mapping() {
    assert_program_eval_to(
        "(import (scheme char))
         (list (char-upcase #\\ß)
               (char-foldcase #\\ß)
               (char-foldcase #\\x1E9E)
               (char-downcase #\\x130)
               (char-upcase #\\x1F80)
               (char-ci=? #\\ς #\\σ)
               (char-ci=? #\\ß #\\x1E9E)
               (char-ci<? #\\ß #\\t)
               (string-ci=? \"Straße\" \"STRASSE\"))",
        "(#\\ß #\\ß #\\ß #\\i #\\ᾈ #t #t #f #t)",
    );
}

// ---------------------------------------------------------------------------
// Family 7 — `string->number` gaps the reader does not have
// ---------------------------------------------------------------------------

/// The reader accepts `+inf.0`, `-nan.0`, `1+2i` and `#e1e400` as literals;
/// `string->number` rejects the first three and reads the last through a
/// float, giving `+inf.0` where R7RS wants an exact integer (401 digits).
///
/// **When this converges, replace with `assert_program_eval_to` against
/// `(+inf.0 +nan.0 1+2i <10^400>)` — or split the last one out — and update
/// the triage doc.**
#[test]
fn string_to_number_accepts_what_the_reader_accepts() {
    let program = "(list (string->number \"+inf.0\")
                         (string->number \"-nan.0\")
                         (string->number \"1+2i\")
                         (exact? (string->number \"#e1e400\")))";
    assert_eq!(
        eval_program(program),
        "(#f #f #f #f)",
        "expected the pinned wrong answer; if this is now (+inf.0 +nan.0 1+2i #t) the defect is fixed"
    );
}

// ---------------------------------------------------------------------------
// Family 8 — `rationalize` at the infinities (R7RS 6.2.6)
// ---------------------------------------------------------------------------

/// `(rationalize +inf.0 3)` is `+inf.0` and `(rationalize 3 +inf.0)` is
/// `0.0` — the second is exact `0` today, the first `0.0`.
///
/// Fixed 2026-08-24.
#[test]
fn rationalize_with_an_infinite_argument() {
    assert_program_eval_to(
        "(import (scheme inexact))
         (list (rationalize +inf.0 3) (rationalize 3 +inf.0) (rationalize -inf.0 1))",
        "(+inf.0 0.0 -inf.0)",
    );
}

// ---------------------------------------------------------------------------
// Family 9 — `environment` rejects a nested import set
// ---------------------------------------------------------------------------

/// `(prefix (only …) …)` is an ordinary import set; `environment` should
/// accept whatever `import` accepts.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "1")` and update the triage doc.**
#[test]
fn environment_accepts_a_nested_import_set() {
    let program = "(import (scheme eval))
                   (eval '(p:car '(1 2))
                         (environment '(prefix (only (scheme base) car) p:)))";
    assert_program_eval_error(program);
}

// ---------------------------------------------------------------------------
// Family 10 — `input-port-open?` on an output port is an error, not `#f`
// ---------------------------------------------------------------------------

/// R7RS 6.13.1: `input-port-open?` "returns #t if port is still open and
/// capable of performing input" — for an output-only port that is `#f`, not
/// a type error. Larceny's `file` suite maps every port predicate over a
/// freshly opened binary port and dies here.
///
/// Fixed 2026-08-24: `#f` for the other direction, on both predicates.
#[test]
fn input_port_open_on_an_output_only_port_is_false() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch_path(&dir, "out.bin");
    let program = format!(
        "(import (scheme file))
         (define p (open-binary-output-file \"{path}\"))
         (define q (open-input-string \"\"))
         (list (output-port-open? p) (input-port-open? p)
               (input-port-open? q) (output-port-open? q))"
    );
    assert_program_eval_to(&program, "(#t #f #t #f)");
}

// ---------------------------------------------------------------------------
// Family 14 — a shadowed `...` is no longer the ellipsis (R7RS 4.3.2)
// ---------------------------------------------------------------------------

/// Where `...` is bound as a variable, a `syntax-rules` written in that scope
/// has no ellipsis: `(_ a b ...)` is a three-variable pattern. Larceny, Kawa
/// and Sagittarius agree; chibi and Gauche reject the definition, as Patina
/// does. This blocks Larceny's `base` suite at load time.
///
/// A whole-macro answer (a sentinel ellipsis name when `...` is a shadowed
/// spelling) was tried and backed out: it is a per-token, scope-aware
/// decision — an ellipsis introduced by an *outer* macro via `(... ...)`
/// must stay an ellipsis even when the expansion lands inside a scope that
/// binds `...` — and it belongs in `Compiler::is_ellipsis`, which already has
/// the scopes. See the triage doc, family 14.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "(2 1 3)")` and update the triage doc.**
#[test]
fn a_shadowed_ellipsis_is_an_ordinary_pattern_variable() {
    let program = "(let ((... 'dots))
                     (define-syntax swap-first-two
                       (syntax-rules () ((_ a b ...) (list b a ...))))
                     (swap-first-two 1 2 3))";
    assert_program_eval_error(program);
}

// ---------------------------------------------------------------------------
// Family 15 — a template's reference to a definition-site local that spells
//             a keyword is rejected as syntax
// ---------------------------------------------------------------------------

/// The macro is defined *inside* the binding, so its template's free `...`
/// (or `if`) is that local variable — the definition scopes say so. The
/// syntax-as-a-value check is by spelling and exempts scoped references
/// (hygiene: an *outer* macro's `if` must stay the special form), so it
/// reports the keyword instead. Making it scope-aware is the hygiene change
/// `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md` reserves; this is what now
/// gates Larceny's `base` suite.
///
/// **When this stops erroring, replace the assertion with
/// `assert_program_eval_to(program, "((1 dots) (2 nineteen))")` and update
/// the triage doc.**
#[test]
fn a_template_may_refer_to_a_definition_site_local_spelled_like_a_keyword() {
    let program = "(let ((... 'dots) (if 'nineteen))
                     (define-syntax mention-dots (syntax-rules () ((_ a) (list a ...))))
                     (define-syntax mention-if (syntax-rules () ((_ a) (list a if))))
                     (list (mention-dots 1) (mention-if 2)))";
    assert_program_eval_error(program);
}
