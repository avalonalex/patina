//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic identifier renaming
//!
//! Based on Steel-scheme's native Rust approach.

use crate::env::Environment;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub mod hygiene;
pub mod pattern;
pub mod template;

// Re-export main functions
pub use hygiene::apply_hygiene;
pub use pattern::match_pattern;
pub use template::expand_template;

/// Pattern in a syntax-rules macro
///
/// Represents the structure that input must match
#[derive(Debug, Clone)]
pub enum Pattern {
    /// Underscore wildcard: matches anything, binds nothing
    Wildcard,

    /// Literal constant: must match exactly
    Literal(Value),

    /// Pattern variable: binds to matched expression
    Variable(Rc<str>),

    /// List pattern: (p1 p2 p3)
    List(Vec<Pattern>),

    /// Vector pattern: #(p1 p2 p3)
    Vector(Vec<Pattern>),

    /// Ellipsis pattern: (p1 p2 ... p3)
    ///
    /// Matches a variable number of elements.
    /// The `repeated` pattern is matched zero or more times.
    Ellipsis {
        /// Patterns before ellipsis
        before: Vec<Pattern>,
        /// Pattern to repeat (zero or more times)
        repeated: Box<Pattern>,
        /// Patterns after ellipsis
        after: Vec<Pattern>,
    },
}

/// Template in a syntax-rules macro
///
/// Represents the output structure to generate
#[derive(Debug, Clone)]
pub enum Template {
    /// Literal value (inserted as-is)
    Literal(Value),

    /// Pattern variable reference (substituted from bindings)
    Variable(Rc<str>),

    /// List template: (t1 t2 t3)
    List(Vec<Template>),

    /// Vector template: #(t1 t2 t3)
    Vector(Vec<Template>),

    /// Ellipsis template: (t1 t2 ... t3)
    ///
    /// Repeats the `repeated` template for each bound value.
    Ellipsis {
        /// Templates before ellipsis
        before: Vec<Template>,
        /// Template to repeat
        repeated: Box<Template>,
        /// Templates after ellipsis
        after: Vec<Template>,
    },

    /// Ellipsis escape: (... template)
    ///
    /// Used to include literal `...` in output.
    /// Example: `(... ...)` produces the symbol `...`
    EllipsisEscape(Box<Template>),
}

/// A single pattern-template pair (one case in syntax-rules)
#[derive(Debug, Clone)]
pub struct MacroRule {
    pub pattern: Pattern,
    pub template: Template,
}

/// A macro definition (from define-syntax)
#[derive(Debug, Clone)]
pub struct Macro {
    /// Macro name (for error messages)
    pub name: Rc<str>,

    /// Literal identifiers (e.g., "else" in cond)
    ///
    /// These are matched by binding identity, not string equality
    pub literals: Vec<Rc<str>>,

    /// Pattern-template rules (tried in order, first-match-wins)
    pub rules: Vec<MacroRule>,

    /// Definition environment (for hygiene)
    ///
    /// Free identifiers in templates refer to bindings in this environment
    pub env: Rc<Environment>,
}

/// Bindings collected during pattern matching
pub type Bindings = HashMap<Rc<str>, BindingValue>;

/// Value bound to a pattern variable
#[derive(Debug, Clone)]
pub enum BindingValue {
    /// Single value (from pattern variable)
    Single(Value),

    /// Multiple values (from ellipsis pattern)
    Multiple(Vec<Value>),
}

impl Macro {
    /// Create a new macro
    pub fn new(
        name: Rc<str>,
        literals: Vec<Rc<str>>,
        rules: Vec<MacroRule>,
        env: Rc<Environment>,
    ) -> Self {
        Self {
            name,
            literals,
            rules,
            env,
        }
    }
}

impl MacroRule {
    /// Create a new macro rule
    pub fn new(pattern: Pattern, template: Template) -> Self {
        Self { pattern, template }
    }
}

// Stub implementations (to be filled in later phases)
// These allow the code to compile without full functionality

/// Parse a syntax-rules form into a Macro
///
/// TODO: Phase 4 - implement parsing
pub fn parse_syntax_rules(_expr: &Value, env: &Rc<Environment>) -> Result<Macro, crate::EvalError> {
    // Stub - will be implemented in Phase 4
    Ok(Macro {
        name: "stub".into(),
        literals: vec![],
        rules: vec![],
        env: env.clone(),
    })
}

/// Parse a Value into a Pattern
///
/// TODO: Phase 4 - implement pattern parsing
pub fn parse_pattern(_expr: &Value) -> Result<Pattern, crate::EvalError> {
    // Stub - will be implemented in Phase 4
    Ok(Pattern::Wildcard)
}

/// Parse a Value into a Template
///
/// TODO: Phase 4 - implement template parsing
pub fn parse_template(_expr: &Value) -> Result<Template, crate::EvalError> {
    // Stub - will be implemented in Phase 4
    Ok(Template::Literal(Value::Null))
}
