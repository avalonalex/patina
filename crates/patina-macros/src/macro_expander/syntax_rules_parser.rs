//! Parser for syntax-rules forms
//!
//! This module provides utilities for parsing the structure of `syntax-rules` forms,
//! extracting literals, custom ellipsis, and rules.
//!
//! Used by both TestExpander (for tests) and can be used by patina-frontend's desugarer.

use crate::macro_expander::utils::{get_identifier_name_tagged, list_to_vec_tagged};
use patina_core::{Heap, LiteralSpec, TaggedValue};
use std::rc::Rc;

/// Parsed components of a syntax-rules form (TaggedValue-based)
#[derive(Debug, Clone)]
pub struct ParsedSyntaxRules {
    /// Optional custom ellipsis symbol (SRFI-46)
    pub custom_ellipsis: Option<Rc<str>>,
    /// List of literal identifiers, with the scopes they were written with
    pub literals: Vec<LiteralSpec>,
    /// List of (pattern, template) pairs as TaggedValue
    pub rules: Vec<(TaggedValue, TaggedValue)>,
}

/// Error type for syntax-rules parsing
#[derive(Debug, Clone)]
pub struct SyntaxRulesParseError(pub String);

impl std::fmt::Display for SyntaxRulesParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SyntaxRulesParseError {}

/// Parse a syntax-rules form into its components
///
/// Handles both standard and SRFI-46 forms:
/// - Standard: `(syntax-rules (literals) (pattern template) ...)`
/// - SRFI-46:  `(syntax-rules <ellipsis> (literals) (pattern template) ...)`
///
/// # Arguments
/// * `form` - The syntax-rules form as a TaggedValue (must be a list starting with `syntax-rules`)
/// * `heap` - The heap for reading TaggedValue data
///
/// # Returns
/// * `Ok(ParsedSyntaxRules)` - Successfully parsed components
/// * `Err(SyntaxRulesParseError)` - Parse error with description
pub fn parse_syntax_rules(
    form: TaggedValue,
    heap: &Heap,
) -> Result<ParsedSyntaxRules, SyntaxRulesParseError> {
    // Convert to vector for easier indexing
    let list = list_to_vec_tagged(form, heap).map_err(SyntaxRulesParseError)?;

    if list.is_empty() {
        return Err(SyntaxRulesParseError(
            "syntax-rules must be a non-empty list".to_string(),
        ));
    }

    // Check first element is 'syntax-rules
    if !is_syntax_rules_keyword(list[0], heap) {
        return Err(SyntaxRulesParseError(format!(
            "Expected syntax-rules, got {:?}",
            list[0]
        )));
    }

    if list.len() < 2 {
        return Err(SyntaxRulesParseError(
            "syntax-rules must have literals and rules".to_string(),
        ));
    }

    // Detect form: standard vs SRFI-46 (custom ellipsis)
    // If element 1 is a list or null, it's the literals list (standard form)
    // If it's a symbol or identifier, it's a custom ellipsis
    let (custom_ellipsis, literals_index) = if list[1].is_pair() || list[1] == TaggedValue::NULL {
        (None, 1)
    } else if let Some(name) = get_identifier_name_tagged(list[1], heap) {
        (Some(name), 2)
    } else {
        return Err(SyntaxRulesParseError(
            "Expected literals list or custom ellipsis".to_string(),
        ));
    };

    // Validate we have enough elements for custom ellipsis form
    if custom_ellipsis.is_some() && list.len() < 3 {
        return Err(SyntaxRulesParseError(
            "syntax-rules with custom ellipsis must have literals and rules".to_string(),
        ));
    }

    // Parse literals list
    let literals = parse_literals_list(list[literals_index], heap)?;

    // Parse rules
    let rules_start = literals_index + 1;
    let rules = if list.len() > rules_start {
        parse_rules(&list[rules_start..], heap)?
    } else {
        Vec::new()
    };

    if rules.is_empty() {
        return Err(SyntaxRulesParseError(
            "syntax-rules must have at least one rule".to_string(),
        ));
    }

    Ok(ParsedSyntaxRules {
        custom_ellipsis,
        literals,
        rules,
    })
}

/// Check if a TaggedValue is the `syntax-rules` keyword
fn is_syntax_rules_keyword(tv: TaggedValue, heap: &Heap) -> bool {
    heap.get_symbol_or_identifier_name(tv)
        .map(|n| n == "syntax-rules")
        .unwrap_or(false)
}

/// Parse the literals list: `(lit1 lit2 ...)`
fn parse_literals_list(
    expr: TaggedValue,
    heap: &Heap,
) -> Result<Vec<LiteralSpec>, SyntaxRulesParseError> {
    let mut literals = Vec::new();
    let mut current = expr;

    while current.is_pair() {
        let car = heap.car(current);
        if let Some(name) = get_identifier_name_tagged(car, heap) {
            // Keep the scopes: literal membership is by identifier identity.
            let scopes = heap
                .get_identifier_data_any(car)
                .map(|(_, scopes)| scopes)
                .unwrap_or_default();
            literals.push(LiteralSpec::new(name, scopes));
        } else {
            return Err(SyntaxRulesParseError(
                "Literals must be symbols".to_string(),
            ));
        }
        current = heap.cdr(current);
    }

    if current != TaggedValue::NULL {
        return Err(SyntaxRulesParseError(
            "Literals must be a proper list".to_string(),
        ));
    }

    Ok(literals)
}

/// Parse rules from a slice of TaggedValues: `((pattern template) ...)`
fn parse_rules(
    rules_slice: &[TaggedValue],
    heap: &Heap,
) -> Result<Vec<(TaggedValue, TaggedValue)>, SyntaxRulesParseError> {
    let mut rules = Vec::new();

    for &rule in rules_slice {
        let rule_list = list_to_vec_tagged(rule, heap)
            .map_err(|_| SyntaxRulesParseError("Each rule must be a list".to_string()))?;

        if rule_list.len() != 2 {
            return Err(SyntaxRulesParseError(
                "Each rule must have exactly pattern and template".to_string(),
            ));
        }

        rules.push((rule_list[0], rule_list[1]));
    }

    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::SharedHeap;
    use std::cell::RefCell;

    fn make_heap() -> SharedHeap {
        Rc::new(RefCell::new(patina_core::Heap::new()))
    }

    fn sym(name: &str, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().intern_symbol(name)
    }

    fn make_list_tagged(items: Vec<TaggedValue>, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().list_from_iter(items)
    }

    #[test]
    fn test_parse_simple_syntax_rules() {
        let heap = make_heap();
        // (syntax-rules () ((m x) x))
        let m = sym("m", &heap);
        let x = sym("x", &heap);
        let pattern = make_list_tagged(vec![m, x], &heap);
        let template = x;
        let rule = make_list_tagged(vec![pattern, template], &heap);
        let sr = sym("syntax-rules", &heap);
        let form = make_list_tagged(vec![sr, TaggedValue::NULL, rule], &heap);

        let parsed = parse_syntax_rules(form, &heap.borrow()).unwrap();
        assert!(parsed.custom_ellipsis.is_none());
        assert!(parsed.literals.is_empty());
        assert_eq!(parsed.rules.len(), 1);
    }

    #[test]
    fn test_parse_with_literals() {
        let heap = make_heap();
        // (syntax-rules (else =>) ((m x) x))
        let else_sym = sym("else", &heap);
        let arrow = sym("=>", &heap);
        let literals = make_list_tagged(vec![else_sym, arrow], &heap);
        let m = sym("m", &heap);
        let x = sym("x", &heap);
        let pattern = make_list_tagged(vec![m, x], &heap);
        let template = x;
        let rule = make_list_tagged(vec![pattern, template], &heap);
        let sr = sym("syntax-rules", &heap);
        let form = make_list_tagged(vec![sr, literals, rule], &heap);

        let parsed = parse_syntax_rules(form, &heap.borrow()).unwrap();
        assert!(parsed.custom_ellipsis.is_none());
        assert_eq!(parsed.literals.len(), 2);
        assert_eq!(parsed.literals[0].name.as_ref(), "else");
        assert_eq!(parsed.literals[1].name.as_ref(), "=>");
    }

    #[test]
    fn test_parse_srfi46_custom_ellipsis() {
        let heap = make_heap();
        // (syntax-rules ::: () ((m x) x))
        let m = sym("m", &heap);
        let x = sym("x", &heap);
        let pattern = make_list_tagged(vec![m, x], &heap);
        let template = x;
        let rule = make_list_tagged(vec![pattern, template], &heap);
        let sr = sym("syntax-rules", &heap);
        let ellipsis = sym(":::", &heap);
        let form = make_list_tagged(vec![sr, ellipsis, TaggedValue::NULL, rule], &heap);

        let parsed = parse_syntax_rules(form, &heap.borrow()).unwrap();
        assert_eq!(
            parsed.custom_ellipsis.as_ref().map(|s| s.as_ref()),
            Some(":::")
        );
        assert!(parsed.literals.is_empty());
    }

    #[test]
    fn test_parse_error_no_rules() {
        let heap = make_heap();
        // (syntax-rules ()) - missing rules
        let sr = sym("syntax-rules", &heap);
        let form = make_list_tagged(vec![sr, TaggedValue::NULL], &heap);
        let result = parse_syntax_rules(form, &heap.borrow());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_not_syntax_rules() {
        let heap = make_heap();
        // (lambda () x) - not syntax-rules
        let lambda = sym("lambda", &heap);
        let x = sym("x", &heap);
        let form = make_list_tagged(vec![lambda, TaggedValue::NULL, x], &heap);
        let result = parse_syntax_rules(form, &heap.borrow());
        assert!(result.is_err());
    }
}
