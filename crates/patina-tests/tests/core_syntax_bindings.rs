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

mod common;
use common::{assert_eval_to, assert_program_eval_error, assert_program_eval_to};

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

/// Lexical shadowing is older than this change and must survive it: local
/// bindings are not in the desugarer's environment at all, so they are handled
/// by `shadowed_names` rather than by the lookup.
#[test]
fn test_local_bindings_still_shadow_keywords() {
    assert_eval_to(
        "(let ((if (lambda (a b c) 'shadowed))) (if 1 2 3))",
        "shadowed",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (define (f begin) (begin 1 2))
         (f list)",
        "(1 2)",
    );
}

// ============================================================================
// Auxiliary syntax
// ============================================================================

/// `else` and `=>` used to be bound as *variables* holding their own symbol,
/// purely so that `(import (only (scheme base) else))` had something to select.
/// The workaround leaked: `(list else)` returned the symbol `else`. They are
/// markers now, so it names what it is.
#[test]
fn test_auxiliary_syntax_is_not_a_symbol() {
    assert_program_eval_to("(import (scheme base)) (list else)", "(#<syntax:else>)");
    assert_program_eval_to("(import (scheme base)) (list =>)", "(#<syntax:=>>)");
}

/// In head position an auxiliary keyword is a mistake, and saying which beats
/// reporting that a symbol is not a procedure — which is what `(else 1)` used
/// to report, via the variable binding.
#[test]
fn test_auxiliary_syntax_in_head_position_is_an_error() {
    assert_program_eval_error("(import (scheme base)) (else 1 2)");
    assert_program_eval_error("(import (scheme base)) (=> 1 2)");
}

/// Their real job is unaffected: `cond` and `case` match `else` and `=>` as
/// `syntax-rules` literals, by name and scopes, without consulting a binding.
#[test]
fn test_auxiliary_syntax_still_works_where_it_belongs() {
    assert_eval_to("(cond (#f 1) (else 'fell-through))", "fell-through");
    assert_eval_to("(cond ((assv 1 '((1 . one))) => cdr) (else 'no))", "one");
    assert_eval_to("(case 9 ((1) 'one) (else 'other))", "other");
}

/// A use site that rebinds `else` must stop it matching, which is the reason
/// R7RS matches literals by binding rather than by spelling.
#[test]
fn test_a_rebound_else_does_not_match() {
    assert_eval_to("(let ((else #f)) (cond (else 1) (#t 2)))", "2");
}

// ============================================================================
// Keywords travel through import sets
// ============================================================================

/// The keyword survives being renamed on the way in *and* reaching the use site
/// under the new name. This is the case the two backends used to answer
/// differently — the VM loaded the library and left `blk` unbound, the
/// tree-walker rejected the library outright, and chibi and Gauche both made it
/// work. Recorded as an open divergence in Track L §6 and deliberately never
/// pinned, because neither of our answers was the right one.
///
/// `sld_file_loading.rs` covers the library-internal form; this is the top
/// level, where both backends already agreed — on binding nothing.
#[test]
fn test_a_renamed_keyword_works_at_top_level() {
    assert_program_eval_to("(import (rename (scheme base) (begin blk))) (blk 1 2)", "2");
}

/// Renaming does not smuggle in a second spelling: `blk` is `begin`, and it is
/// the *same object*, not a copy that merely behaves alike. Markers are
/// interned per heap so that a form has one identity however it was imported.
#[test]
fn test_a_renamed_keyword_is_the_same_object() {
    assert_program_eval_to(
        "(import (scheme base) (rename (scheme base) (begin blk)))
         (eqv? begin blk)",
        "#t",
    );
}

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
// Stage 2 boundary — pinned as still lenient, on purpose
// ============================================================================

/// What is left for stage 2 is exactly one thing: the *bare* spelling still
/// resolves after an import set has excluded or moved it. Deleting the
/// desugarer's spelling fallback is what fixes it, and that can break a program
/// which works today, so it wants its own PR and its own compat run.
///
/// Pinned rather than left untested so that deleting the fallback shows up here
/// as a failing assertion instead of passing unnoticed. chibi and Gauche report
/// `begin` unbound in both programs below.
#[test]
fn test_stage2_except_does_not_yet_hide_a_keyword() {
    assert_program_eval_to("(import (except (scheme base) begin)) (begin 1 2)", "2");
}

#[test]
fn test_stage2_prefix_does_not_yet_hide_the_bare_spelling() {
    assert_program_eval_to("(import (prefix (scheme base) s:)) (begin 1 2)", "2");
}

/// The *other* half of `prefix` needs no stage 2 and is conformant now: a
/// prefixed keyword resolves, because `prefix` copies the marker under the new
/// name and the desugarer dispatches on the form. Composing several is the real
/// check — `s:quote` carries the reader's `'` shorthand, so a prefixed base
/// that got this wrong would fail on the first quoted datum.
#[test]
fn test_a_prefixed_keyword_resolves() {
    assert_program_eval_to("(import (prefix (scheme base) s:)) (s:begin 1 2)", "2");
    assert_program_eval_to(
        "(import (prefix (scheme base) s:))
         (s:let ((x 5)) (s:if x (s:quote yes) (s:quote no)))",
        "yes",
    );
}

/// `(null-environment 5)` still admits R7RS-only forms, for the same reason:
/// `cond-expand` is unbound there, so the spelling fallback claims it. chibi
/// reports an undefined variable.
#[test]
fn test_stage2_null_environment_still_admits_r7rs_forms() {
    assert_program_eval_to(
        "(import (scheme base) (scheme eval) (scheme r5rs))
         (eval '(cond-expand (else 42)) (null-environment 5))",
        "42",
    );
}
