//! Clean macro expander interface and test helpers
//!
//! This module provides:
//! - `TestExpander`: A test helper for creating and testing macros
//!
//! The TestExpander uses `patina_macros` for compilation and expansion,
//! and provides convenient methods for testing macro behavior.

use crate::error::FrontendError;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// Uses `patina_macros` for the underlying macro compilation and expansion.
pub struct TestExpander {
    compiled: patina_macros::CompiledMacro,
}

impl TestExpander {
    /// Create a test expander from a macro definition string
    ///
    /// # Example
    /// ```ignore
    /// let expander = TestExpander::from_definition(
    ///     "my-macro",
    ///     r#"
    ///     (syntax-rules ()
    ///       ((my-macro x) (list x x)))
    ///     "#
    /// ).unwrap();
    /// ```
    pub fn from_definition(name: &str, syntax_rules: &str) -> Result<Self, FrontendError> {
        use crate::parser::Parser;

        // Parse the syntax-rules form
        let mut parser = Parser::new(syntax_rules)?;
        let syntax_rules_form = parser.parse()?;

        // Parse the syntax-rules structure: (syntax-rules (literals) (pattern template) ...)
        let Value::Pair(p1) = &syntax_rules_form else {
            return Err(FrontendError::InvalidSyntax(
                "syntax-rules must be a list".to_string(),
            ));
        };
        let b1 = p1.borrow();
        // Check for syntax-rules keyword - can be Symbol or Identifier (from macro expansion)
        let is_syntax_rules = match &b1.0 {
            Value::Symbol(s) => s.as_ref() == "syntax-rules",
            Value::Identifier(id) => id.name.as_ref() == "syntax-rules",
            _ => false,
        };
        if !is_syntax_rules {
            return Err(FrontendError::InvalidSyntax(format!(
                "Expected syntax-rules, got {}",
                b1.0
            )));
        }

        // Extract (literals) and rules
        let Value::Pair(p2) = &b1.1 else {
            return Err(FrontendError::InvalidSyntax(
                "syntax-rules must have literals and rules".to_string(),
            ));
        };
        let b2 = p2.borrow();
        let literals_form = b2.0.clone();
        let rules_form = b2.1.clone();

        // Parse literals list
        let mut literals = Vec::new();
        let mut current = literals_form;
        while let Value::Pair(p) = current {
            let borrowed = p.borrow();
            if let Value::Symbol(s) = &borrowed.0 {
                literals.push(s.clone());
            } else {
                return Err(FrontendError::InvalidSyntax(
                    "Literals must be symbols".to_string(),
                ));
            }
            current = borrowed.1.clone();
        }
        if !matches!(current, Value::Null) {
            return Err(FrontendError::InvalidSyntax(
                "Literals must be a proper list".to_string(),
            ));
        }

        // Parse rules as (pattern template) pairs
        let mut rules = Vec::new();
        let mut current = rules_form;
        while let Value::Pair(rule_pair) = current {
            let rule_borrowed = rule_pair.borrow();
            let Value::Pair(rule_p) = &rule_borrowed.0 else {
                return Err(FrontendError::InvalidSyntax(
                    "Each rule must be a list".to_string(),
                ));
            };
            let rule_p_borrowed = rule_p.borrow();
            let pattern = rule_p_borrowed.0.clone();
            let Value::Pair(tmpl_p) = &rule_p_borrowed.1 else {
                return Err(FrontendError::InvalidSyntax(
                    "Rule must have a template".to_string(),
                ));
            };
            let tmpl_p_borrowed = tmpl_p.borrow();
            let template = tmpl_p_borrowed.0.clone();
            if !matches!(tmpl_p_borrowed.1, Value::Null) {
                return Err(FrontendError::InvalidSyntax(
                    "Each rule must have exactly pattern and template".to_string(),
                ));
            }
            rules.push((pattern, template));
            current = rule_borrowed.1.clone();
        }
        if !matches!(current, Value::Null) {
            return Err(FrontendError::InvalidSyntax(
                "Rules must be a proper list".to_string(),
            ));
        }

        // Create test environment for compilation (captures bindings for hygiene)
        let test_env = Rc::new(Environment::new());

        // Compile it with the test environment using patina_macros
        let mut compiler = patina_macros::Compiler::with_env(literals, None, test_env);
        let compiled = compiler
            .compile_macro(name.into(), rules)
            .map_err(|e| FrontendError::InvalidSyntax(e.to_string()))?;

        Ok(Self { compiled })
    }

    /// Create a test expander from a compiled macro
    pub fn from_compiled(compiled: patina_macros::CompiledMacro) -> Self {
        Self { compiled }
    }

    /// Assert that the given input expands to the expected output
    ///
    /// # Example
    /// ```ignore
    /// expander.assert_expands_to(
    ///     "(my-macro 42)",
    ///     "(list 42 42)"
    /// ).unwrap();
    /// ```
    pub fn assert_expands_to(&self, input: &str, expected: &str) -> Result<(), String> {
        use crate::parser::Parser;

        // Parse input
        let mut parser = Parser::new(input).map_err(|e| format!("Failed to parse input: {}", e))?;
        let input_form = parser
            .parse()
            .map_err(|e| format!("Failed to parse input: {}", e))?;

        // Parse expected
        let mut parser =
            Parser::new(expected).map_err(|e| format!("Failed to parse expected: {}", e))?;
        let expected_form = parser
            .parse()
            .map_err(|e| format!("Failed to parse expected: {}", e))?;

        // Expand using patina_macros
        let expanded = patina_macros::expand_macro(&self.compiled, &input_form)
            .map_err(|e| format!("Expansion failed: {}", e))?;

        // Compare (ignoring gensym differences)
        if Self::forms_equal_ignoring_gensym(&expanded, &expected_form) {
            Ok(())
        } else {
            Err(format!(
                "Expansion mismatch:\nExpected: {}\nGot:      {}",
                expected_form, expanded
            ))
        }
    }

    /// Expand an input string and return the result as a string
    pub fn expand_to_string(&self, input: &str) -> Result<String, String> {
        use crate::parser::Parser;

        let mut parser = Parser::new(input).map_err(|e| format!("Parse error: {}", e))?;
        let input_form = parser.parse().map_err(|e| format!("Parse error: {}", e))?;

        let expanded = patina_macros::expand_macro(&self.compiled, &input_form)
            .map_err(|e| format!("Expansion error: {}", e))?;

        Ok(format!("{}", expanded))
    }

    /// Check if two forms are equal, ignoring gensym differences
    ///
    /// This allows comparing macro expansions that contain hygienic identifiers
    /// like ##foo#123 and ##foo#456 as equivalent, and also treats ##foo#123
    /// as equivalent to foo (for testing purposes).
    fn forms_equal_ignoring_gensym(a: &Value, b: &Value) -> bool {
        use Value::*;

        // Helper to extract base name from any identifier form
        fn base_name(v: &Value) -> Option<&str> {
            match v {
                Symbol(s) => {
                    if s.starts_with("##") {
                        // Extract name between ## and last #
                        s.strip_prefix("##")
                            .and_then(|ss| ss.rfind('#').map(|i| &ss[..i]))
                            .or(Some(s.as_ref()))
                    } else {
                        Some(s.as_ref())
                    }
                }
                Identifier(id) => Some(id.name.as_ref()),
                _ => None,
            }
        }

        // Try to compare as identifiers first
        if let (Some(name1), Some(name2)) = (base_name(a), base_name(b)) {
            return name1 == name2;
        }

        match (a, b) {
            (Symbol(s1), Symbol(s2)) => {
                // Extract base name from potentially hygienic symbols
                let name1 = if s1.starts_with("##") {
                    // Extract name between ## and last #
                    s1.strip_prefix("##")
                        .and_then(|s| s.rfind('#').map(|i| &s[..i]))
                        .unwrap_or(s1.as_ref())
                } else {
                    s1.as_ref()
                };

                let name2 = if s2.starts_with("##") {
                    s2.strip_prefix("##")
                        .and_then(|s| s.rfind('#').map(|i| &s[..i]))
                        .unwrap_or(s2.as_ref())
                } else {
                    s2.as_ref()
                };

                name1 == name2
            }
            (Pair(p1), Pair(p2)) => {
                let b1 = p1.borrow();
                let b2 = p2.borrow();
                Self::forms_equal_ignoring_gensym(&b1.0, &b2.0)
                    && Self::forms_equal_ignoring_gensym(&b1.1, &b2.1)
            }
            (Vector(v1), Vector(v2)) => {
                v1.borrow().len() == v2.borrow().len()
                    && v1
                        .borrow()
                        .iter()
                        .zip(v2.borrow().iter())
                        .all(|(a, b)| Self::forms_equal_ignoring_gensym(a, b))
            }
            (Null, Null) => true,
            (Boolean(b1), Boolean(b2)) => b1 == b2,
            (Integer(i1), Integer(i2)) => i1 == i2,
            (BigInteger(bi1), BigInteger(bi2)) => bi1 == bi2,
            (Rational(r1), Rational(r2)) => r1 == r2,
            (Real(f1), Real(f2)) => (f1 - f2).abs() < f64::EPSILON,
            (Complex(parts1), Complex(parts2)) => {
                let (ref r1, ref i1) = **parts1;
                let (ref r2, ref i2) = **parts2;
                Self::forms_equal_ignoring_gensym(r1, r2)
                    && Self::forms_equal_ignoring_gensym(i1, i2)
            }
            (String(s1), String(s2)) => s1.borrow().as_str() == s2.borrow().as_str(),
            (Character(c1), Character(c2)) => c1 == c2,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_macro_expansion() {
        let expander = TestExpander::from_definition(
            "my-when",
            r#"
            (syntax-rules ()
              ((my-when test body ...)
               (if test (begin body ...))))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to(
                "(my-when #t (display 1) (display 2))",
                "(if #t (begin (display 1) (display 2)))",
            )
            .expect("Expansion should match");
    }

    #[test]
    fn test_expand_to_string() {
        let expander = TestExpander::from_definition(
            "double",
            r#"
            (syntax-rules ()
              ((double x) (list x x)))
            "#,
        )
        .expect("Failed to create expander");

        let result = expander
            .expand_to_string("(double 42)")
            .expect("Expansion should succeed");

        // The result should contain "list", "42", and appear twice
        assert!(result.contains("list"));
        assert!(result.matches("42").count() == 2);
    }

    #[test]
    fn test_gensym_comparison() {
        let expander = TestExpander::from_definition(
            "test",
            r#"
            (syntax-rules ()
              ((test x) (let ((y x)) y)))
            "#,
        )
        .expect("Failed to create expander");

        // This will generate a gensym for 'y', but we can still test it
        let result = expander
            .expand_to_string("(test 42)")
            .expect("Expansion should succeed");

        // Should contain let and 42
        assert!(result.contains("let"));
        assert!(result.contains("42"));
    }
}
