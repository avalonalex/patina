//! Clean macro expander interface and test helpers
//!
//! This module provides:
//! - `TestExpander`: A test helper for creating and testing macros
//!
//! The TestExpander uses `patina_macros` for compilation and expansion,
//! and provides convenient methods for testing macro behavior.

use crate::error::FrontendError;
use patina_core::{NoGcScope, Owned, SharedHeap};
use patina_runtime::Environment;
use std::rc::Rc;

/// Test helper for creating macro expanders and testing expansions
///
/// This is available in all builds to support testing in downstream crates.
/// Uses `patina_macros` for the underlying macro compilation and expansion.
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
    pub fn from_definition(name: &str, syntax_rules: &str) -> Result<Self, FrontendError> {
        use crate::parser::Parser;

        // Create test environment for compilation (captures bindings for hygiene)
        let test_env = Rc::new(Environment::new());
        let heap = test_env.heap().clone();

        // Read the syntax-rules form on the macro's heap; the macro object
        // comes back out of the context as a handle.
        let mac = NoGcScope::with_cx(&heap, |cx| {
            let form = Parser::new(syntax_rules)?.parse(cx)?;
            let parsed = patina_macros::parse_syntax_rules(form, cx)
                .map_err(|e| FrontendError::InvalidSyntax(e.0))?;
            let mut compiler = patina_macros::Compiler::with_env(
                parsed.literals,
                parsed.custom_ellipsis,
                test_env,
            );
            let (compiled, literals) = compiler
                .compile_macro(cx, name.into(), parsed.rules)
                .map_err(|e| FrontendError::InvalidSyntax(e.to_string()))?;
            let mac = cx.alloc_macro(Rc::new(compiled), literals);
            Ok::<_, FrontendError>(cx.hold(mac))
        })?;

        Ok(Self { heap, mac })
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

        NoGcScope::with_cx(&self.heap, |cx| {
            let mac = cx.held(&self.mac);
            // Read the input and the expected form on the macro's heap
            let input = Parser::new(input)
                .and_then(|mut parser| parser.parse(cx))
                .map_err(|e| format!("Failed to parse input: {}", e))?;
            let expected = Parser::new(expected)
                .and_then(|mut parser| parser.parse(cx))
                .map_err(|e| format!("Failed to parse expected: {}", e))?;

            let expanded = patina_macros::expand_macro_with_scope(mac, input, cx, None)
                .map_err(|e| format!("Expansion failed: {}", e))?
                .form;

            // Compare the forms (ignoring gensym/hygiene differences)
            if patina_macros::TestExpander::tagged_forms_equal_ignoring_gensym(
                expanded, expected, cx,
            ) {
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

    /// Expand an input string and return the result as a string
    pub fn expand_to_string(&self, input: &str) -> Result<String, String> {
        use crate::parser::Parser;

        NoGcScope::with_cx(&self.heap, |cx| {
            let mac = cx.held(&self.mac);
            // Read the input on the macro's heap
            let input = Parser::new(input)
                .and_then(|mut parser| parser.parse(cx))
                .map_err(|e| format!("Parse error: {}", e))?;
            let expanded = patina_macros::expand_macro_with_scope(mac, input, cx, None)
                .map_err(|e| format!("Expansion error: {}", e))?
                .form;
            Ok(cx.format(expanded))
        })
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
