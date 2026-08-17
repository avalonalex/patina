//! Pins `CoreForm`'s dispatching/auxiliary split against the desugarer.
//!
//! This file used to check a hand-written `&[&str]` in `patina-runtime` against
//! the desugarer's `match` arms, because a list of another crate's arms drifts
//! silently in both directions. Both lists are gone: the desugarer now matches
//! exhaustively on `CoreForm`, so the compiler enforces what half of this file
//! was invented to check.
//!
//! What the compiler still cannot check is the *classification*.
//! `CoreForm::is_dispatching` is hand-written, and being wrong about a form is
//! silent in both directions — a dispatching form misfiled as auxiliary becomes
//! an error at every use, and an auxiliary form misfiled as dispatching reaches
//! a `desugar_*` method that has no idea what to do with it.
//!
//! The observable property: a dispatching form is intercepted in head position
//! instead of compiling to a call to a variable of that name, and an auxiliary
//! form is not.

use patina_core::{ALL_CORE_FORMS, SharedHeap, TaggedValue};
use patina_frontend::{Desugarer, Parser};
use patina_ir::CoreExprKind;

fn parse_tv(code: &str) -> (TaggedValue, SharedHeap) {
    let mut parser = Parser::new(code).expect("parser creation failed");
    let tv = parser.parse().expect("parse failed");
    (tv, parser.heap().clone())
}

/// Does `(<name> …)` compile to an application of a variable called `name`?
/// True for an ordinary identifier, false for anything the desugarer claims.
///
/// `Desugarer::new()` carries no environment, so this exercises the stage-1
/// spelling fallback — which is exactly the path that must agree with
/// `is_dispatching`, since that is what the fallback consults.
fn compiles_to_a_call_of_itself(name: &str) -> bool {
    // Two arguments keep every form that takes any past its arity check, so a
    // failure here means "rejected as syntax", never "treated as a call".
    let (tv, heap) = parse_tv(&format!("({} a b)", name));
    let Ok(expr) = Desugarer::new().desugar_tagged(tv, &heap) else {
        return false;
    };
    let CoreExprKind::App { func, .. } = &expr.kind else {
        return false;
    };
    matches!(&func.kind, CoreExprKind::Var { name: n, .. } if n.as_ref() == name)
}

#[test]
fn every_dispatching_form_is_intercepted_by_the_desugarer() {
    for form in ALL_CORE_FORMS.iter().filter(|f| f.is_dispatching()) {
        let name = form.name();
        assert!(
            !compiles_to_a_call_of_itself(name),
            "`{name}` is classified as dispatching, but the desugarer compiles `({name} a b)` as \
             an ordinary call — so exporting it from a library would be accepted and then fail at \
             the use site. Either add the form to the desugarer, or mark it auxiliary."
        );
    }
}

/// The other half of the classification, and the reason it is a separate test:
/// an auxiliary keyword means something only inside an enclosing form, so with
/// no binding in scope it is an ordinary unbound variable — the desugarer's
/// fallback must *not* claim it. (Bound, it is an error in head position; that
/// is `core_syntax_bindings.rs` in `patina-tests`, which needs a real
/// environment.)
#[test]
fn no_auxiliary_form_is_intercepted_by_the_fallback() {
    for form in ALL_CORE_FORMS.iter().filter(|f| !f.is_dispatching()) {
        let name = form.name();
        assert!(
            compiles_to_a_call_of_itself(name),
            "`{name}` is classified as auxiliary, but the desugarer's spelling fallback claims \
             `({name} a b)`. Auxiliary keywords have no meaning in head position and no binding \
             to dispatch on, so the fallback must leave them alone."
        );
    }
}

/// The control: without it, the first assertion above passes for any string
/// that merely fails to desugar, which is most of them.
#[test]
fn an_ordinary_identifier_compiles_to_a_call_of_itself() {
    assert!(compiles_to_a_call_of_itself("frobnicate"));
    // Near-misses of real entries, to catch a typo in the table rather than a
    // genuinely absent form.
    assert!(compiles_to_a_call_of_itself("beign"));
    assert!(compiles_to_a_call_of_itself("qoute"));
}

/// `apply` is the one desugarer arm deliberately left out of `CoreForm`: it is
/// special-cased *and* a real procedure binding, so it resolves the ordinary
/// way and needs no marker.
#[test]
fn apply_is_excluded_on_purpose() {
    assert!(!patina_runtime::library_loader::is_core_syntax("apply"));
    assert!(patina_core::CoreForm::from_name("apply").is_none());
}
