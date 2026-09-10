//! Original test cases for the families of defects that Larceny's R7RS test
//! suites surfaced (Track L §L5.3, 2026-08-24).
//!
//! **Being redistributed, not kept.** #193 Phase 1 is moving these rows to the
//! file about their *subject* rather than about where they were found — see
//! `docs/TEST_ORGANIZATION.md`'s section on it. 64 rows have become 3; the
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
//! **What is left is family 40 alone** — three rows where the two backends
//! genuinely answer differently, each through `assert_divergence`. They are
//! the one thing in this file a suite file could never hold, since a `.scm`
//! row runs on both backends and asserts one value. Their home is
//! `backend_divergence.rs`, which is the registry for exactly this, and moving
//! them is what finally deletes this file and its binary.
//!
//! Two pinning conventions this file used to carry are gone with the rows that
//! used them, and are recorded here because the shapes recur: **a wrong answer
//! both backends agree on** was asserted as-is with a note saying what to do
//! when it converged — family 37 was the last, and its note outlived its own
//! defect by two weeks, which is the argument for SRFI 64's `test-expect-fail`
//! and the right answer instead (see `docs/TEST_ORGANIZATION.md`); and **a
//! crash or hang**, which cannot be asserted at all, carried the correct
//! expectation under `#[ignore]`. No row here is ignored today.

mod common;

use common::{ErrorClass, On, assert_divergence};

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
