//! R7RS macro system implementation
//!
//! This module implements R7RS-small `syntax-rules` macros with:
//! - Pattern matching (including ellipsis patterns)
//! - Template expansion
//! - Hygienic identifier renaming
//!
//! Based on Steel-scheme's native Rust approach.

use patina_runtime::Environment;
use patina_runtime::Value;
use std::collections::HashMap;
use std::rc::Rc;

pub mod compiler;
pub mod expander_v2;
pub mod hygiene;
pub mod matcher_v2;
pub mod pattern;
pub mod pattern_v2;
pub mod template;
pub mod template_v2;

// Re-export main functions
pub use hygiene::apply_hygiene;
pub use pattern::match_pattern;
pub use template::expand_template;

// Re-export V2 types
pub use compiler::{CompiledMacro, CompiledRule, Compiler};
pub use expander_v2::{ExpandError, Expander};
pub use matcher_v2::{MatchError, Matcher};
pub use pattern_v2::Pattern2;
pub use template_v2::{Identifier, Template2};

/// Expand a macro using the V2 PVREF-based system
///
/// This is the new macro expansion pipeline:
/// 1. Try each compiled rule in order
/// 2. Use Matcher to match pattern against input
/// 3. Use Expander to expand template with match environment
/// 4. Apply hygiene to result
///
/// Based on Gauche's macro expansion pipeline.
pub fn expand_macro_v2(
    compiled_macro: &CompiledMacro,
    args: &patina_runtime::Value,
    env: &std::rc::Rc<patina_runtime::Environment>,
) -> Result<patina_runtime::Value, crate::error::FrontendError> {
    let expander = Expander::new();

    // Try each rule until we find a match
    for rule in &compiled_macro.rules {
        // Create matcher for this rule
        let matcher = Matcher::new(rule.num_pvars);

        // Try to match the pattern against the arguments
        match matcher.match_pattern(&rule.pattern, args) {
            Ok(match_env) => {
                // Pattern matched! Expand the template
                let expanded = expander
                    .expand(&rule.template, &match_env)
                    .map_err(|e| crate::error::FrontendError::InvalidSyntax(e.to_string()))?;

                // Apply hygiene: rename free identifiers
                // For now, we collect all symbols from the input to prevent renaming them
                let mut pattern_vars = std::collections::HashSet::new();
                collect_symbols_from_value(args, &mut pattern_vars);

                let hygienic = hygiene::apply_hygiene(&expanded, &pattern_vars, env);

                return Ok(hygienic);
            }
            Err(_) => {
                // This rule didn't match, try next one
                continue;
            }
        }
    }

    // No rule matched
    Err(crate::error::FrontendError::InvalidSyntax(format!(
        "No matching pattern for macro {}",
        compiled_macro.name
    )))
}

/// Expand a macro with the given arguments
///
/// Tries each rule in order until one matches, then expands using that rule's template.
/// Returns the expanded expression or an error if no rule matches.
///
/// # Arguments
/// - `macro_def`: The macro definition to expand
/// - `args`: The arguments to the macro (whole form including keyword)
/// - `env`: The environment to check for macro bindings (for nested macro support)
pub fn expand_macro(
    macro_def: &Macro,
    args: &patina_runtime::Value,
    env: &std::rc::Rc<patina_runtime::Environment>,
) -> Result<patina_runtime::Value, crate::error::FrontendError> {
    // Try each rule until we find a match
    for rule in &macro_def.rules {
        // Try to match the pattern against the arguments
        if let Some(bindings) = match_pattern(&rule.pattern, args, &macro_def.literals) {
            // Pattern matched! Expand the template with these bindings
            let expanded = expand_template(&rule.template, &bindings)?;

            // Apply hygiene: rename free identifiers
            // Collect identifiers that should NOT be renamed:
            // 1. Pattern variable names (keys)
            // 2. All symbols from pattern variable values (to prevent renaming substituted symbols)
            let mut pattern_vars: std::collections::HashSet<std::rc::Rc<str>> =
                bindings.keys().cloned().collect();

            // Add all symbols from the matched values
            collect_symbols_from_bindings(&bindings, &mut pattern_vars);

            let hygienic = hygiene::apply_hygiene(&expanded, &pattern_vars, env);

            return Ok(hygienic);
        }
    }

    // No rule matched
    Err(crate::error::FrontendError::InvalidSyntax(format!(
        "No matching pattern for macro {}",
        macro_def.name
    )))
}

/// Collect all symbols from binding values
///
/// This extracts all symbol identifiers from the values that pattern variables matched.
/// These symbols should not be renamed by hygiene because they came from the input form,
/// not from the macro template.
fn collect_symbols_from_bindings(
    bindings: &Bindings,
    symbols: &mut std::collections::HashSet<std::rc::Rc<str>>,
) {
    for value in bindings.values() {
        match value {
            BindingValue::Single(val) => collect_symbols_from_value(val, symbols),
            BindingValue::Multiple(vals) => {
                for val in vals {
                    collect_symbols_from_value(val, symbols);
                }
            }
        }
    }
}

/// Recursively collect all symbols from a value
fn collect_symbols_from_value(
    value: &patina_runtime::Value,
    symbols: &mut std::collections::HashSet<std::rc::Rc<str>>,
) {
    match value {
        patina_runtime::Value::Symbol(name) => {
            symbols.insert(name.clone());
        }
        patina_runtime::Value::Pair(pair) => {
            collect_symbols_from_value(&pair.0, symbols);
            collect_symbols_from_value(&pair.1, symbols);
        }
        patina_runtime::Value::Vector(vec) => {
            for item in vec.borrow().iter() {
                collect_symbols_from_value(item, symbols);
            }
        }
        // All other values (numbers, strings, booleans, etc.) have no symbols
        _ => {}
    }
}

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

    /// Dotted list pattern: (p1 p2 . rest)
    ///
    /// Matches an improper list where the tail is bound to a variable.
    /// Example: (a b . rest) matches (1 2 3 4 5) with:
    ///   a=1, b=2, rest=(3 4 5)
    DottedList {
        /// Patterns before the dot
        patterns: Vec<Pattern>,
        /// Pattern for the tail (after the dot)
        tail: Box<Pattern>,
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

    /// Dotted list template: (t1 t2 . rest)
    ///
    /// Produces an improper list.
    /// Example: (lambda (a . b) body) where b is a pattern variable
    DottedList {
        /// Templates before the dot
        templates: Vec<Template>,
        /// Template for the tail (after the dot)
        tail: Box<Template>,
    },
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
pub fn parse_syntax_rules(
    _expr: &Value,
    env: &Rc<Environment>,
) -> Result<Macro, crate::error::FrontendError> {
    // Stub - will be implemented in Phase 4
    Ok(Macro {
        name: "stub".into(),
        literals: vec![],
        rules: vec![],
        env: env.clone(),
    })
}

/// Parse a Value into a Pattern
pub fn parse_pattern(expr: &Value) -> Result<Pattern, crate::error::FrontendError> {
    match expr {
        // Underscore wildcard
        Value::Symbol(s) if s.as_ref() == "_" => Ok(Pattern::Wildcard),

        // Symbol: pattern variable
        Value::Symbol(s) => Ok(Pattern::Variable(s.clone())),

        // List: check for ellipsis and dotted pairs
        Value::Pair(_) => {
            let (items, tail) = collect_list_items(expr)?;
            parse_list_pattern(&items, tail.as_ref())
        }

        Value::Null => Ok(Pattern::List(vec![])),

        // Vector
        Value::Vector(v) => {
            let items = v.borrow();
            let patterns: Result<Vec<_>, _> = items.iter().map(parse_pattern).collect();
            Ok(Pattern::Vector(patterns?))
        }

        // Literal value (boolean, number, string, character, etc.)
        other => Ok(Pattern::Literal(other.clone())),
    }
}

/// Parse a list pattern, detecting ellipsis and dotted tails
fn parse_list_pattern(
    items: &[Value],
    tail: Option<&Value>,
) -> Result<Pattern, crate::error::FrontendError> {
    // Check if this is a dotted list pattern
    if let Some(tail_value) = tail {
        // Dotted list: (a b . rest)
        let patterns: Result<Vec<_>, _> = items.iter().map(parse_pattern).collect();
        let tail_pattern = Box::new(parse_pattern(tail_value)?);
        return Ok(Pattern::DottedList {
            patterns: patterns?,
            tail: tail_pattern,
        });
    }

    // Find ellipsis positions
    let ellipsis_pos = items
        .iter()
        .position(|item| matches!(item, Value::Symbol(s) if s.as_ref() == "..."));

    match ellipsis_pos {
        None => {
            // No ellipsis: regular list pattern
            let patterns: Result<Vec<_>, _> = items.iter().map(parse_pattern).collect();
            Ok(Pattern::List(patterns?))
        }
        Some(pos) => {
            // Has ellipsis
            if pos == 0 {
                return Err(crate::error::FrontendError::InvalidSyntax(
                    "Ellipsis cannot appear at start of pattern".to_string(),
                ));
            }

            // The pattern before ... is the repeated pattern
            let before: Result<Vec<_>, _> = items[..pos - 1].iter().map(parse_pattern).collect();

            let repeated = Box::new(parse_pattern(&items[pos - 1])?);

            // Handle 'after' section recursively to support multiple ellipses
            // e.g., ((x y) ...) z ... should parse z ... as another ellipsis pattern
            let after_items = &items[pos + 1..];
            let after = if after_items.is_empty() {
                vec![]
            } else {
                // Recursively parse the after section as a pattern list
                // This allows patterns like: ((x y) ...) z ...
                // to be parsed as Ellipsis with after containing another Ellipsis
                match parse_list_pattern(after_items, None)? {
                    Pattern::List(patterns) => patterns,
                    Pattern::Ellipsis {
                        before: b,
                        repeated: r,
                        after: a,
                    } => {
                        // The after section itself is an ellipsis pattern
                        // Prepend the before patterns, then add the ellipsis
                        let mut result = b;
                        result.push(Pattern::Ellipsis {
                            before: vec![],
                            repeated: r,
                            after: a,
                        });
                        result
                    }
                    other => vec![other],
                }
            };

            Ok(Pattern::Ellipsis {
                before: before?,
                repeated,
                after,
            })
        }
    }
}

/// Parse a Value into a Template
pub fn parse_template(expr: &Value) -> Result<Template, crate::error::FrontendError> {
    match expr {
        // Symbol: template variable
        Value::Symbol(s) => Ok(Template::Variable(s.clone())),

        // List: check for ellipsis, ellipsis escape, and dotted tails
        Value::Pair(_) => {
            let (items, tail) = collect_list_items(expr)?;
            parse_list_template(&items, tail.as_ref())
        }

        Value::Null => Ok(Template::List(vec![])),

        // Vector
        Value::Vector(v) => {
            let items = v.borrow();
            let templates: Result<Vec<_>, _> = items.iter().map(parse_template).collect();
            Ok(Template::Vector(templates?))
        }

        // Literal value
        other => Ok(Template::Literal(other.clone())),
    }
}

/// Parse a list template, detecting ellipsis, ellipsis escape, and dotted tails
fn parse_list_template(
    items: &[Value],
    tail: Option<&Value>,
) -> Result<Template, crate::error::FrontendError> {
    // Check if this is a dotted list template
    if let Some(tail_value) = tail {
        // Dotted list: (a b . rest)
        let templates: Result<Vec<_>, _> = items.iter().map(parse_template).collect();
        let tail_template = Box::new(parse_template(tail_value)?);
        return Ok(Template::DottedList {
            templates: templates?,
            tail: tail_template,
        });
    }

    // Check for ellipsis escape: (... template)
    if items.len() == 2
        && let Value::Symbol(s) = &items[0]
        && s.as_ref() == "..."
    {
        let inner = Box::new(parse_template(&items[1])?);
        return Ok(Template::EllipsisEscape(inner));
    }

    // Find ellipsis positions
    let ellipsis_pos = items
        .iter()
        .position(|item| matches!(item, Value::Symbol(s) if s.as_ref() == "..."));

    match ellipsis_pos {
        None => {
            // No ellipsis: regular list template
            let templates: Result<Vec<_>, _> = items.iter().map(parse_template).collect();
            Ok(Template::List(templates?))
        }
        Some(pos) => {
            // Has ellipsis
            if pos == 0 {
                return Err(crate::error::FrontendError::InvalidSyntax(
                    "Ellipsis cannot appear at start of template".to_string(),
                ));
            }

            // The template before ... is the repeated template
            let before: Result<Vec<_>, _> = items[..pos - 1].iter().map(parse_template).collect();

            let repeated = Box::new(parse_template(&items[pos - 1])?);

            // Handle 'after' section recursively to support multiple ellipses
            // e.g., (x ...) (y ...) should parse both as separate ellipsis templates
            let after_items = &items[pos + 1..];
            let after = if after_items.is_empty() {
                vec![]
            } else {
                // Recursively parse the after section as a template list
                // This allows templates like: (x ...) y ...
                match parse_list_template(after_items, None)? {
                    Template::List(templates) => templates,
                    Template::Ellipsis {
                        before: b,
                        repeated: r,
                        after: a,
                    } => {
                        // The after section itself is an ellipsis template
                        let mut result = b;
                        result.push(Template::Ellipsis {
                            before: vec![],
                            repeated: r,
                            after: a,
                        });
                        result
                    }
                    other => vec![other],
                }
            };

            Ok(Template::Ellipsis {
                before: before?,
                repeated,
                after,
            })
        }
    }
}

/// Helper: Collect items from a list Value
/// Returns (items, tail) where tail is Some(value) for improper lists
fn collect_list_items(
    expr: &Value,
) -> Result<(Vec<Value>, Option<Value>), crate::error::FrontendError> {
    let mut items = Vec::new();
    let mut current = expr;

    loop {
        match current {
            Value::Null => return Ok((items, None)),
            Value::Pair(pair) => {
                items.push(pair.0.clone());
                current = &pair.1;
            }
            _ => {
                // Improper list: (a b . c)
                return Ok((items, Some(current.clone())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // Helper to create a list from values
    fn make_list(items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, val| Value::Pair(Rc::new((val, acc))))
    }

    #[test]
    fn test_parse_pattern_wildcard() {
        let expr = Value::Symbol("_".into());
        let pattern = parse_pattern(&expr).unwrap();
        assert!(matches!(pattern, Pattern::Wildcard));
    }

    #[test]
    fn test_parse_pattern_variable() {
        let expr = Value::Symbol("x".into());
        let pattern = parse_pattern(&expr).unwrap();
        assert!(matches!(pattern, Pattern::Variable(s) if s.as_ref() == "x"));
    }

    #[test]
    fn test_parse_pattern_literal() {
        let expr = Value::Integer(42);
        let pattern = parse_pattern(&expr).unwrap();
        assert!(matches!(pattern, Pattern::Literal(Value::Integer(42))));
    }

    #[test]
    fn test_parse_pattern_simple_list() {
        // (when test body)
        let expr = make_list(vec![
            Value::Symbol("when".into()),
            Value::Symbol("test".into()),
            Value::Symbol("body".into()),
        ]);

        let pattern = parse_pattern(&expr).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                assert!(matches!(&patterns[0], Pattern::Variable(s) if s.as_ref() == "when"));
                assert!(matches!(&patterns[1], Pattern::Variable(s) if s.as_ref() == "test"));
                assert!(matches!(&patterns[2], Pattern::Variable(s) if s.as_ref() == "body"));
            }
            _ => panic!("Expected Pattern::List"),
        }
    }

    #[test]
    fn test_parse_pattern_with_ellipsis() {
        // (when test body ...)
        let expr = make_list(vec![
            Value::Symbol("when".into()),
            Value::Symbol("test".into()),
            Value::Symbol("body".into()),
            Value::Symbol("...".into()),
        ]);

        let pattern = parse_pattern(&expr).unwrap();

        match pattern {
            Pattern::Ellipsis {
                before,
                repeated,
                after,
            } => {
                assert_eq!(before.len(), 2); // when, test
                assert!(matches!(&before[0], Pattern::Variable(s) if s.as_ref() == "when"));
                assert!(matches!(&before[1], Pattern::Variable(s) if s.as_ref() == "test"));
                assert!(matches!(*repeated, Pattern::Variable(s) if s.as_ref() == "body"));
                assert_eq!(after.len(), 0);
            }
            _ => panic!("Expected Pattern::Ellipsis"),
        }
    }

    #[test]
    fn test_parse_pattern_with_ellipsis_and_after() {
        // (let name (bindings ...) body)
        let expr = make_list(vec![
            Value::Symbol("let".into()),
            Value::Symbol("name".into()),
            Value::Symbol("bindings".into()),
            Value::Symbol("...".into()),
            Value::Symbol("body".into()),
        ]);

        let pattern = parse_pattern(&expr).unwrap();

        match pattern {
            Pattern::Ellipsis {
                before,
                repeated,
                after,
            } => {
                assert_eq!(before.len(), 2); // let, name
                assert_eq!(after.len(), 1); // body
                assert!(matches!(*repeated, Pattern::Variable(s) if s.as_ref() == "bindings"));
                assert!(matches!(&after[0], Pattern::Variable(s) if s.as_ref() == "body"));
            }
            _ => panic!("Expected Pattern::Ellipsis"),
        }
    }

    #[test]
    fn test_parse_template_variable() {
        let expr = Value::Symbol("x".into());
        let template = parse_template(&expr).unwrap();
        assert!(matches!(template, Template::Variable(s) if s.as_ref() == "x"));
    }

    #[test]
    fn test_parse_template_literal() {
        let expr = Value::Integer(42);
        let template = parse_template(&expr).unwrap();
        assert!(matches!(template, Template::Literal(Value::Integer(42))));
    }

    #[test]
    fn test_parse_template_simple_list() {
        // (if test body)
        let expr = make_list(vec![
            Value::Symbol("if".into()),
            Value::Symbol("test".into()),
            Value::Symbol("body".into()),
        ]);

        let template = parse_template(&expr).unwrap();

        match template {
            Template::List(templates) => {
                assert_eq!(templates.len(), 3);
                assert!(matches!(&templates[0], Template::Variable(s) if s.as_ref() == "if"));
                assert!(matches!(&templates[1], Template::Variable(s) if s.as_ref() == "test"));
                assert!(matches!(&templates[2], Template::Variable(s) if s.as_ref() == "body"));
            }
            _ => panic!("Expected Template::List"),
        }
    }

    #[test]
    fn test_parse_template_with_ellipsis() {
        // (begin body ...)
        let expr = make_list(vec![
            Value::Symbol("begin".into()),
            Value::Symbol("body".into()),
            Value::Symbol("...".into()),
        ]);

        let template = parse_template(&expr).unwrap();

        match template {
            Template::Ellipsis {
                before,
                repeated,
                after,
            } => {
                assert_eq!(before.len(), 1); // begin
                assert!(matches!(*repeated, Template::Variable(s) if s.as_ref() == "body"));
                assert_eq!(after.len(), 0);
            }
            _ => panic!("Expected Template::Ellipsis"),
        }
    }

    #[test]
    fn test_parse_template_ellipsis_escape() {
        // (... x)
        let expr = make_list(vec![Value::Symbol("...".into()), Value::Symbol("x".into())]);

        let template = parse_template(&expr).unwrap();

        match template {
            Template::EllipsisEscape(inner) => {
                assert!(matches!(*inner, Template::Variable(s) if s.as_ref() == "x"));
            }
            _ => panic!("Expected Template::EllipsisEscape"),
        }
    }

    #[test]
    fn test_parse_vector_pattern() {
        let expr = Value::Vector(Rc::new(RefCell::new(vec![
            Value::Symbol("x".into()),
            Value::Symbol("y".into()),
        ])));

        let pattern = parse_pattern(&expr).unwrap();

        match pattern {
            Pattern::Vector(patterns) => {
                assert_eq!(patterns.len(), 2);
            }
            _ => panic!("Expected Pattern::Vector"),
        }
    }

    #[test]
    fn test_roundtrip_when_macro() {
        // Parse pattern: (when test body ...)
        let pattern_expr = make_list(vec![
            Value::Symbol("when".into()),
            Value::Symbol("test".into()),
            Value::Symbol("body".into()),
            Value::Symbol("...".into()),
        ]);

        let pattern = parse_pattern(&pattern_expr).unwrap();

        // Parse template: (if test (begin body ...))
        let template_expr = make_list(vec![
            Value::Symbol("if".into()),
            Value::Symbol("test".into()),
            make_list(vec![
                Value::Symbol("begin".into()),
                Value::Symbol("body".into()),
                Value::Symbol("...".into()),
            ]),
        ]);

        let template = parse_template(&template_expr).unwrap();

        // Now test matching and expansion
        use crate::macro_expander::{expand_template, match_pattern};

        let input = make_list(vec![
            Value::Symbol("when".into()),
            Value::Boolean(true),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        let bindings = match_pattern(&pattern, &input, &[]).unwrap();
        let output = expand_template(&template, &bindings).unwrap();

        // Should expand to: (if #t (begin 1 2))
        let expected = make_list(vec![
            Value::Symbol("if".into()),
            Value::Boolean(true),
            make_list(vec![
                Value::Symbol("begin".into()),
                Value::Integer(1),
                Value::Integer(2),
            ]),
        ]);

        assert_eq!(format!("{}", output), format!("{}", expected));
    }
}
