//! Shared utilities for the macro system
//!
//! This module provides common constants, helpers, and utility functions
//! used across the macro compiler, matcher, and expander.

use patina_runtime::{PVRef, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// ============================================================================
// Magic String Constants
// ============================================================================

/// The standard R7RS ellipsis symbol
pub const ELLIPSIS: &str = "...";

/// The wildcard/underscore pattern element
pub const WILDCARD: &str = "_";

/// Macro definition forms that should not have their contents marked with scopes
pub const MACRO_DEFINITION_FORMS: &[&str] = &[
    "syntax-rules",
    "define-syntax",
    "let-syntax",
    "letrec-syntax",
    "quote",
];

// ============================================================================
// List/Vector Conversion Utilities
// ============================================================================

/// Iterator over a Scheme list that clones elements lazily
///
/// This iterator traverses a Scheme list, cloning each element only when
/// requested. This is more efficient than converting the entire list to a Vec
/// upfront when pattern matching may fail early.
///
/// # Design Note
///
/// Due to `RefCell` borrowing constraints, we can't return references to list
/// elements. Instead, we clone each `Value` as we iterate. However, this is
/// still beneficial because:
/// 1. We avoid pre-allocating a `Vec` for the entire list
/// 2. Early pattern match failures don't pay for unused clones
/// 3. `Value` clones are cheap for most variants (just `Rc` bumps)
pub struct SchemeListIter {
    current: Value,
}

impl SchemeListIter {
    /// Create a new iterator over a Scheme list
    pub fn new(list: &Value) -> Self {
        Self {
            current: list.clone(),
        }
    }

    /// Count the length of a Scheme list without collecting
    ///
    /// Returns None if not a proper list.
    pub fn len(list: &Value) -> Option<usize> {
        let mut count = 0;
        let mut current = list.clone();
        loop {
            match current {
                Value::Null => return Some(count),
                Value::Pair(p) => {
                    count += 1;
                    let borrowed = p.borrow();
                    current = borrowed.1.clone();
                }
                _ => return None,
            }
        }
    }

    /// Check if a value is a proper list (ends with Null)
    pub fn is_proper_list(list: &Value) -> bool {
        Self::len(list).is_some()
    }
}

impl Iterator for SchemeListIter {
    type Item = Result<Value, ImproperListError>;

    fn next(&mut self) -> Option<Self::Item> {
        match &self.current {
            Value::Null => None,
            Value::Pair(p) => {
                let borrowed = p.borrow();
                let car = borrowed.0.clone();
                let cdr = borrowed.1.clone();
                drop(borrowed);
                self.current = cdr;
                Some(Ok(car))
            }
            _ => {
                // Return error once, then stop
                let err = ImproperListError;
                self.current = Value::Null; // Prevent further iteration
                Some(Err(err))
            }
        }
    }
}

/// Error indicating an improper list was encountered during iteration
#[derive(Debug, Clone, Copy)]
pub struct ImproperListError;

impl std::fmt::Display for ImproperListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "improper list encountered")
    }
}

impl std::error::Error for ImproperListError {}

/// Convert a Scheme proper list Value to a Vec
///
/// Returns an error message if the value is not a proper list.
pub fn list_to_vec(value: &Value) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => break,
            Value::Pair(p) => {
                let borrowed = p.borrow();
                result.push(borrowed.0.clone());
                current = borrowed.1.clone();
            }
            _ => {
                return Err(format!("Expected proper list, got {}", value));
            }
        }
    }

    Ok(result)
}

/// Convert a Vec to a Scheme proper list Value
pub fn vec_to_list(values: Vec<Value>) -> Value {
    let mut result = Value::Null;
    for value in values.into_iter().rev() {
        result = Value::Pair(Rc::new(RefCell::new((value, result))));
    }
    result
}

/// Convert a Scheme list to a Vec, allowing improper lists
///
/// Returns (elements, tail) where tail is the final cdr (Null for proper lists)
pub fn list_to_vec_with_tail(value: &Value) -> (Vec<Value>, Value) {
    let mut result = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => return (result, Value::Null),
            Value::Pair(p) => {
                let borrowed = p.borrow();
                result.push(borrowed.0.clone());
                current = borrowed.1.clone();
            }
            // Improper list - return the tail
            other => return (result, other),
        }
    }
}

// ============================================================================
// Symbol/Identifier Name Extraction
// ============================================================================

/// Extract the name from a Symbol or Identifier
///
/// Returns None for non-identifier values.
pub fn get_identifier_name(value: &Value) -> Option<Rc<str>> {
    match value {
        Value::Symbol(s) => Some(s.clone()),
        Value::Identifier(id) => Some(id.name.clone()),
        _ => None,
    }
}

/// Check if a value is a specific named symbol or identifier
pub fn is_named(value: &Value, name: &str) -> bool {
    match value {
        Value::Symbol(s) => s.as_ref() == name,
        Value::Identifier(id) => id.name.as_ref() == name,
        _ => false,
    }
}

/// Check if a value is the ellipsis symbol (...)
pub fn is_ellipsis_symbol(value: &Value) -> bool {
    is_named(value, ELLIPSIS)
}

/// Check if a value is the wildcard/underscore symbol (_)
pub fn is_wildcard_symbol(value: &Value) -> bool {
    is_named(value, WILDCARD)
}

// ============================================================================
// Pattern/Template String Formatting (for debug output)
// ============================================================================

use crate::macro_expander::Pattern;

/// Convert a Pattern to a readable string with variable names
///
/// This is used for debug output and error messages.
pub fn pattern_to_string_with_names(pattern: &Pattern, names: &HashMap<PVRef, Rc<str>>) -> String {
    match pattern {
        Pattern::Wildcard => WILDCARD.to_string(),
        Pattern::Literal(v) => format!("{}", v),
        Pattern::Var(pv) => names
            .get(pv)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "var".to_string()),
        Pattern::List(patterns) => {
            let inner = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        Pattern::DottedList { patterns, tail } => {
            let mut parts: Vec<String> = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect();
            parts.push(".".to_string());
            parts.push(pattern_to_string_with_names(tail, names));
            format!("({})", parts.join(" "))
        }
        Pattern::Vector(patterns) => {
            let inner = patterns
                .iter()
                .map(|p| pattern_to_string_with_names(p, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({})", inner)
        }
        Pattern::Ellipsis { subpattern, .. } => {
            format!(
                "({} {})",
                pattern_to_string_with_names(subpattern, names),
                ELLIPSIS
            )
        }
    }
}

/// Convert a Pattern to a readable string without variable names
pub fn pattern_to_string(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard => WILDCARD.to_string(),
        Pattern::Literal(v) => format!("{}", v),
        Pattern::Var(_pv) => "var".to_string(),
        Pattern::List(patterns) => {
            let inner = patterns
                .iter()
                .map(pattern_to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        Pattern::DottedList { patterns, tail } => {
            let mut parts: Vec<String> = patterns.iter().map(pattern_to_string).collect();
            parts.push(".".to_string());
            parts.push(pattern_to_string(tail));
            format!("({})", parts.join(" "))
        }
        Pattern::Vector(patterns) => {
            let inner = patterns
                .iter()
                .map(pattern_to_string)
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({})", inner)
        }
        Pattern::Ellipsis { subpattern, .. } => {
            format!("({} {})", pattern_to_string(subpattern), ELLIPSIS)
        }
    }
}

use crate::macro_expander::Template;

/// Convert a Template to a readable string with variable names
pub fn template_to_string_with_names(
    template: &Template,
    names: &HashMap<PVRef, Rc<str>>,
) -> String {
    match template {
        Template::Literal(v) => format!("{}", v),
        Template::Symbol(id) => id.name().to_string(),
        Template::Var(pv) => names
            .get(pv)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "var".to_string()),
        Template::List(templates) => {
            let inner = templates
                .iter()
                .map(|t| template_to_string_with_names(t, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("({})", inner)
        }
        Template::DottedList { templates, tail } => {
            let mut parts: Vec<String> = templates
                .iter()
                .map(|t| template_to_string_with_names(t, names))
                .collect();
            parts.push(".".to_string());
            parts.push(template_to_string_with_names(tail, names));
            format!("({})", parts.join(" "))
        }
        Template::Vector(templates) => {
            let inner = templates
                .iter()
                .map(|t| template_to_string_with_names(t, names))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({})", inner)
        }
        Template::Ellipsis {
            subtemplate,
            nesting,
            ..
        } => {
            let dots = (0..*nesting)
                .map(|_| ELLIPSIS)
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "({} {})",
                template_to_string_with_names(subtemplate, names),
                dots
            )
        }
    }
}

// ============================================================================
// Pattern/Template Variable Collection Utilities
// ============================================================================

use std::collections::HashSet;

/// Collect all pattern variables (PVRefs) from a pattern into a HashSet
///
/// This is used by the matcher to know which variables to extract after matching.
/// Returns a set of unique PVRefs found in the pattern.
pub fn collect_pattern_pvars(pattern: &Pattern) -> HashSet<PVRef> {
    let mut result = HashSet::new();
    collect_pattern_pvars_impl(pattern, &mut result);
    result
}

fn collect_pattern_pvars_impl(pattern: &Pattern, acc: &mut HashSet<PVRef>) {
    match pattern {
        Pattern::Var(pvref) => {
            acc.insert(*pvref);
        }
        Pattern::List(patterns) | Pattern::Vector(patterns) => {
            for p in patterns {
                collect_pattern_pvars_impl(p, acc);
            }
        }
        Pattern::DottedList { patterns, tail } => {
            for p in patterns {
                collect_pattern_pvars_impl(p, acc);
            }
            collect_pattern_pvars_impl(tail, acc);
        }
        Pattern::Ellipsis {
            subpattern, vars, ..
        } => {
            // Include the direct vars from this ellipsis
            for pvref in vars {
                acc.insert(*pvref);
            }
            // Recursively collect from subpattern
            collect_pattern_pvars_impl(subpattern, acc);
        }
        Pattern::Wildcard | Pattern::Literal(_) => {
            // No variables
        }
    }
}

/// Collect all pattern variables with their ellipsis levels
///
/// This is used by the validator to check level consistency between pattern and template.
/// Returns a map from PVRef to its ellipsis nesting level.
pub fn collect_pattern_vars_with_levels(pattern: &Pattern) -> HashMap<PVRef, usize> {
    let mut result = HashMap::new();
    collect_pattern_vars_with_levels_impl(pattern, &mut result);
    result
}

fn collect_pattern_vars_with_levels_impl(pattern: &Pattern, acc: &mut HashMap<PVRef, usize>) {
    match pattern {
        Pattern::Var(pvref) => {
            acc.insert(*pvref, pvref.level());
        }
        Pattern::List(patterns) | Pattern::Vector(patterns) => {
            for p in patterns {
                collect_pattern_vars_with_levels_impl(p, acc);
            }
        }
        Pattern::DottedList { patterns, tail } => {
            for p in patterns {
                collect_pattern_vars_with_levels_impl(p, acc);
            }
            collect_pattern_vars_with_levels_impl(tail, acc);
        }
        Pattern::Ellipsis { subpattern, .. } => {
            collect_pattern_vars_with_levels_impl(subpattern, acc);
        }
        Pattern::Wildcard | Pattern::Literal(_) => {}
    }
}

/// Collect all template variables with their usage levels
///
/// This is used by the validator to check that template variables are used at
/// appropriate ellipsis nesting levels.
/// Returns a map from PVRef to its usage level in the template.
pub fn collect_template_vars_with_levels(template: &Template) -> HashMap<PVRef, usize> {
    let mut result = HashMap::new();
    collect_template_vars_with_levels_impl(template, 0, &mut result);
    result
}

fn collect_template_vars_with_levels_impl(
    template: &Template,
    current_level: usize,
    acc: &mut HashMap<PVRef, usize>,
) {
    match template {
        Template::Var(pvref) => {
            acc.insert(*pvref, current_level);
        }
        Template::List(templates) | Template::Vector(templates) => {
            for t in templates {
                collect_template_vars_with_levels_impl(t, current_level, acc);
            }
        }
        Template::DottedList { templates, tail } => {
            for t in templates {
                collect_template_vars_with_levels_impl(t, current_level, acc);
            }
            collect_template_vars_with_levels_impl(tail, current_level, acc);
        }
        Template::Ellipsis {
            subtemplate,
            nesting,
            ..
        } => {
            // Variables inside ellipsis are at a higher level
            let inner_level = current_level + (*nesting as usize);
            collect_template_vars_with_levels_impl(subtemplate, inner_level, acc);
        }
        Template::Literal(_) | Template::Symbol(_) => {}
    }
}

/// Collect template variables at or above a minimum level
///
/// This is used by the compiler to determine which variables drive ellipsis iteration.
/// Returns a sorted, deduplicated list of PVRefs at or above the specified level.
pub fn collect_template_vars_at_level(template: &Template, min_level: usize) -> Vec<PVRef> {
    let mut result = Vec::new();
    collect_template_vars_at_level_impl(template, min_level, &mut result);
    result.sort_by_key(|pv| (pv.level(), pv.index()));
    result.dedup();
    result
}

fn collect_template_vars_at_level_impl(
    template: &Template,
    min_level: usize,
    acc: &mut Vec<PVRef>,
) {
    match template {
        Template::Var(pvref) if pvref.level() >= min_level => {
            acc.push(*pvref);
        }
        Template::List(items) | Template::Vector(items) => {
            for item in items {
                collect_template_vars_at_level_impl(item, min_level, acc);
            }
        }
        Template::DottedList { templates, tail } => {
            for t in templates {
                collect_template_vars_at_level_impl(t, min_level, acc);
            }
            collect_template_vars_at_level_impl(tail, min_level, acc);
        }
        Template::Ellipsis { subtemplate, .. } => {
            collect_template_vars_at_level_impl(subtemplate, min_level, acc);
        }
        _ => {}
    }
}

// ============================================================================
// Macro Form Detection
// ============================================================================

/// Check if a value is a macro definition form (syntax-rules, define-syntax, etc.)
///
/// These forms should not have their contents marked with macro scopes during
/// template expansion, as they define their own hygiene context.
pub fn is_macro_definition_form(value: &Value) -> bool {
    if let Value::Pair(pair) = value {
        let borrowed = pair.borrow();
        let car = &borrowed.0;

        if let Some(name) = get_identifier_name(car) {
            return MACRO_DEFINITION_FORMS
                .iter()
                .any(|&form| form == name.as_ref());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_to_vec_proper_list() {
        let list = vec_to_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let result = list_to_vec(&list).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_list_to_vec_empty() {
        let result = list_to_vec(&Value::Null).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_list_to_vec_improper() {
        // Improper list: (1 . 2)
        let improper = Value::Pair(Rc::new(RefCell::new((
            Value::Integer(1),
            Value::Integer(2),
        ))));
        let result = list_to_vec(&improper);
        assert!(result.is_err());
    }

    #[test]
    fn test_vec_to_list_roundtrip() {
        let original = vec![Value::Integer(1), Value::Integer(2)];
        let list = vec_to_list(original.clone());
        let result = list_to_vec(&list).unwrap();
        assert_eq!(result.len(), original.len());
    }

    #[test]
    fn test_is_named() {
        let sym = Value::Symbol(Rc::from("foo"));
        assert!(is_named(&sym, "foo"));
        assert!(!is_named(&sym, "bar"));

        let id = Value::Identifier(Box::new(patina_runtime::IdentifierData {
            name: Rc::from("foo"),
            scopes: patina_runtime::ScopeSet::new(),
        }));
        assert!(is_named(&id, "foo"));
    }

    #[test]
    fn test_is_ellipsis_symbol() {
        let ellipsis = Value::Symbol(Rc::from("..."));
        assert!(is_ellipsis_symbol(&ellipsis));

        let not_ellipsis = Value::Symbol(Rc::from("foo"));
        assert!(!is_ellipsis_symbol(&not_ellipsis));
    }

    #[test]
    fn test_is_wildcard_symbol() {
        let wildcard = Value::Symbol(Rc::from("_"));
        assert!(is_wildcard_symbol(&wildcard));

        let not_wildcard = Value::Symbol(Rc::from("foo"));
        assert!(!is_wildcard_symbol(&not_wildcard));
    }

    #[test]
    fn test_is_macro_definition_form() {
        let syntax_rules = Value::Pair(Rc::new(RefCell::new((
            Value::Symbol(Rc::from("syntax-rules")),
            Value::Null,
        ))));
        assert!(is_macro_definition_form(&syntax_rules));

        let define_syntax = Value::Pair(Rc::new(RefCell::new((
            Value::Symbol(Rc::from("define-syntax")),
            Value::Null,
        ))));
        assert!(is_macro_definition_form(&define_syntax));

        let not_macro = Value::Pair(Rc::new(RefCell::new((
            Value::Symbol(Rc::from("lambda")),
            Value::Null,
        ))));
        assert!(!is_macro_definition_form(&not_macro));
    }

    #[test]
    fn test_scheme_list_iter_proper_list() {
        let list = vec_to_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let iter = SchemeListIter::new(&list);
        let collected: Result<Vec<_>, _> = iter.collect();
        let values = collected.unwrap();
        assert_eq!(values.len(), 3);
        assert!(matches!(values[0], Value::Integer(1)));
        assert!(matches!(values[1], Value::Integer(2)));
        assert!(matches!(values[2], Value::Integer(3)));
    }

    #[test]
    fn test_scheme_list_iter_empty() {
        let iter = SchemeListIter::new(&Value::Null);
        let collected: Result<Vec<_>, _> = iter.collect();
        let values = collected.unwrap();
        assert!(values.is_empty());
    }

    #[test]
    fn test_scheme_list_iter_improper() {
        // Improper list: (1 . 2)
        let improper = Value::Pair(Rc::new(RefCell::new((
            Value::Integer(1),
            Value::Integer(2),
        ))));
        let iter = SchemeListIter::new(&improper);
        let collected: Result<Vec<_>, _> = iter.collect();
        assert!(collected.is_err());
    }

    #[test]
    fn test_scheme_list_iter_len() {
        let list = vec_to_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(SchemeListIter::len(&list), Some(3));
        assert_eq!(SchemeListIter::len(&Value::Null), Some(0));

        // Improper list
        let improper = Value::Pair(Rc::new(RefCell::new((
            Value::Integer(1),
            Value::Integer(2),
        ))));
        assert_eq!(SchemeListIter::len(&improper), None);
    }

    #[test]
    fn test_scheme_list_iter_is_proper_list() {
        let list = vec_to_list(vec![Value::Integer(1), Value::Integer(2)]);
        assert!(SchemeListIter::is_proper_list(&list));
        assert!(SchemeListIter::is_proper_list(&Value::Null));

        // Improper list
        let improper = Value::Pair(Rc::new(RefCell::new((
            Value::Integer(1),
            Value::Integer(2),
        ))));
        assert!(!SchemeListIter::is_proper_list(&improper));
    }
}
