//! Pins `CoreForm`'s dispatching/auxiliary split against the desugarer, and
//! pins that a keyword is recognized *only* through a binding.
//!
//! This file has been narrowed twice. It began as a check that a hand-written
//! `&[&str]` in `patina-runtime` matched the desugarer's `match` arms, because
//! a list of another crate's arms drifts silently in both directions. That list
//! is gone: the desugarer matches exhaustively on `CoreForm`, so the compiler
//! enforces it.
//!
//! What the compiler still cannot check is the *classification*.
//! `CoreForm::is_dispatching` is hand-written, and being wrong about a form is
//! silent in both directions — a dispatching form misfiled as auxiliary becomes
//! an error at every use, and an auxiliary form misfiled as dispatching reaches
//! a `desugar_*` method with no idea what to do with it.
//!
//! Stage 2 added the second property. There is no spelling fallback any more,
//! so "is this syntax?" has exactly one answer: what the environment says.
//! `keywords_are_not_recognized_without_a_binding` is the regression guard for
//! that, and it is the test that would have failed at any point before stage 2.

use patina_core::{ALL_CORE_FORMS, SharedHeap, TaggedValue};
use patina_frontend::{Desugarer, Parser};
use patina_ir::CoreExprKind;
use patina_runtime::Environment;
use std::rc::Rc;

fn parse_tv(code: &str) -> (TaggedValue, SharedHeap) {
    let mut parser = Parser::new(code).expect("parser creation failed");
    let tv = parser.parse().expect("parse failed");
    (tv, parser.heap().clone())
}

/// How `(<name> a b)` desugars.
enum Head {
    /// An application of a variable called `name` — an ordinary identifier.
    CallOfItself,
    /// Claimed by the desugarer as a form.
    Intercepted,
    /// Rejected outright.
    Rejected,
}

fn desugar_head(desugarer: &Desugarer, name: &str) -> Head {
    // Two arguments keep every form that takes any past its arity check, so a
    // rejection here means "refused as syntax", never "treated as a call".
    let (tv, heap) = parse_tv(&format!("({} a b)", name));
    let Ok(expr) = desugarer.desugar_tagged(tv, &heap) else {
        return Head::Rejected;
    };
    match &expr.kind {
        CoreExprKind::App { func, .. } if matches!(&func.kind, CoreExprKind::Var { name: n, .. } if n.as_ref() == name) => {
            Head::CallOfItself
        }
        _ => Head::Intercepted,
    }
}

/// `Desugarer::new()` binds the keywords and nothing else — the desugarer's
/// `(null-environment)`.
#[test]
fn every_dispatching_form_is_intercepted_when_bound() {
    let desugarer = Desugarer::new();
    for form in ALL_CORE_FORMS.iter().filter(|f| f.is_dispatching()) {
        let name = form.name();
        assert!(
            !matches!(desugar_head(&desugarer, name), Head::CallOfItself),
            "`{name}` is classified as dispatching, but the desugarer compiles `({name} a b)` as \
             an ordinary call — so exporting it from a library would be accepted and then fail at \
             the use site. Either add the form to the desugarer, or mark it auxiliary."
        );
    }
}

/// The other half of the classification. An auxiliary keyword means something
/// only inside an enclosing form, so in head position it is a mistake — and
/// now that it is *bound*, the desugarer can say so instead of emitting a call
/// to an unbound name.
#[test]
fn every_auxiliary_form_is_rejected_when_bound() {
    let desugarer = Desugarer::new();
    for form in ALL_CORE_FORMS.iter().filter(|f| !f.is_dispatching()) {
        let name = form.name();
        assert!(
            matches!(desugar_head(&desugarer, name), Head::Rejected),
            "`{name}` is classified as auxiliary, so `({name} a b)` should be refused as a \
             misplaced keyword. Either it grew a meaning in head position — in which case mark \
             it dispatching and give it an arm — or the classification is wrong."
        );
    }
}

/// **The stage-2 property.** With nothing bound, a keyword is an ordinary
/// identifier: `(begin a b)` compiles to a call of a variable named `begin`.
///
/// Before stage 2 the desugarer recognized keywords by spelling wherever they
/// were unbound, so every one of these would have been intercepted. That
/// fallback is what made `(except (scheme base) begin)` mean nothing inside a
/// library, and deleting it is what this test guards.
///
/// It uses a bare `Environment::new()` rather than `Desugarer::new()`, which
/// seeds the keywords on purpose.
#[test]
fn keywords_are_not_recognized_without_a_binding() {
    let desugarer = Desugarer::with_env(Rc::new(Environment::new()));
    for form in ALL_CORE_FORMS {
        let name = form.name();
        assert!(
            matches!(desugar_head(&desugarer, name), Head::CallOfItself),
            "`{name}` was claimed as syntax by a desugarer whose environment binds nothing. \
             Keywords are recognized through bindings only — a spelling fallback has come back."
        );
    }
}

/// The control: without it, the interception assertions pass for any string
/// that merely fails to desugar, which is most of them.
#[test]
fn an_ordinary_identifier_compiles_to_a_call_of_itself() {
    let desugarer = Desugarer::new();
    assert!(matches!(
        desugar_head(&desugarer, "frobnicate"),
        Head::CallOfItself
    ));
    // Near-misses of real entries, to catch a typo in the table rather than a
    // genuinely absent form.
    assert!(matches!(
        desugar_head(&desugarer, "beign"),
        Head::CallOfItself
    ));
    assert!(matches!(
        desugar_head(&desugarer, "qoute"),
        Head::CallOfItself
    ));
}

/// `apply` is the one desugarer arm deliberately left out of `CoreForm`: it is
/// special-cased *and* a real procedure binding. It is also the last head
/// symbol recognized by spelling — so unlike every keyword above, it is
/// intercepted even with nothing bound.
#[test]
fn apply_is_excluded_on_purpose() {
    assert!(patina_core::CoreForm::from_name("apply").is_none());
    let empty = Desugarer::with_env(Rc::new(Environment::new()));
    assert!(
        matches!(desugar_head(&empty, "apply"), Head::Intercepted),
        "`apply` is still spelling-recognized; if that changed, update this test and the note in \
         `desugar_list_tagged`"
    );
}
