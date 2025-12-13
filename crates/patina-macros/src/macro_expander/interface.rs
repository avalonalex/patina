//! Clean macro expander interface
//!
//! This module provides a trait-based abstraction for macro expansion that:
//! - Decouples the expansion algorithm from the consumer
//! - Enables easy testing with test helpers
//! - Allows swapping implementations (V1 vs V2, experimental algorithms, etc.)
//! - Provides a unified API for both compiled and interpreted macros

use crate::error::MacroError;
use patina_runtime::Value;

/// Result type for macro expansion operations
pub type ExpansionResult = Result<Value, MacroError>;

/// Trait for macro expansion engines
///
/// Implementations can use different algorithms (pattern matching, procedural, etc.)
/// but must all implement this common interface.
pub trait MacroExpander {
    /// Expand a macro call
    ///
    /// # Arguments
    /// - `macro_form`: The complete macro form including the macro name
    ///
    /// # Returns
    /// The expanded expression, or an error if expansion fails
    fn expand(&self, macro_form: &Value) -> ExpansionResult;

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
    fn expand(&self, macro_form: &Value) -> ExpansionResult {
        super::expand_macro(&self.compiled, macro_form)
    }

    fn name(&self) -> &str {
        &self.compiled.name
    }
}

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// The implementation and tests are only compiled in test mode.
#[allow(dead_code)]
pub struct TestExpander {
    expander: Box<dyn MacroExpander>,
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
    ///
    /// This method is only available in test builds (requires patina-frontend as dev-dependency).
    #[cfg(test)]
    pub fn from_definition(name: &str, syntax_rules: &str) -> Result<Self, MacroError> {
        use patina_frontend::parser::Parser;

        // Parse the syntax-rules form
        let mut parser = Parser::new(syntax_rules)
            .map_err(|e| MacroError::InvalidSyntax(format!("Parse error: {}", e)))?;
        let syntax_rules_form = parser
            .parse()
            .map_err(|e| MacroError::InvalidSyntax(format!("Parse error: {}", e)))?;

        Self::from_syntax_rules_value(name, &syntax_rules_form)
    }

    /// Create a test expander from a parsed syntax-rules Value
    ///
    /// This is the internal implementation shared by from_definition and can be
    /// used directly when you already have a parsed Value.
    #[cfg(test)]
    fn from_syntax_rules_value(name: &str, syntax_rules_form: &Value) -> Result<Self, MacroError> {
        use super::syntax_rules_parser::parse_syntax_rules;
        use patina_runtime::Environment;
        use std::rc::Rc;

        // Parse the syntax-rules form using shared parser
        let parsed =
            parse_syntax_rules(syntax_rules_form).map_err(|e| MacroError::InvalidSyntax(e.0))?;

        // Create test environment for macro compilation
        let test_env = Rc::new(Environment::new());

        // Compile the macro (with custom ellipsis if SRFI-46)
        let mut compiler =
            super::Compiler::with_env(parsed.literals, parsed.custom_ellipsis, test_env);
        let compiled = compiler.compile_macro(name.into(), parsed.rules)?;

        Ok(Self {
            expander: Box::new(CompiledMacroExpander::new(compiled)),
        })
    }

    /// Stub for non-test builds
    #[cfg(not(test))]
    #[allow(dead_code)]
    pub fn from_definition(_name: &str, _syntax_rules: &str) -> Result<Self, MacroError> {
        Err(MacroError::InvalidSyntax(
            "TestExpander::from_definition is only available in test builds".to_string(),
        ))
    }

    /// Create a test expander from a compiled macro
    pub fn from_compiled(compiled: super::CompiledMacro) -> Self {
        Self {
            expander: Box::new(CompiledMacroExpander::new(compiled)),
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
    #[cfg(test)]
    pub fn assert_expands_to(&self, input: &str, expected: &str) -> Result<(), String> {
        use patina_frontend::parser::Parser;

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
            .expand(&input_form)
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

    /// Stub for non-test builds
    #[cfg(not(test))]
    #[allow(dead_code)]
    pub fn assert_expands_to(&self, _input: &str, _expected: &str) -> Result<(), String> {
        Err("TestExpander::assert_expands_to is only available in test builds".to_string())
    }

    /// Expand an input string and return the result as a string
    #[cfg(test)]
    pub fn expand_to_string(&self, input: &str) -> Result<String, String> {
        use patina_frontend::parser::Parser;

        let mut parser = Parser::new(input).map_err(|e| format!("Parse error: {}", e))?;
        let input_form = parser.parse().map_err(|e| format!("Parse error: {}", e))?;

        let expanded = self
            .expander
            .expand(&input_form)
            .map_err(|e| format!("Expansion error: {}", e))?;

        Ok(format!("{}", expanded))
    }

    /// Stub for non-test builds
    #[cfg(not(test))]
    #[allow(dead_code)]
    pub fn expand_to_string(&self, _input: &str) -> Result<String, String> {
        Err("TestExpander::expand_to_string is only available in test builds".to_string())
    }

    /// Check if two forms are equal, ignoring gensym/hygiene differences
    ///
    /// This allows comparing macro expansions that contain hygienic identifiers:
    /// - Symbol vs Symbol: compare names (stripping gensym suffixes)
    /// - Identifier vs Identifier: compare names (ignoring scope sets)
    /// - Symbol vs Identifier: compare names (for testing where expected uses Symbols)
    #[allow(dead_code)]
    fn forms_equal_ignoring_gensym(a: &Value, b: &Value) -> bool {
        use Value::*;

        // Helper to extract the base name from a symbol or identifier
        fn get_base_name(v: &Value) -> Option<&str> {
            match v {
                Symbol(s) => {
                    // Strip gensym suffix (##name#123 -> name)
                    if s.starts_with("##") {
                        s.strip_prefix("##")
                            .and_then(|rest| rest.rfind('#').map(|i| &rest[..i]))
                    } else {
                        Some(s.as_ref())
                    }
                }
                Identifier(id) => Some(id.name.as_ref()),
                _ => None,
            }
        }

        match (a, b) {
            // Handle Symbol/Identifier comparisons (both directions)
            (Symbol(_), Symbol(_))
            | (Identifier(_), Identifier(_))
            | (Symbol(_), Identifier(_))
            | (Identifier(_), Symbol(_)) => match (get_base_name(a), get_base_name(b)) {
                (Some(n1), Some(n2)) => n1 == n2,
                _ => false,
            },
            (Pair(p1), Pair(p2)) => {
                let pair1 = p1.borrow();
                let pair2 = p2.borrow();
                Self::forms_equal_ignoring_gensym(&pair1.0, &pair2.0)
                    && Self::forms_equal_ignoring_gensym(&pair1.1, &pair2.1)
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

    // ========================================================================
    // Basic macro expansion tests
    // ========================================================================

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
    fn test_hygiene_in_let_binding() {
        let expander = TestExpander::from_definition(
            "test-macro",
            r#"
            (syntax-rules ()
              ((test-macro x) (let ((y x)) y)))
            "#,
        )
        .expect("Failed to create expander");

        // This will generate a hygienic identifier for 'y'
        let result = expander
            .expand_to_string("(test-macro 42)")
            .expect("Expansion should succeed");

        // Should contain let and 42
        assert!(result.contains("let"));
        assert!(result.contains("42"));
    }

    // ========================================================================
    // Ellipsis pattern tests
    // ========================================================================

    #[test]
    fn test_ellipsis_zero_elements() {
        let expander = TestExpander::from_definition(
            "my-list",
            r#"
            (syntax-rules ()
              ((my-list x ...) (list x ...)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(my-list)", "(list)")
            .expect("Empty ellipsis should work");
    }

    #[test]
    fn test_ellipsis_multiple_elements() {
        let expander = TestExpander::from_definition(
            "my-list",
            r#"
            (syntax-rules ()
              ((my-list x ...) (list x ...)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(my-list 1 2 3)", "(list 1 2 3)")
            .expect("Multiple element ellipsis should work");
    }

    #[test]
    fn test_ellipsis_with_fixed_elements() {
        let expander = TestExpander::from_definition(
            "my-cons",
            r#"
            (syntax-rules ()
              ((my-cons first rest ...) (cons first (list rest ...))))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(my-cons 1 2 3 4)", "(cons 1 (list 2 3 4))")
            .expect("Fixed + ellipsis should work");
    }

    // ========================================================================
    // Literal matching tests
    // ========================================================================

    #[test]
    fn test_literal_matching() {
        let expander = TestExpander::from_definition(
            "my-cond",
            r#"
            (syntax-rules (else)
              ((my-cond (else result)) result)
              ((my-cond (test result)) (if test result)))
            "#,
        )
        .expect("Failed to create expander");

        // Test else branch
        expander
            .assert_expands_to("(my-cond (else 42))", "42")
            .expect("else literal should match");

        // Test non-else branch
        expander
            .assert_expands_to("(my-cond (#t 42))", "(if #t 42)")
            .expect("non-else should expand to if");
    }

    // ========================================================================
    // Multiple rule tests
    // ========================================================================

    #[test]
    fn test_multiple_rules_first_match() {
        let expander = TestExpander::from_definition(
            "my-or",
            r#"
            (syntax-rules ()
              ((my-or) #f)
              ((my-or x) x)
              ((my-or x y ...) (if x x (my-or y ...))))
            "#,
        )
        .expect("Failed to create expander");

        // First rule: no arguments
        expander
            .assert_expands_to("(my-or)", "#f")
            .expect("Empty or should return #f");

        // Second rule: single argument
        expander
            .assert_expands_to("(my-or 42)", "42")
            .expect("Single arg or should return arg");
    }

    // ========================================================================
    // Nested pattern tests
    // ========================================================================

    #[test]
    fn test_nested_list_pattern() {
        let expander = TestExpander::from_definition(
            "swap-pair",
            r#"
            (syntax-rules ()
              ((swap-pair (a b)) (list b a)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(swap-pair (1 2))", "(list 2 1)")
            .expect("Nested pattern should work");
    }

    #[test]
    fn test_deeply_nested_pattern() {
        let expander = TestExpander::from_definition(
            "extract",
            r#"
            (syntax-rules ()
              ((extract ((a b) c)) (list a b c)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(extract ((1 2) 3))", "(list 1 2 3)")
            .expect("Deeply nested pattern should work");
    }

    // ========================================================================
    // Template tests
    // ========================================================================

    #[test]
    fn test_template_with_literal_data() {
        let expander = TestExpander::from_definition(
            "wrap",
            r#"
            (syntax-rules ()
              ((wrap x) (quote (wrapper x))))
            "#,
        )
        .expect("Failed to create expander");

        let result = expander
            .expand_to_string("(wrap 42)")
            .expect("Expansion should succeed");

        assert!(result.contains("quote"));
        assert!(result.contains("wrapper"));
        assert!(result.contains("42"));
    }

    // ========================================================================
    // Error handling tests
    // ========================================================================

    #[test]
    fn test_no_matching_rule() {
        let expander = TestExpander::from_definition(
            "needs-two",
            r#"
            (syntax-rules ()
              ((needs-two a b) (list a b)))
            "#,
        )
        .expect("Failed to create expander");

        let result = expander.expand_to_string("(needs-two 1)");
        assert!(result.is_err(), "Should fail with wrong arity");
    }

    #[test]
    fn test_invalid_syntax_rules() {
        let result = TestExpander::from_definition("bad", "(not-syntax-rules ())");
        assert!(result.is_err(), "Should reject non-syntax-rules");
    }

    // ========================================================================
    // SRFI-46 custom ellipsis tests
    // ========================================================================

    #[test]
    fn test_srfi46_custom_ellipsis() {
        // Using ::: as custom ellipsis
        let expander = TestExpander::from_definition(
            "my-list",
            r#"
            (syntax-rules ::: ()
              ((my-list x :::) (list x :::)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(my-list 1 2 3)", "(list 1 2 3)")
            .expect("Custom ellipsis should work");
    }

    #[test]
    fn test_srfi46_ellipsis_as_literal() {
        // Using ... as a literal (not ellipsis)
        let expander = TestExpander::from_definition(
            "literal-dots",
            r#"
            (syntax-rules ::: (...)
              ((literal-dots x ... y) (list x y)))
            "#,
        )
        .expect("Failed to create expander");

        expander
            .assert_expands_to("(literal-dots 1 ... 2)", "(list 1 2)")
            .expect("Ellipsis as literal should work");
    }
}
