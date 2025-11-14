//! Clean macro expander interface
//!
//! This module provides a trait-based abstraction for macro expansion that:
//! - Decouples the expansion algorithm from the consumer
//! - Enables easy testing with test helpers
//! - Allows swapping implementations (V1 vs V2, experimental algorithms, etc.)
//! - Provides a unified API for both compiled and interpreted macros

use crate::error::FrontendError;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Result type for macro expansion operations
pub type ExpansionResult = Result<Value, FrontendError>;

/// Trait for macro expansion engines
///
/// Implementations can use different algorithms (pattern matching, procedural, etc.)
/// but must all implement this common interface.
pub trait MacroExpander {
    /// Expand a macro call
    ///
    /// # Arguments
    /// - `macro_form`: The complete macro form including the macro name
    /// - `env`: Environment for hygiene and nested macro resolution
    ///
    /// # Returns
    /// The expanded expression, or an error if expansion fails
    fn expand(&self, macro_form: &Value, env: &Rc<Environment>) -> ExpansionResult;

    /// Get the name of this macro (for error messages)
    fn name(&self) -> &str;
}

/// A macro expander created from compiled macro data (V2 PVREF system)
pub struct CompiledMacroExpander {
    compiled: super::CompiledMacro,
}

impl CompiledMacroExpander {
    /// Create a new expander from compiled macro data
    pub fn new(compiled: super::CompiledMacro) -> Self {
        Self { compiled }
    }
}

impl MacroExpander for CompiledMacroExpander {
    fn expand(&self, macro_form: &Value, env: &Rc<Environment>) -> ExpansionResult {
        super::expand_macro(&self.compiled, macro_form, env)
    }

    fn name(&self) -> &str {
        &self.compiled.name
    }
}

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// The implementation and tests are only compiled in test mode.
pub struct TestExpander {
    expander: Box<dyn MacroExpander>,
    test_env: Rc<Environment>,
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
        let Value::Symbol(kw) = &p1.0 else {
            return Err(FrontendError::InvalidSyntax(
                "Expected syntax-rules keyword".to_string(),
            ));
        };
        if kw.as_ref() != "syntax-rules" {
            return Err(FrontendError::InvalidSyntax(format!(
                "Expected syntax-rules, got {}",
                kw
            )));
        }

        // Extract (literals) and rules
        let Value::Pair(p2) = &p1.1 else {
            return Err(FrontendError::InvalidSyntax(
                "syntax-rules must have literals and rules".to_string(),
            ));
        };
        let literals_form = &p2.0;
        let rules_form = &p2.1;

        // Parse literals list
        let mut literals = Vec::new();
        let mut current = literals_form.clone();
        while let Value::Pair(p) = current {
            if let Value::Symbol(s) = &p.0 {
                literals.push(s.clone());
            } else {
                return Err(FrontendError::InvalidSyntax(
                    "Literals must be symbols".to_string(),
                ));
            }
            current = p.1.clone();
        }
        if !matches!(current, Value::Null) {
            return Err(FrontendError::InvalidSyntax(
                "Literals must be a proper list".to_string(),
            ));
        }

        // Parse rules as (pattern template) pairs
        let mut rules = Vec::new();
        let mut current = rules_form.clone();
        while let Value::Pair(rule_pair) = current {
            let Value::Pair(rule_p) = &rule_pair.0 else {
                return Err(FrontendError::InvalidSyntax(
                    "Each rule must be a list".to_string(),
                ));
            };
            let pattern = rule_p.0.clone();
            let Value::Pair(tmpl_p) = &rule_p.1 else {
                return Err(FrontendError::InvalidSyntax(
                    "Rule must have a template".to_string(),
                ));
            };
            let template = tmpl_p.0.clone();
            if !matches!(tmpl_p.1, Value::Null) {
                return Err(FrontendError::InvalidSyntax(
                    "Each rule must have exactly pattern and template".to_string(),
                ));
            }
            rules.push((pattern, template));
            current = rule_pair.1.clone();
        }
        if !matches!(current, Value::Null) {
            return Err(FrontendError::InvalidSyntax(
                "Rules must be a proper list".to_string(),
            ));
        }

        // Compile it
        let mut compiler = super::Compiler::new(literals, None);
        let compiled = compiler.compile_macro(name.into(), rules)?;

        // Create test environment
        let test_env = Rc::new(Environment::new());

        Ok(Self {
            expander: Box::new(CompiledMacroExpander::new(compiled)),
            test_env,
        })
    }

    /// Create a test expander from a compiled macro
    pub fn from_compiled(compiled: super::CompiledMacro) -> Self {
        let test_env = Rc::new(Environment::new());
        Self {
            expander: Box::new(CompiledMacroExpander::new(compiled)),
            test_env,
        }
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

        // Expand
        let expanded = self
            .expander
            .expand(&input_form, &self.test_env)
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

        let expanded = self
            .expander
            .expand(&input_form, &self.test_env)
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
                Self::forms_equal_ignoring_gensym(&p1.0, &p2.0)
                    && Self::forms_equal_ignoring_gensym(&p1.1, &p2.1)
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
            (Complex(r1, i1), Complex(r2, i2)) => {
                (r1 - r2).abs() < f64::EPSILON && (i1 - i2).abs() < f64::EPSILON
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
