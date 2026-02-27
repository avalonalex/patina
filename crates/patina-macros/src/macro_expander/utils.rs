//! Shared utilities for the macro system
//!
//! This module provides common constants, helpers, and utility functions
//! used across the macro compiler, matcher, and expander.

use patina_core::{Heap, TaggedValue};
use patina_runtime::PVRef;
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
// TaggedValue List/Vector Conversion Utilities
// ============================================================================

/// Error indicating an improper list was encountered during iteration
#[derive(Debug, Clone, Copy)]
pub struct ImproperListError;

impl std::fmt::Display for ImproperListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "improper list encountered")
    }
}

impl std::error::Error for ImproperListError {}

/// Iterator over a Scheme list represented as TaggedValue
///
/// This iterator traverses a Scheme list stored on the heap, yielding each
/// element as a TaggedValue. Works directly with TaggedValue and requires
/// a heap reference.
pub struct TaggedListIter<'a> {
    current: TaggedValue,
    heap: &'a Heap,
}

impl<'a> TaggedListIter<'a> {
    /// Create a new iterator over a TaggedValue list
    pub fn new(list: TaggedValue, heap: &'a Heap) -> Self {
        Self {
            current: list,
            heap,
        }
    }

    /// Count the length of a TaggedValue list without collecting
    ///
    /// Returns None if not a proper list.
    pub fn len(list: TaggedValue, heap: &Heap) -> Option<usize> {
        let mut count = 0;
        let mut current = list;
        loop {
            if current == TaggedValue::NULL {
                return Some(count);
            } else if current.is_pair() {
                count += 1;
                current = heap.cdr(current);
            } else {
                return None;
            }
        }
    }

    /// Check if a TaggedValue is a proper list (ends with Null)
    pub fn is_proper_list(list: TaggedValue, heap: &Heap) -> bool {
        Self::len(list, heap).is_some()
    }
}

impl<'a> Iterator for TaggedListIter<'a> {
    type Item = Result<TaggedValue, ImproperListError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == TaggedValue::NULL {
            None
        } else if self.current.is_pair() {
            let car = self.heap.car(self.current);
            let cdr = self.heap.cdr(self.current);
            self.current = cdr;
            Some(Ok(car))
        } else {
            // Improper list - return error and stop
            let err = ImproperListError;
            self.current = TaggedValue::NULL;
            Some(Err(err))
        }
    }
}

/// Convert a TaggedValue proper list to a Vec
///
/// Returns an error message if the value is not a proper list.
pub fn list_to_vec_tagged(value: TaggedValue, heap: &Heap) -> Result<Vec<TaggedValue>, String> {
    let mut result = Vec::new();
    let mut current = value;

    loop {
        if current == TaggedValue::NULL {
            break;
        } else if current.is_pair() {
            result.push(heap.car(current));
            current = heap.cdr(current);
        } else {
            return Err(format!("Expected proper list, got {:?}", current));
        }
    }

    Ok(result)
}

/// Convert a Vec to a TaggedValue proper list
pub fn vec_to_list_tagged(values: Vec<TaggedValue>, heap: &mut Heap) -> TaggedValue {
    let mut result = TaggedValue::NULL;
    for value in values.into_iter().rev() {
        result = heap.alloc_pair(value, result);
    }
    result
}

/// Convert a TaggedValue list to a Vec, allowing improper lists
///
/// Returns (elements, tail) where tail is the final cdr (NULL for proper lists)
pub fn list_to_vec_with_tail_tagged(
    value: TaggedValue,
    heap: &Heap,
) -> (Vec<TaggedValue>, TaggedValue) {
    let mut result = Vec::new();
    let mut current = value;

    loop {
        if current == TaggedValue::NULL {
            return (result, TaggedValue::NULL);
        } else if current.is_pair() {
            result.push(heap.car(current));
            current = heap.cdr(current);
        } else {
            // Improper list - return the tail
            return (result, current);
        }
    }
}

// ============================================================================
// TaggedValue Symbol/Identifier Name Extraction
// ============================================================================

/// Extract the name from a TaggedValue Symbol or Identifier
///
/// Returns None for non-identifier values.
/// Uses Heap's built-in method for efficiency.
pub fn get_identifier_name_tagged(tv: TaggedValue, heap: &Heap) -> Option<Rc<str>> {
    heap.get_symbol_or_identifier_name(tv).map(Rc::from)
}

/// Check if a TaggedValue is a specific named symbol or identifier
///
/// Uses Heap's built-in method for efficiency.
pub fn is_named_tagged(tv: TaggedValue, name: &str, heap: &Heap) -> bool {
    heap.is_named(tv, name)
}

/// Check if a TaggedValue is the ellipsis symbol (...)
pub fn is_ellipsis_tagged(tv: TaggedValue, heap: &Heap) -> bool {
    is_named_tagged(tv, ELLIPSIS, heap)
}

/// Check if a TaggedValue is the wildcard/underscore symbol (_)
pub fn is_wildcard_tagged(tv: TaggedValue, heap: &Heap) -> bool {
    is_named_tagged(tv, WILDCARD, heap)
}

/// Check if a TaggedValue is a pair (list node)
#[inline]
pub fn is_pair_tagged(tv: TaggedValue) -> bool {
    tv.is_pair()
}

/// Check if a TaggedValue is null (empty list)
#[inline]
pub fn is_null_tagged(tv: TaggedValue) -> bool {
    tv == TaggedValue::NULL
}

/// Check if a TaggedValue is a list (pair or null)
#[inline]
pub fn is_list_tagged(tv: TaggedValue) -> bool {
    tv.is_pair() || tv == TaggedValue::NULL
}

/// Check if a TaggedValue is a vector
#[inline]
pub fn is_vector_tagged(tv: TaggedValue) -> bool {
    tv.is_vector()
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
        Pattern::Literal(tv) => format!("{:?}", tv),
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
        Pattern::Literal(tv) => format!("{:?}", tv),
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
        Template::Literal(tv) => format!("{:?}", tv),
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
