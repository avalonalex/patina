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

mod common;
use common::{assert_program_eval_error, assert_program_eval_to};

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

/// A local binding wins, so a keyword's *spelling* is an ordinary variable once
/// something shadows it. This is the check that has to consult `shadowed_names`
/// rather than the environment alone.
#[test]
fn test_a_shadowed_keyword_is_an_ordinary_variable() {
    assert_program_eval_to("(import (scheme base)) (let ((else 5)) else)", "5");
    assert_program_eval_to("(import (scheme base)) ((lambda (if) if) 7)", "7");
    assert_program_eval_to(
        "(import (scheme base)) (let ((cond 1) (quote 2)) (+ cond quote))",
        "3",
    );
}

/// Quoted data is data. It desugars to a literal and never reaches the check.
#[test]
fn test_quoted_syntax_is_data() {
    assert_program_eval_to("(import (scheme base)) '(if cond else)", "(if cond else)");
    assert_program_eval_to("(import (scheme base)) (car '(if))", "if");
    assert_program_eval_to("(import (scheme base)) `(a ,(+ 1 1) else)", "(a 2 else)");
}

/// Auxiliary syntax still does its real job — matched as a `syntax-rules`
/// literal inside the form that gives it meaning, which never desugars it as an
/// expression.
#[test]
fn test_auxiliary_syntax_still_works_where_it_belongs() {
    assert_program_eval_to("(import (scheme base)) (cond (#f 1) (else 'ok))", "ok");
    assert_program_eval_to(
        "(import (scheme base)) (cond ((assv 1 '((1 . one))) => cdr) (else 'no))",
        "one",
    );
    assert_program_eval_to("(import (scheme base)) (case 9 ((1) 'one) (else 'x))", "x");
}

/// The derived forms are all macros whose expansions the check now sees. Any
/// one of them emitting a keyword in value position would fail here — which is
/// the cheapest guard against this rule being subtly too strict.
#[test]
fn test_the_derived_forms_still_expand() {
    assert_program_eval_to(
        "(import (scheme base)) (do ((i 0 (+ i 1))) ((= i 2) 'done))",
        "done",
    );
    assert_program_eval_to(
        "(import (scheme base)) (let-values (((a b) (values 1 2))) (+ a b))",
        "3",
    );
    assert_program_eval_to(
        "(import (scheme base)) (guard (e (#t 'caught)) (raise 1))",
        "caught",
    );
    assert_program_eval_to(
        "(import (scheme base))
         (define-record-type <p> (mk a) p? (a p-a))
         (p-a (mk 7))",
        "7",
    );
    assert_program_eval_to(
        "(import (scheme base)) (let loop ((i 0)) (if (= i 2) 'done (loop (+ i 1))))",
        "done",
    );
}

/// `apply` is a real procedure binding, not a keyword or a macro, so it stays a
/// value. It is also the one head symbol the desugarer still recognizes by
/// spelling — the check reads the binding, so the two do not interfere.
#[test]
fn test_apply_is_still_a_value() {
    assert_program_eval_to("(import (scheme base)) (procedure? apply)", "#t");
    assert_program_eval_to("(import (scheme base)) (let ((f apply)) (f + '(1 2)))", "3");
}

// ============================================================================
// The rule reads bindings, not spellings
// ============================================================================

/// The whole design in one program: rebind the name and the check stops firing,
/// because there is no longer any syntax there to misuse.
///
/// R7RS §5.3.1 makes the `define` case normative — "if ⟨variable⟩ is not bound,
/// *or is a syntactic keyword*, then the definition will bind ⟨variable⟩ to a
/// new location". Gauche agrees on both; chibi rejects the `set!` itself.
#[test]
fn test_rebinding_a_keyword_makes_it_a_value() {
    assert_program_eval_to("(import (scheme base)) (define if 5) (+ if 1)", "6");
    assert_program_eval_to("(import (scheme base)) (set! if 5) (+ if 1)", "6");
    // And the rebound name is an ordinary procedure position, too.
    assert_program_eval_to(
        "(import (scheme base)) (define (if a b) (list 'proc a b)) (if 1 2)",
        "(proc 1 2)",
    );
}

/// A keyword that was never imported is an unbound variable, not a misuse of
/// syntax — so the two failures stay distinguishable. Both are errors; what
/// matters is that the check does not claim a name it knows nothing about.
#[test]
fn test_an_unimported_keyword_is_merely_unbound() {
    assert_program_eval_error(
        r#"
        (define-library (val nokeyword)
          (import (patina internal lists))
          (export go)
          (begin (define (go) (list cond))))
        (import (val nokeyword))
        (go)
        "#,
    );
}
