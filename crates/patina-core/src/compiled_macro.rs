//! Compiled macro data structures
//!
//! This module contains the data structures for compiled syntax-rules macros.
//! These are separate from the macro compilation/expansion logic (in patina-macros)
//! to allow `Value::Macro` to store `CompiledMacro` directly without `dyn Any`.
//!
//! Key types:
//! - `Pattern` - Compiled pattern for matching
//! - `Template` - Compiled template for expansion
//! - `Identifier` - Hygienic identifier with scope information
//! - `LiteralBinding` - Literal identifier with optional binding scope information
//! - `CompiledRule` - A single pattern/template rule
//! - `CompiledMacro` - Complete compiled macro definition

use crate::heap::SharedHeap;
use crate::pvref::PVRef;
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
use std::collections::HashMap;
use std::rc::Rc;

// ============================================================================
// LiteralBinding
// ============================================================================

/// A literal identifier with optional binding scope information
///
/// R7RS Section 4.3.2 specifies that literal matching uses `bound-identifier=?` semantics:
/// > "A literal identifier matches an input identifier if both have the same binding,
/// >  or both are unbound and have the same name."
///
/// This struct captures the binding information at macro definition time so we can
/// properly implement this semantics during pattern matching.
///
/// ## Example
///
/// ```scheme
/// ;; Case A: k is bound BEFORE macro definition
/// (let ((k 999))                              ; k bound with scopes {S1}
///   (let-syntax ((n (syntax-rules (k)         ; literal k captures scopes {S1}
///                     ((n k) 'matched)
///                     ((n x) 'no-match))))
///     (n k)))                                  ; k at use-site has scopes {S1}
/// ;; Should return 'matched because both refer to the same binding
///
/// ;; Case B: k is bound AFTER macro definition
/// (let-syntax ((n (syntax-rules (k)           ; literal k is unbound (None)
///                   ((n k) 'matched)
///                   ((n x) 'no-match))))
///   (let ((k 999))                            ; k bound with scopes {S1, S2}
///     (n k)))                                  ; k at use-site has scopes {S1, S2}
/// ;; Should return 'no-match because they have different bindings
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct LiteralBinding {
    /// The literal identifier name
    pub name: Rc<str>,
    /// Scope set of the literal's binding at macro definition time.
    /// - `None` = the literal was unbound (free identifier) at definition time
    /// - `Some(scopes)` = the literal was bound with these scopes at definition time
    pub binding_scopes: Option<ScopeSet>,
}

// ============================================================================
// Pattern
// ============================================================================

/// Compiled pattern for PVREF-based macro system
///
/// This is compiled once at macro definition time and reused for all expansions.
/// Based on Gauche's pattern compilation approach (macro.c:400+).
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Wildcard (_) - matches anything, binds nothing
    Wildcard,

    /// Literal value that must match exactly
    Literal(TaggedValue),

    /// Pattern variable - binds to matched expression
    ///
    /// Uses PVREF (Pattern Variable Reference) for O(1) lookup.
    Var(PVRef),

    /// List pattern: (p1 p2 p3)
    List(Vec<Pattern>),

    /// Vector pattern: #(p1 p2 p3)
    Vector(Vec<Pattern>),

    /// Dotted list pattern: (p1 p2 . rest)
    ///
    /// Matches an improper list where the tail is bound to a variable.
    DottedList {
        patterns: Vec<Pattern>,
        tail: Box<Pattern>,
    },

    /// Ellipsis pattern: (p1 ... p2)
    ///
    /// Matches zero or more repetitions of the subpattern.
    Ellipsis {
        /// Pattern to repeat
        subpattern: Box<Pattern>,

        /// Ellipsis nesting level (1 for first ..., 2 for nested, etc.)
        level: u8,

        /// Number of items after this ellipsis (excluding final CDR)
        ///
        /// This is Gauche's optimization that avoids backtracking.
        /// Example: From (x ... y z), x ... has num_following = 2
        num_following: usize,

        /// Pattern variables used in this subpattern
        ///
        /// Precomputed list of PVREFs for efficient matching.
        vars: Vec<PVRef>,
    },
}

impl Pattern {
    /// Check if this pattern is a wildcard
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Pattern::Wildcard)
    }

    /// Check if this pattern is a literal
    pub fn is_literal(&self) -> bool {
        matches!(self, Pattern::Literal(_))
    }

    /// Check if this pattern is a variable
    pub fn is_var(&self) -> bool {
        matches!(self, Pattern::Var(_))
    }

    /// Check if this pattern is an ellipsis
    pub fn is_ellipsis(&self) -> bool {
        matches!(self, Pattern::Ellipsis { .. })
    }

    /// Get the PVREF if this is a variable pattern
    pub fn as_var(&self) -> Option<PVRef> {
        match self {
            Pattern::Var(pvref) => Some(*pvref),
            _ => None,
        }
    }

    /// Visit every heap value embedded in this pattern. GC tracing hook.
    pub fn for_each_literal(&self, f: &mut dyn FnMut(TaggedValue)) {
        match self {
            Pattern::Literal(tv) => f(*tv),
            Pattern::Wildcard | Pattern::Var(_) => {}
            Pattern::List(patterns) | Pattern::Vector(patterns) => {
                for pattern in patterns {
                    pattern.for_each_literal(f);
                }
            }
            Pattern::DottedList { patterns, tail } => {
                for pattern in patterns {
                    pattern.for_each_literal(f);
                }
                tail.for_each_literal(f);
            }
            Pattern::Ellipsis { subpattern, .. } => subpattern.for_each_literal(f),
        }
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pattern::Wildcard => write!(f, "_"),
            Pattern::Literal(tv) => write!(f, "<literal {:?}>", tv),
            Pattern::Var(pvref) => write!(f, "?{}", pvref),
            Pattern::List(patterns) => {
                write!(f, "(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ")")
            }
            Pattern::Vector(patterns) => {
                write!(f, "#(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ")")
            }
            Pattern::DottedList { patterns, tail } => {
                write!(f, "(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, " . {})", tail)
            }
            Pattern::Ellipsis {
                subpattern,
                level,
                num_following,
                vars,
            } => {
                write!(
                    f,
                    "(... {} @level={} following={} vars={:?})",
                    subpattern, level, num_following, vars
                )
            }
        }
    }
}

// ============================================================================
// Identifier (for hygiene)
// ============================================================================

/// Identifier for hygiene
///
/// Wraps a symbol with scope information for hygienic macro expansion.
/// Uses Racket-style scope sets for hygiene.
///
/// ## Hygiene Model (Scope Sets)
///
/// - **Free variables** (bound at macro definition time): Have `definition_scopes` set
///   to the scope set at macro definition time.
///
/// - **Introduced identifiers** (from macro template): Have `definition_scopes` as None.
///   They get an expansion scope during expansion via the flip-scope algorithm.
#[derive(Clone, Debug)]
pub struct Identifier {
    name: Rc<str>,
    /// Scope set from macro definition time (for scope-based hygiene).
    ///
    /// - `Some(scopes)` = FREE VARIABLE with definition-time scopes.
    /// - `None` = INTRODUCED IDENTIFIER (will get expansion scope via flip-scope).
    definition_scopes: Option<ScopeSet>,
}

impl Identifier {
    /// Create a new identifier without definition scopes (introduced identifier)
    pub fn new(name: impl Into<Rc<str>>) -> Self {
        Self {
            name: name.into(),
            definition_scopes: None,
        }
    }

    /// Create a new identifier with definition scopes (free variable)
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
    pub fn is_free_variable(&self) -> bool {
        self.definition_scopes.is_some()
    }

    /// Get the definition scopes
    pub fn definition_scopes(&self) -> Option<&ScopeSet> {
        self.definition_scopes.as_ref()
    }
}

impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

// ============================================================================
// Template
// ============================================================================

/// Compiled template for PVREF-based macro system
///
/// Compiled once at macro definition time and reused for all expansions.
/// Based on Gauche's template compilation approach.
#[derive(Clone, Debug)]
pub enum Template {
    /// Literal value (inserted as-is)
    Literal(TaggedValue),

    /// Symbol to be inserted (hygienically renamed)
    Symbol(Identifier),

    /// Pattern variable reference
    ///
    /// Uses PVREF for O(1) lookup from pattern match bindings.
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
    Ellipsis {
        /// Template to repeat
        subtemplate: Box<Template>,

        /// Ellipsis nesting level
        level: u8,

        /// Number of consecutive ... in the template
        ///
        /// - 1 = normal single ellipsis: `x ...`
        /// - 2 = double ellipsis: `x ... ...` (SRFI-149)
        nesting: u8,

        /// Pattern variables used in this subtemplate
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

    /// Visit every heap value embedded in this template. GC tracing hook.
    pub fn for_each_literal(&self, f: &mut dyn FnMut(TaggedValue)) {
        match self {
            Template::Literal(tv) => f(*tv),
            Template::Symbol(_) | Template::Var(_) => {}
            Template::List(templates) | Template::Vector(templates) => {
                for template in templates {
                    template.for_each_literal(f);
                }
            }
            Template::DottedList { templates, tail } => {
                for template in templates {
                    template.for_each_literal(f);
                }
                tail.for_each_literal(f);
            }
            Template::Ellipsis { subtemplate, .. } => subtemplate.for_each_literal(f),
        }
    }
}

impl std::fmt::Display for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Template::Literal(tv) => write!(f, "<literal {:?}>", tv),
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

// ============================================================================
// CompiledRule and CompiledMacro
// ============================================================================

/// Compiled macro rule
///
/// Contains both pattern and template in PVREF-based representation,
/// along with metadata for efficient matching and expansion.
#[derive(Clone, Debug)]
pub struct CompiledRule {
    /// Compiled pattern
    pub pattern: Pattern,

    /// Compiled template
    pub template: Template,

    /// Number of pattern variables in this rule
    pub num_pvars: usize,

    /// Maximum ellipsis nesting level in this rule
    pub max_level: usize,

    /// Mapping from pattern variable names to their PVREFs (for debug output)
    pub pvar_names: HashMap<PVRef, Rc<str>>,
}

/// Compiled macro definition
///
/// Contains all rules for a syntax-rules macro in compiled form.
#[derive(Clone, Debug)]
pub struct CompiledMacro {
    /// Macro name (for error messages)
    pub name: Rc<str>,

    /// Literal identifiers with their binding information
    ///
    /// Each literal captures whether it was bound at macro definition time
    /// and what scopes that binding had. This enables correct `bound-identifier=?`
    /// semantics during pattern matching.
    pub literals: Vec<LiteralBinding>,

    /// Compiled rules (tried in order, first-match-wins)
    pub rules: Vec<CompiledRule>,

    /// Maximum number of pattern variables in any rule
    pub max_pvars: usize,

    /// Scope set at macro definition time (for scope-based hygiene)
    ///
    /// Free variables in templates will carry this scope set.
    pub definition_scopes: ScopeSet,

    /// Shared heap that stores TaggedValue literals from Pattern::Literal and Template::Literal.
    ///
    /// This heap is allocated at compile time and must be used (or merged into) the
    /// expansion-time heap so that literal TaggedValues remain valid.
    pub heap: SharedHeap,
}

impl CompiledMacro {
    /// Visit every heap value embedded in this macro's patterns and
    /// templates. GC tracing hook: a live macro binding keeps its literal
    /// values live.
    pub fn for_each_literal(&self, f: &mut dyn FnMut(TaggedValue)) {
        for rule in &self.rules {
            rule.pattern.for_each_literal(f);
            rule.template.for_each_literal(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn test_template_literal() {
        let tmpl = Template::Literal(TaggedValue::fixnum(42));
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
