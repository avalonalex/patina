//! Parser for syntax-rules forms
//!
//! This module provides utilities for parsing the structure of `syntax-rules` forms,
//! extracting literals, custom ellipsis, and rules.
//!
//! Used by both TestExpander (for tests) and can be used by patina-frontend's desugarer.

use crate::macro_expander::IdentifierKey;
use crate::macro_expander::utils::{get_identifier_name_tagged, list_to_vec_tagged};
use patina_core::{Cx, Value};
use std::rc::Rc;

/// Parsed components of a syntax-rules form, in the context it was read in
#[derive(Debug, Clone)]
pub struct ParsedSyntaxRules<'gc> {
    /// Optional custom ellipsis symbol (SRFI-46)
    pub custom_ellipsis: Option<Rc<str>>,
    /// List of literal identifiers, with the scopes they were written with
    pub literals: Vec<IdentifierKey>,
    /// List of (pattern, template) pairs
    pub rules: Vec<(Value<'gc>, Value<'gc>)>,
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
/// * `form` - The syntax-rules form (must be a list starting with `syntax-rules`)
/// * `cx` - The context the form is read through
///
/// # Returns
/// * `Ok(ParsedSyntaxRules)` - Successfully parsed components
/// * `Err(SyntaxRulesParseError)` - Parse error with description
pub fn parse_syntax_rules<'gc>(
    form: Value<'gc>,
    cx: &Cx<'gc>,
) -> Result<ParsedSyntaxRules<'gc>, SyntaxRulesParseError> {
    // Convert to vector for easier indexing
    let list = list_to_vec_tagged(form, cx).map_err(SyntaxRulesParseError)?;

    if list.is_empty() {
        return Err(SyntaxRulesParseError(
            "syntax-rules must be a non-empty list".to_string(),
        ));
    }

    // Check first element is 'syntax-rules
    if !is_syntax_rules_keyword(list[0], cx) {
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
    let (custom_ellipsis, literals_index) = if list[1].is_pair() || list[1].is_null() {
        (None, 1)
    } else if let Some(name) = get_identifier_name_tagged(list[1], cx) {
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
    let literals = parse_literals_list(list[literals_index], cx)?;

    // Parse rules
    let rules_start = literals_index + 1;
    let rules = if list.len() > rules_start {
        parse_rules(&list[rules_start..], cx)?
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

/// Check if a value is the `syntax-rules` keyword
fn is_syntax_rules_keyword<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> bool {
    cx.get_symbol_or_identifier_name(tv)
        .is_some_and(|n| &*n == "syntax-rules")
}

/// Parse the literals list: `(lit1 lit2 ...)`
fn parse_literals_list<'gc>(
    expr: Value<'gc>,
    cx: &Cx<'gc>,
) -> Result<Vec<IdentifierKey>, SyntaxRulesParseError> {
    // Refuses a circular list with an improper one; walking it never ended
    // (#459).
    let Some(elements) = cx.list_to_vec(expr) else {
        return Err(SyntaxRulesParseError(
            "Literals must be a proper list".to_string(),
        ));
    };
    let mut literals = Vec::new();
    for car in elements {
        // Keep the scopes, not just the name: literal membership is decided by
        // identifier identity.
        if let Some(key) = IdentifierKey::from_cx(car, cx) {
            literals.push(key);
        } else {
            return Err(SyntaxRulesParseError(
                "Literals must be symbols".to_string(),
            ));
        }
    }

    Ok(literals)
}

/// Parse rules from a slice of values: `((pattern template) ...)`
fn parse_rules<'gc>(
    rules_slice: &[Value<'gc>],
    cx: &Cx<'gc>,
) -> Result<Vec<(Value<'gc>, Value<'gc>)>, SyntaxRulesParseError> {
    let mut rules = Vec::new();

    for &rule in rules_slice {
        let rule_list = list_to_vec_tagged(rule, cx)
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
    use patina_core::{GcDriver, Instance, new_shared_heap};

    /// Run `f` with a context over a fresh heap.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        let heap = new_shared_heap();
        let instance = Instance::default();
        let mut driver = GcDriver::acquire(&heap).expect("the carrier's driver is free");
        driver.mutate(&instance, f)
    }

    #[test]
    fn test_parse_simple_syntax_rules() {
        with_cx(|cx| {
            // (syntax-rules () ((m x) x))
            let m = cx.intern_symbol("m");
            let x = cx.intern_symbol("x");
            let pattern = cx.list_from_iter([m, x]);
            let rule = cx.list_from_iter([pattern, x]);
            let sr = cx.intern_symbol("syntax-rules");
            let form = cx.list_from_iter([sr, Value::NULL, rule]);

            let parsed = parse_syntax_rules(form, cx).unwrap();
            assert!(parsed.custom_ellipsis.is_none());
            assert!(parsed.literals.is_empty());
            assert_eq!(parsed.rules.len(), 1);
        });
    }

    #[test]
    fn test_parse_with_literals() {
        with_cx(|cx| {
            // (syntax-rules (else =>) ((m x) x))
            let else_sym = cx.intern_symbol("else");
            let arrow = cx.intern_symbol("=>");
            let literals = cx.list_from_iter([else_sym, arrow]);
            let m = cx.intern_symbol("m");
            let x = cx.intern_symbol("x");
            let pattern = cx.list_from_iter([m, x]);
            let rule = cx.list_from_iter([pattern, x]);
            let sr = cx.intern_symbol("syntax-rules");
            let form = cx.list_from_iter([sr, literals, rule]);

            let parsed = parse_syntax_rules(form, cx).unwrap();
            assert!(parsed.custom_ellipsis.is_none());
            assert_eq!(parsed.literals.len(), 2);
            assert_eq!(parsed.literals[0].name.as_ref(), "else");
            assert_eq!(parsed.literals[1].name.as_ref(), "=>");
        });
    }

    #[test]
    fn test_parse_srfi46_custom_ellipsis() {
        with_cx(|cx| {
            // (syntax-rules ::: () ((m x) x))
            let m = cx.intern_symbol("m");
            let x = cx.intern_symbol("x");
            let pattern = cx.list_from_iter([m, x]);
            let rule = cx.list_from_iter([pattern, x]);
            let sr = cx.intern_symbol("syntax-rules");
            let ellipsis = cx.intern_symbol(":::");
            let form = cx.list_from_iter([sr, ellipsis, Value::NULL, rule]);

            let parsed = parse_syntax_rules(form, cx).unwrap();
            assert_eq!(
                parsed.custom_ellipsis.as_ref().map(|s| s.as_ref()),
                Some(":::")
            );
            assert!(parsed.literals.is_empty());
        });
    }

    #[test]
    fn test_parse_error_no_rules() {
        with_cx(|cx| {
            // (syntax-rules ()) - missing rules
            let sr = cx.intern_symbol("syntax-rules");
            let form = cx.list_from_iter([sr, Value::NULL]);
            assert!(parse_syntax_rules(form, cx).is_err());
        });
    }

    #[test]
    fn test_parse_error_not_syntax_rules() {
        with_cx(|cx| {
            // (lambda () x) - not syntax-rules
            let lambda = cx.intern_symbol("lambda");
            let x = cx.intern_symbol("x");
            let form = cx.list_from_iter([lambda, Value::NULL, x]);
            assert!(parse_syntax_rules(form, cx).is_err());
        });
    }
}
