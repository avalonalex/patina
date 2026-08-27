//! Clean macro expander interface and test helpers
//!
//! This module provides:
//! - `TestExpander`: A test helper for creating and testing macros
//!
//! The TestExpander uses `patina_macros` for compilation and expansion,
//! and provides convenient methods for testing macro behavior.

use crate::error::FrontendError;
use patina_runtime::Environment;
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

        // Create test environment for compilation (captures bindings for hygiene)
        let test_env = Rc::new(Environment::new());
        let heap = test_env.heap().clone();

        // Parse the syntax-rules form directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(syntax_rules, heap.clone())?;
        let form_tv = parser.parse()?;

        // Parse the syntax-rules structure using shared parser from patina_macros
        let parsed = patina_macros::parse_syntax_rules(form_tv, &heap.borrow())
            .map_err(|e| FrontendError::InvalidSyntax(e.0))?;

        // Rules are already Vec<(TaggedValue, TaggedValue)> — pass directly to compiler
        let mut compiler = patina_macros::Compiler::with_env(
            parsed.literals,
            parsed.custom_ellipsis,
            test_env,
            heap,
        );
        let compiled = compiler
            .compile_macro(name.into(), parsed.rules)
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
        let expanded_tv = patina_macros::expand_macro_with_scope(
            &self.compiled,
            input_tv,
            heap,
            &std::collections::HashSet::new(),
            None,
        )
        .map_err(|e| format!("Expansion failed: {}", e))?
        .form;

        // Compare TaggedValues directly (ignoring gensym/hygiene differences)
        if patina_macros::TestExpander::tagged_forms_equal_ignoring_gensym(
            expanded_tv,
            expected_tv,
            &heap.borrow(),
        ) {
            Ok(())
        } else {
            // Format TaggedValues directly for error message
            let expanded = patina_core::format_tagged(expanded_tv, &heap.borrow());
            let expected = patina_core::format_tagged(expected_tv, &heap.borrow());
            Err(format!(
                "Expansion mismatch:\nExpected: {}\nGot:      {}",
                expected, expanded
            ))
        }
    }

    /// Expand an input string and return the result as a string
    pub fn expand_to_string(&self, input: &str) -> Result<String, String> {
        use crate::parser::Parser;

        let heap = &self.compiled.heap;

        // Parse input directly to TaggedValue on the macro's heap
        let mut parser = Parser::new_with_heap(input, heap.clone())
            .map_err(|e| format!("Parse error: {}", e))?;
        let input_tv = parser.parse().map_err(|e| format!("Parse error: {}", e))?;

        // Expand using production TaggedValue path
        let expanded_tv = patina_macros::expand_macro_with_scope(
            &self.compiled,
            input_tv,
            heap,
            &std::collections::HashSet::new(),
            None,
        )
        .map_err(|e| format!("Expansion error: {}", e))?
        .form;

        // Format TaggedValue directly for display
        Ok(patina_core::format_tagged(expanded_tv, &heap.borrow()))
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
