//! PVREF-based template representation (Version 2)
//!
//! This module implements the new PVREF (Pattern Variable Reference) based
//! template representation for the macro system.
//!
//! Key improvements over the original Template:
//! - Uses PVREF encoding instead of string-based variable references
//! - Supports double ellipsis (... ...) for SRFI-149
//! - Precomputes variable lists for efficient expansion
//!
//! Inspired by Gauche's template compilation (macro.c:400+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use patina_runtime::{PVRef, Value};
use std::rc::Rc;

/// Identifier for hygiene
///
/// Wraps a symbol with scope information for hygienic macro expansion.
/// This will be used to rename introduced identifiers.
#[derive(Clone, Debug)]
pub struct Identifier {
    name: Rc<str>,
    // TODO: Add scope information for full hygiene support
}

impl Identifier {
    pub fn new(name: impl Into<Rc<str>>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &Rc<str> {
        &self.name
    }
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Compiled template for PVREF-based macro system
///
/// This is the second-generation template representation using PVREF encoding.
/// It's compiled once at macro definition time and reused for all expansions.
///
/// Based on Gauche's template compilation approach (macro.c:400+).
#[derive(Clone, Debug)]
pub enum Template2 {
    /// Literal value (inserted as-is)
    Literal(Value),

    /// Symbol to be inserted (hygienically renamed)
    ///
    /// Free identifiers in templates are converted to Identifier at compile time
    /// and renamed during expansion for hygiene.
    Symbol(Identifier),

    /// Pattern variable reference
    ///
    /// Uses PVREF (Pattern Variable Reference) for O(1) lookup.
    /// The value is substituted from pattern match bindings.
    Var(PVRef),

    /// List template: (t1 t2 t3)
    List(Vec<Template2>),

    /// Vector template: #(t1 t2 t3)
    Vector(Vec<Template2>),

    /// Dotted list template: (t1 t2 . rest)
    DottedList {
        templates: Vec<Template2>,
        tail: Box<Template2>,
    },

    /// Ellipsis template: (t1 ... t2)
    ///
    /// Repeats the subtemplate for each bound value.
    /// Supports multiple consecutive ellipses for SRFI-149.
    Ellipsis {
        /// Template to repeat
        subtemplate: Box<Template2>,

        /// Ellipsis nesting level
        level: u8,

        /// Number of consecutive ... in the template
        ///
        /// - 1 = normal single ellipsis: `x ...`
        /// - 2 = double ellipsis: `x ... ...` (SRFI-149)
        /// - etc.
        ///
        /// This enables support for patterns like ((a b ...) ...)
        /// with templates like ((a b ... ...) ...)
        nesting: u8,

        /// Pattern variables used in this subtemplate
        ///
        /// Precomputed list of PVREFs that will be iterated during expansion.
        vars: Vec<PVRef>,
    },
}

impl Template2 {
    /// Check if this template is a literal
    pub fn is_literal(&self) -> bool {
        matches!(self, Template2::Literal(_))
    }

    /// Check if this template is a symbol
    pub fn is_symbol(&self) -> bool {
        matches!(self, Template2::Symbol(_))
    }

    /// Check if this template is a variable
    pub fn is_var(&self) -> bool {
        matches!(self, Template2::Var(_))
    }

    /// Check if this template is an ellipsis
    pub fn is_ellipsis(&self) -> bool {
        matches!(self, Template2::Ellipsis { .. })
    }

    /// Get the PVREF if this is a variable template
    pub fn as_var(&self) -> Option<PVRef> {
        match self {
            Template2::Var(pvref) => Some(*pvref),
            _ => None,
        }
    }
}

impl std::fmt::Display for Template2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Template2::Literal(v) => write!(f, "{}", v),
            Template2::Symbol(id) => write!(f, "{}", id),
            Template2::Var(pvref) => write!(f, "${}", pvref),
            Template2::List(templates) => {
                write!(f, "(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Template2::Vector(templates) => {
                write!(f, "#(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Template2::DottedList { templates, tail } => {
                write!(f, "(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, " . {})", tail)
            }
            Template2::Ellipsis {
                subtemplate,
                level,
                nesting,
                vars,
            } => {
                write!(
                    f,
                    "(...{} {} @level={} nesting={} vars={:?})",
                    " ...".repeat((nesting - 1) as usize),
                    subtemplate,
                    level,
                    nesting,
                    vars
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template2_literal() {
        let tmpl = Template2::Literal(Value::Integer(42));
        assert!(tmpl.is_literal());
        assert_eq!(format!("{}", tmpl), "42");
    }

    #[test]
    fn test_template2_symbol() {
        let tmpl = Template2::Symbol(Identifier::new("if"));
        assert!(tmpl.is_symbol());
        assert_eq!(format!("{}", tmpl), "if");
    }

    #[test]
    fn test_template2_var() {
        let pvref = PVRef::new(0, 0);
        let tmpl = Template2::Var(pvref);
        assert!(tmpl.is_var());
        assert_eq!(tmpl.as_var(), Some(pvref));
        assert_eq!(format!("{}", tmpl), "$PVRef(level=0, index=0)");
    }

    #[test]
    fn test_template2_list() {
        let tmpl = Template2::List(vec![
            Template2::Symbol(Identifier::new("if")),
            Template2::Var(PVRef::new(0, 0)),
            Template2::Var(PVRef::new(0, 1)),
        ]);
        assert_eq!(
            format!("{}", tmpl),
            "(if $PVRef(level=0, index=0) $PVRef(level=0, index=1))"
        );
    }

    #[test]
    fn test_template2_ellipsis_single() {
        let tmpl = Template2::Ellipsis {
            subtemplate: Box::new(Template2::Var(PVRef::new(1, 0))),
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
        let tmpl = Template2::Ellipsis {
            subtemplate: Box::new(Template2::Var(PVRef::new(2, 0))),
            level: 2,
            nesting: 2,
            vars: vec![PVRef::new(2, 0)],
        };
        assert!(tmpl.is_ellipsis());
        assert!(format!("{}", tmpl).contains("..."));
        assert!(format!("{}", tmpl).contains("nesting=2"));
    }
}
