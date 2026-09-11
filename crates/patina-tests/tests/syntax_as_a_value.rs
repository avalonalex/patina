//! Syntax is not a value, and Patina now says so.
//!
//! R7RS puts syntactic keywords and variables in disjoint categories (§3.1),
//! and its `⟨expression⟩` grammar (§7.1.3) admits only the latter — so
//! `(procedure? if)` is not a well-formed procedure call at all. But the report
//! writes this as a plain "it is an error", which §1.3.2 defines as *not*
//! requiring detection: "implementations are not required to detect or report
//! the error, though they are encouraged to do so."
//!
//! **So this file pins a choice, not a conformance fix.** Chez 10.4.1 and
//! chibi 0.12 raise; Gauche 0.9.15 returns an opaque object, which is what
//! Patina used to do. A scheme-reports thread on syntax objects records that an
//! implementation may "accept it and initialize the variable with some object
//! whose properties are not specified by R7RS" — which blesses the old
//! behaviour too. Both answers are legal; if this is ever reversed, reverse it
//! knowingly, and update `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md` §2.5.
//!
//! The rule is applied to **macros as well as keywords**. Doing keywords alone
//! would rebuild the very split this work removed — before it, `(procedure? if)`
//! errored and `(procedure? cond)` returned `#f`, which was an accident of `if`
//! having had no binding to load.
//!
//! **It is a desugar-time rule, which is why this file is Rust.** A rejection
//! happens before the program runs, so no `guard` inside the program can see
//! its own form refused; the rows that show the rule *accepting* things are a
//! suite file, `tests/scheme/expansion/keyword-bindings.scm`.
//!
//! **It has recorded residuals**, all pinned at the bottom of this file. The check asks what a name resolves to *while the
//! form is being desugared*, and what the desugarer knows then is not
//! everything: a name that is not syntax yet still loads `#<macro>` or
//! `#<syntax:…>` as a value.
//!
//! That one is a missed rejection. The third is not, and is why this
//! paragraph no longer claims the rule "never" rejects wrongly: a keyword a
//! *macro expansion* rebinds is invisible to the body scan that
//! `body_definition_names` performs, so the check still sees the core binding
//! and rejects a legal program. Written-out internal definitions are seen
//! (audit C2); expansions are not. Closing any of the three means resolving
//! later — at every variable *read*, on the VM's hottest instruction, or after
//! expanding a body's macro uses. The residual is the price; it is written
//! down rather than implied.

mod common;
use common::{
    ErrorClass, assert_program_eval_error, assert_program_eval_error_at, assert_program_eval_to,
};

// ============================================================================
// Rejected
// ============================================================================

/// The keywords, in the position the report's grammar excludes them from.
#[test]
fn test_a_syntactic_keyword_is_not_a_value() {
    for expr in [
        "(procedure? if)",
        "(list quote)",
        "(define x lambda)",
        "(car (list define))",
        // Auxiliary syntax too. `else` used to answer `(else)` here, because
        // `base.sld` bound it to the symbol `'else` to get it through an
        // import set — the workaround this design retired.
        "(list else)",
        "(list =>)",
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {expr}"));
    }
}

/// A keyword unquoted into a quasiquote template is the same rejection,
/// reached through the one path that used to *panic* instead (audit A1).
///
/// The unquote is desugared by whoever expands the template, and until this
/// was fixed neither expander could say so: the VM's
/// `compiler/quasiquote_expand.rs` panicked outright — a process abort from
/// `` `(1 ,if) `` — and the tree-walker's `cps_eval/quasiquote.rs` wrapped the
/// diagnostic in `InternalError`, which reads as an interpreter bug rather
/// than the program error it is.
///
/// The stages differ and always will: the VM expands templates while
/// compiling, the tree-walker while evaluating. The diagnostic is the part
/// that has to match, and does.
#[test]
fn test_a_keyword_unquoted_into_a_template_is_not_a_value() {
    for (expr, expected) in [
        ("`(1 ,if)", "`if` is a syntactic keyword"),
        ("`(1 ,@if)", "`if` is a syntactic keyword"),
        ("`(1 ,when)", "`when` is a macro"),
        ("`(a . ,if)", "`if` is a syntactic keyword"),
        ("`#(1 ,if)", "`if` is a syntactic keyword"),
        ("`(1 `(2 ,,if))", "`if` is a syntactic keyword"),
    ] {
        assert_program_eval_error_at(
            &format!("(import (scheme base)) {expr}"),
            ErrorClass::AtRuntime,
            ErrorClass::BeforeRun,
            expected,
        );
    }
}

/// Macros answer the same way, which is the point: `if` and `cond` are one
/// rule, not two.
#[test]
fn test_a_macro_is_not_a_value_either() {
    for expr in [
        "(procedure? cond)",
        "(list when)",
        "(define x case)",
        // A user-defined macro, so this is not about which names are built in.
        "(begin (define-syntax m (syntax-rules () ((_) 1))) (list m))",
    ] {
        assert_program_eval_error(&format!("(import (scheme base)) {expr}"));
    }
}

// ============================================================================
// Not rejected — the false positives this could easily have had
// ============================================================================
//
// Moved to `tests/scheme/expansion/keyword-bindings.scm` (#193 Phase 2): a
// shadowed keyword, an internal definition over one, `define-syntax` through
// its binding, quoted keywords, the derived forms, `apply` as a value. They
// are what the rule proved *works*, which a portable program can observe and
// chibi and Gauche can arbitrate — all 14 rows agree on all four
// implementations. What stays here is what a program cannot observe from
// inside: a rejection the desugarer makes before the program runs, a keyword
// rebound at the top level, and the residuals.

// That `else` and `=>` still work inside `cond` and `case` — matched as
// `syntax-rules` literals, never desugared as expressions — is covered on both
// backends by `compliance/derived.rs` (`test_cond_with_else`,
// `test_cond_with_arrow`, `test_case_with_else`). `core_syntax_bindings.rs`
// says in as many words that it does not restate them; neither does this file.

// ============================================================================
// The rule reads bindings, not spellings
// ============================================================================

/// `define` over a keyword is licensed and `set!` over one is not, and the
/// asymmetry is the report's, not ours.
///
/// R7RS §5.3.1 is normative for the first — "if ⟨variable⟩ is not bound, *or is
/// a syntactic keyword*, then the definition will bind ⟨variable⟩ to a new
/// location" — and says nothing of the sort for `set!`, whose ⟨variable⟩ must
/// already be one (§4.1.6, §3.1). chibi draws the line in exactly that place,
/// rejecting `(set! if 5)` with the same message it gives for `(list if)`.
/// Gauche accepts it and then breaks inside its own startup code, which is the
/// argument against following it.
///
/// (`(define (if a b) …)` in *head* position is
/// `core_syntax_bindings.rs::test_define_shadows_a_syntactic_keyword`; what is
/// new here is the value half.)
#[test]
fn test_define_may_rebind_a_keyword_but_set_may_not() {
    assert_program_eval_to("(import (scheme base)) (define if 5) (+ if 1)", "6");
    assert_program_eval_error("(import (scheme base)) (set! if 5)");
    assert_program_eval_error("(import (scheme base)) (set! cond 5)");
}

/// A keyword that was never imported is an unbound variable, not a misuse of
/// syntax — the check must not claim a name it knows nothing about.
///
/// The first version of this test imported `(patina internal lists)`, which
/// exports no `define`, so the library failed on its own first line and never
/// reached `cond` at all. `assert_program_eval_error` accepts any error, so it
/// passed while testing nothing. Importing exactly `define` and `list` is what
/// makes `cond` the only thing that can fail.
#[test]
fn test_an_unimported_keyword_is_merely_unbound() {
    assert_program_eval_error(
        r#"
        (define-library (val nokeyword)
          (import (only (scheme base) define list))
          (export go)
          (begin (define (go) (list cond))))
        (import (val nokeyword))
        (go)
        "#,
    );
}

// ============================================================================
// Residuals — the rule is a desugar-time snapshot
// ============================================================================

/// A name that is not syntax *yet* still loads as a value.
///
/// The check reads the environment as it stands while the enclosing form is
/// desugared, and returns "not syntax" on a lookup miss — which is right, or an
/// ordinary forward reference to a procedure would be an error. The cost is
/// that `#<macro>`, the value this rule exists to abolish, is still reachable
/// through a forward reference. chibi reports `undefined variable: foo` here,
/// because it resolves at the *read*.
///
/// Pinned rather than fixed: closing it means checking on every variable read,
/// which is `LoadGlobal` on the VM — and the per-site inline cache stores a
/// slot, so a fill-time check would be unsound and a per-load tag test would
/// sit on the hottest instruction in the interpreter. Recorded so the
/// `#<macro>` formatting in `datum_writer.rs` is not mistaken for dead code.
#[test]
fn test_a_forward_reference_to_syntax_is_not_caught() {
    assert_program_eval_to(
        "(import (scheme base))
         (define (f) (list foo))
         (define-syntax foo (syntax-rules () ((_) 1)))
         (f)",
        "(#<macro>)",
    );
}

/// A definition a macro *expands into* is not seen by the body scan.
///
/// `body_definition_names` reads the forms as written, before expansion, so a
/// keyword bound by `define-record-type` or `define-values` is invisible to
/// it. Same coarseness as the spelling test below and in the same direction —
/// a missed shadow, never an invented one — but it costs a legal program here
/// rather than a missed rejection, so it is the sharper of the two residuals.
/// chibi accepts this.
///
/// Closing it means expanding the body's macro uses before scanning it, which
/// reorders expansion against definition and is a change to when hygiene
/// happens, not a bug fix.
#[test]
fn test_a_definition_introduced_by_a_macro_is_not_seen_by_the_body_scan() {
    assert_program_eval_error(
        "(import (scheme base))
         (define (f) (define-values (if) (values 3)) (+ if 1))
         (f)",
    );
}

/// A binding that merely shares a spelling no longer suppresses the check.
///
/// The template binds a *scoped* `cond`; the reference is the use site's own,
/// which hygiene says that binding does not capture — so it is the global
/// macro, and naming it as a value is the error this file is about. The
/// spelling match in `shadowed_names` used to suppress the answer and the
/// program evaluated to `#<macro>`.
///
/// Fixed 2026-08-27 with Larceny triage family 37, which binds each name at
/// the scopes it was *written* with plus the form's: the template's `cond` is
/// bound where the template stands, and the use site's reference does not
/// reach it. chibi rejects this program too.
#[test]
fn test_a_shadowed_spelling_no_longer_suppresses_the_check() {
    assert_program_eval_error(
        "(import (scheme base))
         (define-syntax m (syntax-rules () ((_ body) (let ((cond 5)) body))))
         (m cond)",
    );
}
