//! Re-export Template and Identifier from patina-core
//!
//! These types are now defined in patina-core for shared use across crates.
//! This module re-exports them for backwards compatibility.

pub use patina_runtime::{Identifier, Template};

#[cfg(test)]
mod tests {
    use super::*;
    use patina_runtime::{PVRef, ScopeSet, Value};

    #[test]
    fn test_template2_literal() {
        let tmpl = Template::Literal(Value::Integer(42));
        assert!(tmpl.is_literal());
        assert_eq!(format!("{}", tmpl), "42");
    }

    #[test]
    fn test_template2_symbol() {
        let tmpl = Template::Symbol(Identifier::new("if"));
        assert!(tmpl.is_symbol());
        assert_eq!(format!("{}", tmpl), "if");
    }

    #[test]
    fn test_template2_var() {
        let pvref = PVRef::new(0, 0);
        let tmpl = Template::Var(pvref);
        assert!(tmpl.is_var());
        assert_eq!(tmpl.as_var(), Some(pvref));
        assert_eq!(format!("{}", tmpl), "$PVRef(level=0, index=0)");
    }

    #[test]
    fn test_template2_list() {
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
    fn test_template2_ellipsis_single() {
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
    fn test_template2_ellipsis_double() {
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
}
