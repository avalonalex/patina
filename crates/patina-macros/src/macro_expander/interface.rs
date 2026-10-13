//! Clean macro expander interface and test helpers
//!
//! This module provides:
//! - `TestExpander`: A test helper for creating and testing macros
//!
//! The TestExpander uses `patina_macros` for compilation and expansion,
//! calling `expand_macro_with_scope`, the production path.

use crate::error::MacroError;
use patina_core::{Cx, Owned, SharedHeap, Value};

/// Read one datum of `text` through `cx`.
#[cfg(test)]
fn read<'gc>(cx: &mut Cx<'gc>, text: &str) -> Result<Value<'gc>, patina_frontend::ParseError> {
    patina_frontend::parser::Parser::new(text).and_then(|mut parser| parser.parse(cx))
}

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// The implementation and tests are only compiled in test mode.
#[allow(dead_code)]
pub struct TestExpander {
    heap: SharedHeap,
    /// The macro object, rooted for as long as the expander lives.
    mac: Owned,
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
        use patina_runtime::Environment;
        use std::rc::Rc;

        // Create test environment for macro compilation
        let test_env = Rc::new(Environment::new());
        let heap = test_env.heap().clone();

        // The syntax-rules form is read on the macro's heap, in the context
        // that compiles it; the macro object comes back out as a handle.
        let instance = Rc::clone(heap.borrow().instance());
        let mut driver = patina_core::GcDriver::acquire(&heap).expect("the driver is free");
        let mac = driver.mutate(&instance, |cx| {
            let form = read(cx, syntax_rules)
                .map_err(|e| MacroError::InvalidSyntax(format!("Parse error: {}", e)))?;
            let parsed = super::syntax_rules_parser::parse_syntax_rules(form, cx)
                .map_err(|e| MacroError::InvalidSyntax(e.0))?;
            let mut compiler =
                super::Compiler::with_env(parsed.literals, parsed.custom_ellipsis, test_env);
            let (compiled, literals) = compiler.compile_macro(cx, name.into(), parsed.rules)?;
            let mac = cx.alloc_macro(Rc::new(compiled), literals);
            Ok::<_, MacroError>(cx.hold(mac))
        })?;
        drop(driver);

        Ok(Self { heap, mac })
    }

    /// Run `f` in a context over the expander's heap, with its macro object.
    #[cfg(test)]
    fn in_cx<R>(&self, f: impl for<'gc> FnOnce(&mut Cx<'gc>, Value<'gc>) -> R) -> R {
        let instance = std::rc::Rc::clone(self.heap.borrow().instance());
        let mut driver = patina_core::GcDriver::acquire(&self.heap).expect("the driver is free");
        driver.mutate(&instance, |cx| {
            let mac = cx.held(&self.mac);
            f(cx, mac)
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
        self.in_cx(|cx, mac| {
            // Read the input and the expected form on the macro's heap
            let input = read(cx, input).map_err(|e| format!("Failed to parse input: {}", e))?;
            let expected =
                read(cx, expected).map_err(|e| format!("Failed to parse expected: {}", e))?;
            let expanded = super::expand_macro_with_scope(mac, input, cx, None)
                .map_err(|e| format!("Expansion failed: {}", e))?
                .form;

            // Compare the forms (ignoring gensym/hygiene differences)
            if Self::tagged_forms_equal_ignoring_gensym(expanded, expected, cx) {
                Ok(())
            } else {
                Err(format!(
                    "Expansion mismatch:\nExpected: {}\nGot:      {}",
                    cx.format(expected),
                    cx.format(expanded)
                ))
            }
        })
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
        self.in_cx(|cx, mac| {
            // Read the input on the macro's heap
            let input = read(cx, input).map_err(|e| format!("Parse error: {}", e))?;
            let expanded = super::expand_macro_with_scope(mac, input, cx, None)
                .map_err(|e| format!("Expansion error: {}", e))?
                .form;
            Ok(cx.format(expanded))
        })
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
    pub fn tagged_forms_equal_ignoring_gensym<'gc>(
        a: Value<'gc>,
        b: Value<'gc>,
        cx: &Cx<'gc>,
    ) -> bool {
        // Fast path: identical values
        if a == b {
            return true;
        }

        // Try to compare as identifiers/symbols
        let a_name = get_base_name_tagged(a, cx);
        let b_name = get_base_name_tagged(b, cx);
        if let (Some(n1), Some(n2)) = (&a_name, &b_name) {
            return n1 == n2;
        }
        // If one is an identifier and the other isn't, they're not equal
        if a_name.is_some() || b_name.is_some() {
            return false;
        }

        // Handle pairs
        if a.is_pair() && b.is_pair() {
            let (a_car, a_cdr) = cx.get_pair(a);
            let (b_car, b_cdr) = cx.get_pair(b);
            return Self::tagged_forms_equal_ignoring_gensym(a_car, b_car, cx)
                && Self::tagged_forms_equal_ignoring_gensym(a_cdr, b_cdr, cx);
        }

        // Handle fixnums
        if let (Some(i1), Some(i2)) = (a.as_fixnum(), b.as_fixnum()) {
            return i1 == i2;
        }

        // Handle booleans
        if a == Value::TRUE && b == Value::TRUE {
            return true;
        }
        if a == Value::FALSE && b == Value::FALSE {
            return true;
        }

        // Handle null
        if a.is_null() && b.is_null() {
            return true;
        }

        // Handle chars
        if let (Some(c1), Some(c2)) = (a.as_char(), b.as_char()) {
            return c1 == c2;
        }

        // Handle vectors
        if a.is_vector() && b.is_vector() {
            let a_len = cx.vector_len(a);
            let b_len = cx.vector_len(b);
            if a_len != b_len {
                return false;
            }
            for i in 0..a_len {
                let a_elem = cx.vector_ref(a, i);
                let b_elem = cx.vector_ref(b, i);
                if !Self::tagged_forms_equal_ignoring_gensym(a_elem, b_elem, cx) {
                    return false;
                }
            }
            return true;
        }

        false
    }
}

/// Extract the base name from a symbol or identifier, stripping gensym suffixes
fn get_base_name_tagged<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> Option<String> {
    let name = cx.get_symbol_or_identifier_name(tv)?;
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
