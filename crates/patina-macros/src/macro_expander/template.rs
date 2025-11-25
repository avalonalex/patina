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

use patina_runtime::{PVRef, ScopeSet, Value};
use std::rc::Rc;

/// Identifier for hygiene
///
/// Wraps a symbol with scope information for hygienic macro expansion.
/// Uses Racket-style scope sets for hygiene (based on "Binding as Sets of Scopes", Flatt 2016).
///
/// This follows Gauche's approach: "symbols which a template inserts into the expanded
/// form are converted to identifiers at the macro definition time, encapsulating
/// the defining environment of the macro."
///
/// ## Hygiene Model (Scope Sets)
///
/// - **Free variables** (bound at macro definition time): Have `definition_scopes` set
///   to the scope set at macro definition time. These scopes enable correct binding
///   resolution using subset matching.
///
/// - **Introduced identifiers** (from macro template): Have `definition_scopes` as None.
///   They get an expansion scope during expansion via the flip-scope algorithm.
///
/// Two identifiers are "the same" if they have the same name AND same scopes.
/// This allows hygiene without renaming - scopes discriminate identifiers.
#[derive(Clone, Debug)]
pub struct Identifier {
    name: Rc<str>,
    /// Scope set from macro definition time (for scope-based hygiene).
    ///
    /// - `Some(scopes)` = FREE VARIABLE with definition-time scopes.
    ///   These scopes enable correct binding resolution using subset matching.
    ///
    /// - `None` = INTRODUCED IDENTIFIER (will get expansion scope via flip-scope).
    definition_scopes: Option<ScopeSet>,
}

impl Identifier {
    /// Create a new identifier without definition scopes (introduced identifier)
    ///
    /// Used for introduced identifiers that should get an expansion scope during expansion.
    /// These identifiers avoid capturing use-site bindings via the flip-scope algorithm.
    pub fn new(name: impl Into<Rc<str>>) -> Self {
        Self {
            name: name.into(),
            definition_scopes: None,
        }
    }

    /// Create a new identifier with definition scopes (free variable)
    ///
    /// Used for free variables in templates that should preserve their
    /// lexical scope from macro definition time using scope sets.
    ///
    /// # Arguments
    /// * `name` - The identifier name
    /// * `scopes` - The scope set at macro definition time
    pub fn with_scopes(name: impl Into<Rc<str>>, scopes: ScopeSet) -> Self {
        Self {
            name: name.into(),
            definition_scopes: Some(scopes),
        }
    }

    /// Get the identifier's name
    pub fn name(&self) -> &Rc<str> {
        &self.name
    }

    /// Check if this is a free variable (has definition scopes)
    ///
    /// Returns true if this identifier was bound at macro definition time
    /// and should preserve its definition-time binding.
    pub fn is_free_variable(&self) -> bool {
        self.definition_scopes.is_some()
    }

    /// Get the definition scopes
    ///
    /// Returns Some(scopes) for free variables, None for introduced identifiers.
    pub fn definition_scopes(&self) -> Option<&ScopeSet> {
        self.definition_scopes.as_ref()
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
pub enum Template {
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
    List(Vec<Template>),

    /// Vector template: #(t1 t2 t3)
    Vector(Vec<Template>),

    /// Dotted list template: (t1 t2 . rest)
    DottedList {
        templates: Vec<Template>,
        tail: Box<Template>,
    },

    /// Ellipsis template: (t1 ... t2)
    ///
    /// Repeats the subtemplate for each bound value.
    /// Supports multiple consecutive ellipses for SRFI-149.
    Ellipsis {
        /// Template to repeat
        subtemplate: Box<Template>,

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

impl Template {
    /// Check if this template is a literal
    pub fn is_literal(&self) -> bool {
        matches!(self, Template::Literal(_))
    }

    /// Check if this template is a symbol
    pub fn is_symbol(&self) -> bool {
        matches!(self, Template::Symbol(_))
    }

    /// Check if this template is a variable
    pub fn is_var(&self) -> bool {
        matches!(self, Template::Var(_))
    }

    /// Check if this template is an ellipsis
    pub fn is_ellipsis(&self) -> bool {
        matches!(self, Template::Ellipsis { .. })
    }

    /// Get the PVREF if this is a variable template
    pub fn as_var(&self) -> Option<PVRef> {
        match self {
            Template::Var(pvref) => Some(*pvref),
            _ => None,
        }
    }
}

impl std::fmt::Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Template::Literal(v) => write!(f, "{}", v),
            Template::Symbol(id) => write!(f, "{}", id),
            Template::Var(pvref) => write!(f, "${}", pvref),
            Template::List(templates) => {
                write!(f, "(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Template::Vector(templates) => {
                write!(f, "#(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            Template::DottedList { templates, tail } => {
                write!(f, "(")?;
                for (i, t) in templates.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, " . {})", tail)
            }
            Template::Ellipsis {
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
}
