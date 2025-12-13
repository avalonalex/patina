//! Parser for syntax-rules forms
//!
//! This module provides utilities for parsing the structure of `syntax-rules` forms,
//! extracting literals, custom ellipsis, and rules.
//!
//! Used by both TestExpander (for tests) and can be used by patina-frontend's desugarer.

use patina_runtime::Value;
use std::rc::Rc;

/// Parsed components of a syntax-rules form
#[derive(Debug, Clone)]
pub struct ParsedSyntaxRules {
    /// Optional custom ellipsis symbol (SRFI-46)
    pub custom_ellipsis: Option<Rc<str>>,
    /// List of literal identifiers
    pub literals: Vec<Rc<str>>,
    /// List of (pattern, template) pairs
    pub rules: Vec<(Value, Value)>,
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
/// * `form` - The syntax-rules form as a Value (must be a list starting with `syntax-rules`)
///
/// # Returns
/// * `Ok(ParsedSyntaxRules)` - Successfully parsed components
/// * `Err(SyntaxRulesParseError)` - Parse error with description
pub fn parse_syntax_rules(form: &Value) -> Result<ParsedSyntaxRules, SyntaxRulesParseError> {
    // Convert to vector for easier indexing
    let list = list_to_vec(form).map_err(SyntaxRulesParseError)?;

    if list.is_empty() {
        return Err(SyntaxRulesParseError(
            "syntax-rules must be a non-empty list".to_string(),
        ));
    }

    // Check first element is 'syntax-rules
    if !is_syntax_rules_keyword(&list[0]) {
        return Err(SyntaxRulesParseError(format!(
            "Expected syntax-rules, got {}",
            list[0]
        )));
    }

    if list.len() < 2 {
        return Err(SyntaxRulesParseError(
            "syntax-rules must have literals and rules".to_string(),
        ));
    }

    // Detect form: standard vs SRFI-46 (custom ellipsis)
    let (custom_ellipsis, literals_index) = match &list[1] {
        // If it's a list or null, it's the literals list (standard form)
        Value::Pair(_) | Value::Null => (None, 1),
        // If it's a symbol or identifier, it's a custom ellipsis
        Value::Symbol(s) => (Some(s.clone()), 2),
        Value::Identifier(id) => (Some(id.name.clone()), 2),
        _ => {
            return Err(SyntaxRulesParseError(
                "Expected literals list or custom ellipsis".to_string(),
            ));
        }
    };

    // Validate we have enough elements for custom ellipsis form
    if custom_ellipsis.is_some() && list.len() < 3 {
        return Err(SyntaxRulesParseError(
            "syntax-rules with custom ellipsis must have literals and rules".to_string(),
        ));
    }

    // Parse literals list
    let literals = parse_literals_list(&list[literals_index])?;

    // Parse rules
    let rules_start = literals_index + 1;
    let rules = if list.len() > rules_start {
        parse_rules(&list[rules_start..])?
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
fn is_syntax_rules_keyword(value: &Value) -> bool {
    match value {
        Value::Symbol(s) => s.as_ref() == "syntax-rules",
        Value::Identifier(id) => id.name.as_ref() == "syntax-rules",
        _ => false,
    }
}

/// Parse the literals list: `(lit1 lit2 ...)`
fn parse_literals_list(expr: &Value) -> Result<Vec<Rc<str>>, SyntaxRulesParseError> {
    let mut literals = Vec::new();
    let mut current = expr.clone();

    while let Value::Pair(pair) = current {
        let borrowed = pair.borrow();
        match &borrowed.0 {
            Value::Symbol(s) => literals.push(s.clone()),
            Value::Identifier(id) => literals.push(id.name.clone()),
            _ => {
                return Err(SyntaxRulesParseError(
                    "Literals must be symbols".to_string(),
                ));
            }
        }
        current = borrowed.1.clone();
    }

    if !matches!(current, Value::Null) {
        return Err(SyntaxRulesParseError(
            "Literals must be a proper list".to_string(),
        ));
    }

    Ok(literals)
}

/// Parse rules from a slice of Values: `((pattern template) ...)`
fn parse_rules(rules_slice: &[Value]) -> Result<Vec<(Value, Value)>, SyntaxRulesParseError> {
    let mut rules = Vec::new();

    for rule in rules_slice {
        let rule_list = list_to_vec(rule)
            .map_err(|_| SyntaxRulesParseError("Each rule must be a list".to_string()))?;

        if rule_list.len() != 2 {
            return Err(SyntaxRulesParseError(
                "Each rule must have exactly pattern and template".to_string(),
            ));
        }

        rules.push((rule_list[0].clone(), rule_list[1].clone()));
    }

    Ok(rules)
}

/// Convert a Scheme list to a Vec (local helper to avoid circular deps)
fn list_to_vec(value: &Value) -> Result<Vec<Value>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn make_list(items: Vec<Value>) -> Value {
        let mut result = Value::Null;
        for item in items.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((item, result))));
        }
        result
    }

    fn sym(s: &str) -> Value {
        Value::Symbol(Rc::from(s))
    }

    #[test]
    fn test_parse_simple_syntax_rules() {
        // (syntax-rules () ((m x) x))
        let pattern = make_list(vec![sym("m"), sym("x")]);
        let template = sym("x");
        let rule = make_list(vec![pattern, template]);
        let form = make_list(vec![sym("syntax-rules"), Value::Null, rule]);

        let parsed = parse_syntax_rules(&form).unwrap();
        assert!(parsed.custom_ellipsis.is_none());
        assert!(parsed.literals.is_empty());
        assert_eq!(parsed.rules.len(), 1);
    }

    #[test]
    fn test_parse_with_literals() {
        // (syntax-rules (else =>) ((m x) x))
        let literals = make_list(vec![sym("else"), sym("=>")]);
        let pattern = make_list(vec![sym("m"), sym("x")]);
        let template = sym("x");
        let rule = make_list(vec![pattern, template]);
        let form = make_list(vec![sym("syntax-rules"), literals, rule]);

        let parsed = parse_syntax_rules(&form).unwrap();
        assert!(parsed.custom_ellipsis.is_none());
        assert_eq!(parsed.literals.len(), 2);
        assert_eq!(parsed.literals[0].as_ref(), "else");
        assert_eq!(parsed.literals[1].as_ref(), "=>");
    }

    #[test]
    fn test_parse_srfi46_custom_ellipsis() {
        // (syntax-rules ::: () ((m x) x))
        let pattern = make_list(vec![sym("m"), sym("x")]);
        let template = sym("x");
        let rule = make_list(vec![pattern, template]);
        let form = make_list(vec![sym("syntax-rules"), sym(":::"), Value::Null, rule]);

        let parsed = parse_syntax_rules(&form).unwrap();
        assert_eq!(
            parsed.custom_ellipsis.as_ref().map(|s| s.as_ref()),
            Some(":::")
        );
        assert!(parsed.literals.is_empty());
    }

    #[test]
    fn test_parse_error_no_rules() {
        // (syntax-rules ()) - missing rules
        let form = make_list(vec![sym("syntax-rules"), Value::Null]);
        let result = parse_syntax_rules(&form);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_not_syntax_rules() {
        // (lambda () x) - not syntax-rules
        let form = make_list(vec![sym("lambda"), Value::Null, sym("x")]);
        let result = parse_syntax_rules(&form);
        assert!(result.is_err());
    }
}
