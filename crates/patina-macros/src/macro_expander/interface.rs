//! Clean macro expander interface and test helpers
//!
//! This module provides:
//! - `TestExpander`: A test helper for creating and testing macros
//!
//! The TestExpander uses `patina_macros` for compilation and expansion,
//! calling `expand_macro_with_shadowed_tagged` (the production TaggedValue path).

use crate::error::MacroError;
use patina_core::TaggedValue;

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// The implementation and tests are only compiled in test mode.
#[allow(dead_code)]
pub struct TestExpander {
    compiled: super::CompiledMacro,
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
        use patina_runtime::Environment;
        use std::rc::Rc;

        // Create test environment for macro compilation
        let test_env = Rc::new(Environment::new());
        let heap = test_env.heap().clone();

        // Parse the syntax-rules form directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(syntax_rules, heap.clone())
            .map_err(|e| MacroError::InvalidSyntax(format!("Parse error: {}", e)))?;
        let form_tv = parser
            .parse()
            .map_err(|e| MacroError::InvalidSyntax(format!("Parse error: {}", e)))?;

        // Parse the syntax-rules structure using shared parser
        let parsed = super::syntax_rules_parser::parse_syntax_rules(form_tv, &heap.borrow())
            .map_err(|e| MacroError::InvalidSyntax(e.0))?;

        // Rules are already Vec<(TaggedValue, TaggedValue)> — pass directly to compiler
        let mut compiler =
            super::Compiler::with_env(parsed.literals, parsed.custom_ellipsis, test_env, heap);
        let compiled = compiler.compile_macro(name.into(), parsed.rules)?;

        Ok(Self { compiled })
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
    #[cfg(test)]
    pub fn assert_expands_to(&self, input: &str, expected: &str) -> Result<(), String> {
        use patina_frontend::parser::Parser;

        let heap = &self.compiled.heap;

        // Parse input directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(input, heap.clone())
            .map_err(|e| format!("Failed to parse input: {}", e))?;
        let input_tv = parser
            .parse()
            .map_err(|e| format!("Failed to parse input: {}", e))?;

        // Parse expected directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(expected, heap.clone())
            .map_err(|e| format!("Failed to parse expected: {}", e))?;
        let expected_tv = parser
            .parse()
            .map_err(|e| format!("Failed to parse expected: {}", e))?;

        // Expand using production TaggedValue path
        let expanded_tv = super::expand_macro_with_shadowed_tagged(
            &self.compiled,
            input_tv,
            heap,
            &std::collections::HashSet::new(),
            None,
        )
        .map_err(|e| format!("Expansion failed: {}", e))?;

        // Compare TaggedValues directly (ignoring gensym/hygiene differences)
        if Self::tagged_forms_equal_ignoring_gensym(expanded_tv, expected_tv, &heap.borrow()) {
            Ok(())
        } else {
            let heap_ref = heap.borrow();
            Err(format!(
                "Expansion mismatch:\nExpected: {}\nGot:      {}",
                patina_core::format_tagged(expected_tv, &heap_ref),
                patina_core::format_tagged(expanded_tv, &heap_ref)
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

        let heap = &self.compiled.heap;

        // Parse input directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(input, heap.clone())
            .map_err(|e| format!("Parse error: {}", e))?;
        let input_tv = parser.parse().map_err(|e| format!("Parse error: {}", e))?;

        // Expand using production TaggedValue path
        let expanded_tv = super::expand_macro_with_shadowed_tagged(
            &self.compiled,
            input_tv,
            heap,
            &std::collections::HashSet::new(),
            None,
        )
        .map_err(|e| format!("Expansion error: {}", e))?;

        Ok(patina_core::format_tagged(expanded_tv, &heap.borrow()))
    }

    /// Stub for non-test builds
    #[cfg(not(test))]
    #[allow(dead_code)]
    pub fn expand_to_string(&self, _input: &str) -> Result<String, String> {
        Err("TestExpander::expand_to_string is only available in test builds".to_string())
    }

    /// Check if two TaggedValue forms are equal, ignoring gensym/hygiene differences
    ///
    /// This allows comparing macro expansions that contain hygienic identifiers:
    /// - Symbol vs Symbol: compare names (stripping gensym suffixes)
    /// - Identifier vs Identifier: compare names (ignoring scope sets)
    /// - Symbol vs Identifier: compare names (for testing where expected uses Symbols)
    #[allow(dead_code)]
    pub fn tagged_forms_equal_ignoring_gensym(
        a: TaggedValue,
        b: TaggedValue,
        heap: &patina_core::Heap,
    ) -> bool {
        // Fast path: identical tagged values
        if a == b {
            return true;
        }

        // Try to compare as identifiers/symbols
        let a_name = get_base_name_tagged(a, heap);
        let b_name = get_base_name_tagged(b, heap);
        if let (Some(n1), Some(n2)) = (&a_name, &b_name) {
            return n1 == n2;
        }
        // If one is an identifier and the other isn't, they're not equal
        if a_name.is_some() || b_name.is_some() {
            return false;
        }

        // Handle pairs
        if a.is_pair() && b.is_pair() {
            let (a_car, a_cdr) = heap.get_pair(a);
            let (b_car, b_cdr) = heap.get_pair(b);
            return Self::tagged_forms_equal_ignoring_gensym(a_car, b_car, heap)
                && Self::tagged_forms_equal_ignoring_gensym(a_cdr, b_cdr, heap);
        }

        // Handle fixnums
        if let (Some(i1), Some(i2)) = (a.as_fixnum(), b.as_fixnum()) {
            return i1 == i2;
        }

        // Handle booleans
        if a == TaggedValue::TRUE && b == TaggedValue::TRUE {
            return true;
        }
        if a == TaggedValue::FALSE && b == TaggedValue::FALSE {
            return true;
        }

        // Handle null
        if a == TaggedValue::NULL && b == TaggedValue::NULL {
            return true;
        }

        // Handle chars
        if let (Some(c1), Some(c2)) = (a.as_char(), b.as_char()) {
            return c1 == c2;
        }

        // Handle vectors
        if a.is_vector() && b.is_vector() {
            let a_len = heap.vector_len(a);
            let b_len = heap.vector_len(b);
            if a_len != b_len {
                return false;
            }
            for i in 0..a_len {
                let a_elem = heap.vector_ref(a, i);
                let b_elem = heap.vector_ref(b, i);
                if !Self::tagged_forms_equal_ignoring_gensym(a_elem, b_elem, heap) {
                    return false;
                }
            }
            return true;
        }

        false
    }
}

/// Extract the base name from a TaggedValue symbol or identifier, stripping gensym suffixes
fn get_base_name_tagged(tv: TaggedValue, heap: &patina_core::Heap) -> Option<String> {
    let name = heap.get_symbol_or_identifier_name(tv)?;
    if name.starts_with("##") {
        // Strip gensym suffix (##name#123 -> name)
        name.strip_prefix("##")
            .and_then(|rest| rest.rfind('#').map(|i| rest[..i].to_string()))
    } else {
        Some(name.to_string())
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
