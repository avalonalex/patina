//! Original test cases for the families of defects that Larceny's R7RS test
//! suites surfaced (Track L §L5.3, 2026-08-24).
//!
//! **Being redistributed, not kept.** #193 Phase 1 is moving these rows to the
//! file about their *subject* rather than about where they were found — see
//! `docs/TEST_ORGANIZATION.md`'s section on it. 64 rows have become 9; the
//! remainder is the macro and hygiene block, plus what genuinely cannot leave
//! Rust. Two rows that could not are already gone to files that own their
//! subject: family 1's nested `include` to `include_syntax.rs` and family 10's
//! port predicates to `standard_ports.rs`, both because they need real files
//! on disk. When the last row leaves, this file goes with it and the binary
//! count drops by one.
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

use common::{ErrorClass, On, assert_divergence, assert_program_eval_to};

// ---------------------------------------------------------------------------
// Family 33 — a template's `quote` resolved at the use site
// ---------------------------------------------------------------------------

/// The import half, and the literal form. SRFI 101 exports its own `quote`,
/// which builds random-access lists. Its own template `(get-cached 'datum)`
/// must reach the `quote` SRFI 101 was written against — it used to reach
/// the exported one and expand without end — and a *literal* `'(1 2)` in
/// another library's template must be a pair, not whatever the program's
/// `quote` builds. Both are the definition site's `quote`; only the user's
/// own `'(1 2)` is the program's.
///
/// Fixed 2026-08-26: the relinker rewrites a `quote` head it used to skip,
/// and the template compiler compiles the `quote` of a literal datum as a
/// reference rather than emitting it verbatim with the datum.
#[test]
fn a_templates_quote_is_the_definition_sites_quote_under_an_imported_one() {
    assert_program_eval_to(
        "(define-library (probe lit)
           (import (scheme base))
           (export lit)
           (begin (define-syntax lit (syntax-rules () ((_) '(1 2))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe lit))
         (r7:list (let ((f (lambda () '(x)))) (eq? (f) (f)))
                  (car '(1 2))
                  (r7:pair? (lit)))",
        "(#t 1 #t)",
    );
}

// ---------------------------------------------------------------------------
// Family 35 — relinking rewrote the user's code inside a macro call
// ---------------------------------------------------------------------------

/// `both`'s template uses `(scheme base)`'s `list`; the program's `list` is
/// SRFI 101's. The template's reference is relinked to the definition
/// site's `list` — that is referential transparency — but the `(list 1 2)`
/// the user wrote *as the argument* means the program's, and it used to be
/// rewritten too, because the relinker matched by spelling. So `v` is a
/// pair and its second element is not.
///
/// Fixed 2026-08-26: the relinker renames only identifiers carrying the
/// expansion's own scope, which the expander puts on what a template
/// introduces and on nothing that came in through a pattern variable.
#[test]
fn relinking_leaves_the_users_code_inside_a_macro_call_alone() {
    assert_program_eval_to(
        "(define-library (probe both)
           (import (scheme base))
           (export both)
           (begin (define-syntax both (syntax-rules () ((_ e) (list 'template e))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe both))
         (define v (both (list 1 2)))
         (r7:list (r7:pair? v) (r7:pair? (r7:cadr v)))",
        "(#t #f)",
    );
}

/// Inside a quasiquote, `(quote b)` is two symbols of data and an `unquote`
/// within it is still evaluated. The first fix rewrote such a head to the
/// relinked `quote` at any depth (a `quote.N` symbol in the data), and the
/// template compiler had always inserted the quoted datum verbatim, so its
/// `,(helper 7)` — a library-private reference — was never relinked.
#[test]
fn a_quote_inside_a_quasiquote_template_is_data_with_its_unquotes_evaluated() {
    assert_program_eval_to(
        "(define-library (probe qq)
           (import (scheme base))
           (export qq qq2 qq3)
           (begin
             (define (helper x) (* 6 x))
             (define-syntax qq (syntax-rules () ((_ e) `(a (quote b) ,e))))
             (define-syntax qq2 (syntax-rules () ((_ e) `(a '(b ,(helper 7)) ,e))))
             (define-syntax qq3 (syntax-rules () ((_ e) `#(a 'b ,e))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe qq))
         (r7:list (qq 1) (qq2 1) (qq3 1))",
        "((a 'b 1) (a '(b 42) 1) #(a 'b 1))",
    );
}

/// A `quote` that reached a template through a pattern variable and was
/// then relinked is still `quote`: the relinker classifies a head by what
/// it resolves to, not its spelling, so the datum after a `quote.N` head is
/// left alone. Pre-existing — it walked the datum and renamed its `list`.
#[test]
fn a_relinked_quote_head_still_protects_its_datum() {
    assert_program_eval_to(
        "(define-library (probe wq)
           (import (scheme base))
           (export outer)
           (begin
             (define-syntax with-q (syntax-rules () ((_ q) (q (list 1)))))
             (define-syntax outer (syntax-rules () ((_) (with-q quote))))))
         (import (except (scheme base) quote car cons list list?)
                 (prefix (scheme base) r7:)
                 (srfi 101)
                 (probe wq))
         (outer)",
        "(list 1)",
    );
}

/// Symbols under an ellipsis escape are references like any other: a
/// library-private `helper` inside `(... (helper 1))` resolves where the
/// macro was written, and a generated macro's `(tag x ...)` reaches the
/// library's `tag`. The escape compiler used to emit every non-pattern
/// symbol as a bare literal, so nothing under `(... …)` was hygienic; the
/// first occurrence is what the by-spelling relinker had covered by
/// accident, the second never worked.
#[test]
fn references_under_an_ellipsis_escape_resolve_at_the_definition_site() {
    assert_program_eval_to(
        "(define-library (probe esc)
           (import (scheme base))
           (export mk def-tagger)
           (begin
             (define (helper x) (* 5 x))
             (define (tag . xs) (cons 'tagged xs))
             (define-syntax mk
               (syntax-rules ()
                 ((_) (begin (helper 0)
                             (define-syntax g (syntax-rules () ((_) (... (helper 1)))))))))
             (define-syntax def-tagger
               (syntax-rules ()
                 ((_ name) (define-syntax name (... (syntax-rules () ((_ x ...) (tag x ...))))))))))
         (import (scheme base) (probe esc))
         (mk)
         (def-tagger t)
         (list (g) (t 1 2))",
        "(5 (tagged 1 2))",
    );
}

/// A vector object embedded in code — `(eval (list 'outer vec) env)` — is
/// the object the expansion mutates, not a copy. The scope flip walks
/// vectors now (so a quasiquoted `#(,(helper x))` can be relinked) and its
/// first version copied every vector it walked; it copies only one whose
/// elements changed.
#[test]
fn a_vector_object_in_evaluated_code_keeps_its_identity_through_expansion() {
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme repl))
         (define-syntax inner (syntax-rules () ((_ x) (vector-set! x 0 'changed))))
         (define-syntax outer (syntax-rules () ((_ x) (inner x))))
         (define vec (vector 1 2))
         (eval (list 'outer vec) (interaction-environment))
         vec",
        "#(changed 2)",
    );
}

// ---------------------------------------------------------------------------
// Family 40 — the VM resolves a cross-expansion macro-introduced global by
// bare name; the tree-walker follows chibi and refuses it.
//
// Surfaced by the review of the family-36 fix (the fallback that skips a
// rejected binding). One expansion's `(define x …)` introduces a scoped
// top-level definition; a *different* expansion's template reference to that
// spelling carries scopes that reject it. chibi 0.12 errors "undefined
// variable" on every shape below — one expansion's private definition is not
// another expansion's to see — and the tree-walker now agrees. The VM still
// answers: its compiler installs a bare-name alias for a renamed macro-
// introduced global (`alpha_rename`'s `rename_body`), the mechanism whose
// by-name reach Track L §6 already records as undecidable-under-renaming
// (the jabberwocky-steal defect). These quarantines therefore pin the VM as
// the diverging backend; fixing it means fixing relinking-by-name, not
// loosening the tree-walker back to the capture chibi rejects.
// ---------------------------------------------------------------------------

/// Read direction: `use-x`'s template `x` means whatever `x` is at `use-x`'s
/// definition site — and the only `x` there is `def-x`'s hygienically hidden
/// one, which chibi and the tree-walker refuse to let it see.
#[test]
fn one_expansions_definition_is_not_another_expansions_reference() {
    assert_divergence(
        "(define-syntax def-x (syntax-rules () ((_) (define x 10))))
         (def-x)
         (define-syntax use-x (syntax-rules () ((_) x)))
         (use-x)",
        On::Vm,
        "10",
        ErrorClass::AtRuntime,
        "scheme_tests/reports/larceny_triage.md, family 40",
    );
}

/// Write direction, exercising `set_scoped_terminal`'s refusal — the only
/// test that reaches it, since every hygiene-matrix write row's global is a
/// plain `define` the terminal's `local_slot` arm answers first.
#[test]
fn one_expansions_definition_is_not_another_expansions_write_target() {
    assert_divergence(
        "(define-syntax defc (syntax-rules () ((_) (define count 0))))
         (defc)
         (define-syntax inc (syntax-rules () ((_) (set! count (+ count 1)))))
         (inc)
         'done",
        On::Vm,
        "done",
        ErrorClass::AtRuntime,
        "scheme_tests/reports/larceny_triage.md, family 40",
    );
}

/// The generated-getter idiom across two expansions: `defgetter`'s template
/// `priv` resolves at `defgetter`'s definition site, where no visible `priv`
/// exists — `defpriv`'s is hygienically hidden. The R7RS suite's
/// `jabberwocky` shape keeps working because there the `define` and the
/// generated `define-syntax` share one expansion, so the getter's reference
/// carries the defining expansion's scope. `define_scoped_definition`'s doc
/// records the contract boundary this pins.
#[test]
fn a_generated_getter_cannot_see_a_different_expansions_private_define() {
    assert_divergence(
        "(define-syntax defpriv (syntax-rules () ((_) (define priv 10))))
         (define-syntax defgetter
           (syntax-rules () ((_ g) (define-syntax g (syntax-rules () ((_) priv))))))
         (defpriv)
         (defgetter get)
         (get)",
        On::Vm,
        "10",
        ErrorClass::AtRuntime,
        "scheme_tests/reports/larceny_triage.md, family 40",
    );
}
