//! Tests for Pattern and Template types
//!
//! These tests verify the core pattern and template structures used in macro expansion.

use super::{Identifier, Pattern, Template};
use patina_runtime::{PVRef, ScopeSet};

// ============================================================================
// Pattern tests
// ============================================================================

#[test]
fn test_pattern_wildcard() {
    let pat = Pattern::Wildcard;
    assert!(pat.is_wildcard());
    assert!(!pat.is_var());
    assert_eq!(format!("{}", pat), "_");
}

#[test]
fn test_pattern_var() {
    let pvref = PVRef::new(0, 0);
    let pat = Pattern::Var(pvref);
    assert!(pat.is_var());
    assert_eq!(pat.as_var(), Some(pvref));
    assert_eq!(format!("{}", pat), "?PVRef(level=0, index=0)");
}

#[test]
fn test_pattern_list() {
    let pat = Pattern::List(vec![
        Pattern::Var(PVRef::new(0, 0)),
        Pattern::Var(PVRef::new(0, 1)),
    ]);
    assert_eq!(
        format!("{}", pat),
        "(?PVRef(level=0, index=0) ?PVRef(level=0, index=1))"
    );
}

#[test]
fn test_pattern_ellipsis() {
    let pat = Pattern::Ellipsis {
        subpattern: Box::new(Pattern::Var(PVRef::new(1, 0))),
        level: 1,
        num_following: 2,
        vars: vec![PVRef::new(1, 0)],
    };
    assert!(pat.is_ellipsis());
    assert!(format!("{}", pat).contains("@level=1"));
    assert!(format!("{}", pat).contains("following=2"));
}

// ============================================================================
// Template tests
// ============================================================================

#[test]
fn test_template_literal() {
    let tmpl = Template::Literal(patina_core::TaggedValue::fixnum(42));
    assert!(tmpl.is_literal());
    assert!(format!("{}", tmpl).contains("literal"));
}

#[test]
fn test_template_symbol() {
    let tmpl = Template::Symbol(Identifier::new("if"));
    assert!(tmpl.is_symbol());
    assert_eq!(format!("{}", tmpl), "if");
}

#[test]
fn test_template_var() {
    let pvref = PVRef::new(0, 0);
    let tmpl = Template::Var(pvref);
    assert!(tmpl.is_var());
    assert_eq!(tmpl.as_var(), Some(pvref));
    assert_eq!(format!("{}", tmpl), "$PVRef(level=0, index=0)");
}

#[test]
fn test_template_list() {
    let tmpl = Template::List(vec![
        Template::Symbol(Identifier::new("if")),
        Template::Var(PVRef::new(0, 0)),
        Template::Var(PVRef::new(0, 1)),
    ]);
    assert_eq!(
        format!("{}", tmpl),
        "(if $PVRef(level=0, index=0) $PVRef(level=0, index=1))"
    );
}

#[test]
fn test_template_ellipsis_single() {
    let tmpl = Template::Ellipsis {
        subtemplate: Box::new(Template::Var(PVRef::new(1, 0))),
        level: 1,
        nesting: 1,
        vars: vec![PVRef::new(1, 0)],
    };
    assert!(tmpl.is_ellipsis());
    assert!(format!("{}", tmpl).contains("@level=1"));
    assert!(format!("{}", tmpl).contains("nesting=1"));
}

#[test]
fn test_template_ellipsis_double() {
    // Double ellipsis: x ... ...
    let tmpl = Template::Ellipsis {
        subtemplate: Box::new(Template::Var(PVRef::new(2, 0))),
        level: 2,
        nesting: 2,
        vars: vec![PVRef::new(2, 0)],
    };
    assert!(tmpl.is_ellipsis());
    assert!(format!("{}", tmpl).contains("..."));
    assert!(format!("{}", tmpl).contains("nesting=2"));
}

// ============================================================================
// Identifier tests
// ============================================================================

#[test]
fn test_identifier_free_variable() {
    let id = Identifier::with_scopes("x", ScopeSet::new());
    assert!(id.is_free_variable());
    assert_eq!(id.name().as_ref(), "x");
}

#[test]
fn test_identifier_introduced() {
    let id = Identifier::new("y");
    assert!(!id.is_free_variable());
    assert!(id.definition_scopes().is_none());
}
