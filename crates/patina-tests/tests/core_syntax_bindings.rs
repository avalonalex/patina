//! Syntactic keywords are bindings, not spellings.
//!
//! `begin`, `if`, `lambda` and 20 others used to be recognized by name, in
//! every scope, unconditionally — they had no entry in any environment. Import
//! sets and export resolution are binding-based, so neither could reach them,
//! and the consequences showed up in six unrelated-looking places. These pin
//! the ones stage 1 closes. What stage 2 still owes is narrower than the design
//! predicted — only that a *bare* spelling resolves after an import set has
//! excluded or moved it — and it is pinned as *still lenient* below, so that
//! stage flipping it is a visible change rather than a silent one.
//!
//! Design and staging: `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`.
//! Every helper here runs both backends.
//!
//! **Three rows moved** to `tests/scheme/expansion/keyword-bindings.scm` (#193
//! Phase 2) — local bindings shadowing a keyword, a rebound `else`, and a
//! keyword renamed on import — where chibi and Gauche answer them too. What
//! stays rebinds a keyword at the top level, builds a `define-library`, or
//! imports at the top level, none of which a shared suite file can do without
//! changing every row after it.

mod common;
use common::{assert_program_eval_error, assert_program_eval_to};

// ============================================================================
// A definition shadows a keyword (R7RS §5.3.1)
// ============================================================================

/// R7RS §5.3.1 is normative and names this exact case: "if ⟨variable⟩ is not
/// bound, *or is a syntactic keyword*, then the definition will bind
/// ⟨variable⟩ to a new location before performing the assignment."
///
/// Patina used to answer `2` here, because the desugarer's name `match` claimed
/// the form before anything asked what `if` was bound to.
#[test]
fn test_define_shadows_a_syntactic_keyword() {
    assert_program_eval_to(
        "(import (scheme base))
         (define (if a b c) (list 'proc a b c))
         (if 1 2 3)",
        "(proc 1 2 3)",
    );
}

/// The half that always worked, kept beside it: the two must not disagree
/// again. A macro binding won because macros *were* looked up; that asymmetry
/// is the whole diagnosis.
#[test]
fn test_define_syntax_also_shadows_a_syntactic_keyword() {
    assert_program_eval_to(
        "(import (scheme base))
         (define-syntax if (syntax-rules () ((_ a b c) (list 'mymacro a b c))))
         (if 1 2 3)",
        "(mymacro 1 2 3)",
    );
}

// ============================================================================
// Auxiliary syntax
// ============================================================================

/// In head position an auxiliary keyword is a mistake, and saying which beats
/// reporting that a symbol is not a procedure — which is what `(else 1)` used
/// to report, via the variable binding.
#[test]
fn test_auxiliary_syntax_in_head_position_is_an_error() {
    assert_program_eval_error("(import (scheme base)) (else 1 2)");
    assert_program_eval_error("(import (scheme base)) (=> 1 2)");
}

// That `else` and `=>` still do their real job — matching as `syntax-rules`
// literals inside `cond` and `case` — is already covered on both backends by
// `tests/scheme/expansion/derived-forms.scm` ("cond falls through to else",
// "cond's => passes the test's value", "case falls through to else"). Not
// restated here: those are the regression guards
// for `cond`/`case`, and a second copy only splits the failure across two
// files.

// ============================================================================
// Keywords travel through import sets
// ============================================================================

// A renamed keyword is the *same* interned marker, not a copy: pinned by
// `core_syntax_is_interned_per_heap` in `patina-core`, which is where it moved
// when `(eqv? begin blk)` stopped evaluating.

/// `only` selecting a keyword works because there is now something to select.
/// It used to *reject the program* — "Identifier 'begin' not found in import
/// set" — which is what the first consumer of a blanket re-export does.
#[test]
fn test_only_can_select_a_keyword() {
    assert_program_eval_to(
        "(import (only (scheme base) begin quote list) (scheme write))
         (begin (list 1 2))",
        "(1 2)",
    );
}

// ============================================================================
// Stage 2 — an import set scopes keywords like any other binding
// ============================================================================

/// The keyword form of the rule stage 2 exists for: a library gets syntax only
/// by importing it. `begin` is excluded here, so the body cannot use it — which
/// is chibi's and Gauche's answer, and was impossible to reach while the
/// desugarer recognized keywords by spelling wherever they were unbound.
#[test]
fn test_except_hides_a_keyword_from_a_library() {
    assert_program_eval_error(
        r#"
        (define-library (syn ex)
          (import (except (scheme base) begin))
          (export go)
          (begin (define (go) (begin 1 2))))
        (import (syn ex))
        (go)
        "#,
    );
}

/// And `prefix` moves it rather than duplicating it: the prefixed name works
/// (that much was true from stage 1) and the bare one no longer does.
#[test]
fn test_prefix_moves_a_keyword_out_of_the_bare_spelling() {
    assert_program_eval_to(
        r#"
        (define-library (syn pfx)
          (import (prefix (scheme base) s:))
          (export go)
          ;; The outer `begin` is a library declaration, which the .sld parser
          ;; reads structurally and no import set touches. Everything inside it
          ;; is ordinary code and must use the prefixed names.
          (begin (s:define (go) (s:begin 1 2))))
        (import (syn pfx))
        (go)
        "#,
        "2",
    );
    assert_program_eval_error(
        r#"
        (define-library (syn pfx2)
          (import (prefix (scheme base) s:))
          (export go)
          (begin (s:define (go) (begin 1 2))))
        (import (syn pfx2))
        (go)
        "#,
    );
}

/// `(null-environment 5)` is null now. It used to admit any R7RS form the
/// desugarer knew by spelling — `cond-expand`, `include`, `import`,
/// `syntax-error` — because none of them was bound there to be left out.
#[test]
fn test_null_environment_admits_only_r5rs_syntax() {
    assert_program_eval_error(
        "(import (scheme base) (scheme eval) (scheme r5rs))
         (eval '(cond-expand (else 42)) (null-environment 5))",
    );
    // The R5RS keywords it *should* have still work, so the environment is
    // narrowed rather than empty.
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme r5rs) (scheme write))
         (eval '(begin 1 2) (null-environment 5))",
        "2",
    );
}

// ============================================================================
// The top level is not covered by any of the above, and not because of keywords
// ============================================================================

/// At the top level an import set does not remove anything, because
/// `load_bootstrap` seeds every `(scheme base)` export into the global
/// environment before a program runs — that is what lets a script with no
/// `import` at all work, which Patina supports and chibi does not.
///
/// **Not a keyword defect**, which is the point of asserting `car` beside
/// `begin`: an ordinary procedure survives `except` exactly the same way. The
/// design doc listed the top-level `(except … begin)` case under stage 2, and
/// that was a misattribution — deleting the spelling fallback fixed the library
/// case above and could not have fixed this one. Its fix is to stop pre-seeding
/// the global environment, which is a deliberate REPL affordance and a separate
/// decision.
#[test]
fn test_a_top_level_import_set_removes_nothing_keyword_or_not() {
    assert_program_eval_to("(import (except (scheme base) begin)) (begin 1 2)", "2");
    assert_program_eval_to(
        "(import (except (scheme base) car) (scheme write)) (car (list 1 2))",
        "1",
    );
}
