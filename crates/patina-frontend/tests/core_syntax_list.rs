//! Pins `patina-runtime`'s `DESUGARED_FORMS` against the desugarer it claims
//! to describe.
//!
//! The list exists because these keywords have no binding anywhere, so a
//! library re-exporting one has nothing to resolve (Track L; `build_library`
//! in `patina-runtime`). A hand-kept list of another crate's `match` arms
//! drifts silently in both directions — a new special form the list misses
//! makes `(export it)` fail again, and a name the desugarer does not handle
//! makes `(export it)` succeed and then break at the *use* site. Neither
//! shows up in a test that only asks the list about itself.
//!
//! The observable property: the desugarer intercepts a core form in head
//! position instead of compiling a call to a variable of that name.

use patina_core::{SharedHeap, TaggedValue};
use patina_frontend::{Desugarer, Parser};
use patina_ir::CoreExprKind;

fn parse_tv(code: &str) -> (TaggedValue, SharedHeap) {
    let mut parser = Parser::new(code).expect("parser creation failed");
    let tv = parser.parse().expect("parse failed");
    (tv, parser.heap().clone())
}

/// Does `(<name> …)` compile to an application of a variable called `name`?
/// True for an ordinary identifier, false for anything the desugarer claims.
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
fn every_desugared_form_is_intercepted_by_the_desugarer() {
    for name in patina_runtime::library_loader::DESUGARED_FORMS {
        assert!(
            !compiles_to_a_call_of_itself(name),
            "DESUGARED_FORMS lists `{name}`, but the desugarer compiles `({name} a b)` as an \
             ordinary call — so exporting it from a library would be accepted and then fail at \
             the use site. Remove it from the list, or add the form to the desugarer."
        );
    }
}

/// The control: without it, the assertion above passes for any string that
/// merely fails to desugar, which is most of them.
#[test]
fn an_ordinary_identifier_compiles_to_a_call_of_itself() {
    assert!(compiles_to_a_call_of_itself("frobnicate"));
    // Near-misses of real entries, to catch a typo in the list rather than a
    // genuinely absent form.
    assert!(compiles_to_a_call_of_itself("beign"));
    assert!(compiles_to_a_call_of_itself("qoute"));
}

/// `apply` is the one desugarer arm deliberately left out of the list: it is
/// special-cased *and* a real procedure binding, so it resolves the ordinary
/// way and needs no exemption when exported.
#[test]
fn apply_is_excluded_on_purpose() {
    assert!(!patina_runtime::library_loader::is_core_syntax("apply"));
}

/// The other half of the list is not the desugarer's, and must not be checked
/// against it: these mean something only inside a macro transformer, and in
/// head position they are ordinary unbound variables.
#[test]
fn syntax_rules_keywords_are_not_desugarer_forms() {
    for name in patina_runtime::library_loader::SYNTAX_RULES_KEYWORDS {
        assert!(
            !patina_runtime::library_loader::DESUGARED_FORMS.contains(name),
            "`{name}` is in both lists; each name belongs to exactly one dispatch site"
        );
    }
}
