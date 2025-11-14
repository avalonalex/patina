//! Special form registry for runtime-extensible special forms
//!
//! This module provides a registry-based system for special forms that enables:
//! - Runtime registration of new special forms (for DSLs/extensions)
//! - Introspection and help system support
//! - Centralized special form management
//! - Modular implementation (each form in its own file)

use super::trait_def::SpecialForm;
use crate::eval::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Environment, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Registry for special form implementations
///
/// This registry manages all special form implementations and provides dispatch.
/// Special forms are registered by name and can be looked up for evaluation.
///
/// # Example
///
/// ```ignore
/// use patina_tree_walker::eval::special_forms::{SpecialFormRegistry, QuoteForm};
///
/// let mut registry = SpecialFormRegistry::new();
/// registry.register(Box::new(QuoteForm));
///
/// // Later, in the evaluator:
/// if registry.contains("quote") {
///     let result = registry.eval("quote", &args, evaluator, env, in_tail)?;
/// }
/// ```
pub struct SpecialFormRegistry {
    /// Map from special form name to implementation
    forms: HashMap<&'static str, Box<dyn SpecialForm>>,
}

impl SpecialFormRegistry {
    /// Create a new empty special form registry
    ///
    /// Call `register()` to add special forms to the registry.
    pub fn new() -> Self {
        SpecialFormRegistry {
            forms: HashMap::new(),
        }
    }

    /// Register a special form
    ///
    /// If a special form with the same name already exists, it will be replaced.
    /// This enables customization and testing (can replace forms with mocks).
    ///
    /// # Arguments
    ///
    /// * `form` - The special form implementation (boxed trait object)
    ///
    /// # Example
    ///
    /// ```ignore
    /// registry.register(Box::new(QuoteForm));
    /// registry.register(Box::new(IfForm));
    /// registry.register(Box::new(LambdaForm));
    /// ```
    pub fn register(&mut self, form: Box<dyn SpecialForm>) {
        self.forms.insert(form.name(), form);
    }

    /// Check if a special form is registered
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the special form (e.g., "quote", "if")
    ///
    /// # Returns
    ///
    /// `true` if the special form is registered, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if registry.contains("quote") {
    ///     // quote is available
    /// }
    /// ```
    pub fn contains(&self, name: &str) -> bool {
        self.forms.contains_key(name)
    }

    /// Get a special form by name
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the special form
    ///
    /// # Returns
    ///
    /// `Some(&dyn SpecialForm)` if found, `None` otherwise.
    pub fn get(&self, name: &str) -> Option<&dyn SpecialForm> {
        self.forms.get(name).map(|boxed| &**boxed)
    }

    /// Evaluate a special form
    ///
    /// This is the main dispatch method called by the evaluator when it encounters
    /// a special form in the code.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the special form
    /// * `args` - The arguments (unevaluated)
    /// * `evaluator` - The evaluator instance
    /// * `env` - The environment in which to evaluate
    /// * `in_tail_position` - Whether this call is in tail position
    ///
    /// # Returns
    ///
    /// The evaluation result, or an error if the special form is not found
    /// or evaluation fails.
    ///
    /// # Errors
    ///
    /// - `EvalError::InvalidSyntax` if the special form is not registered
    /// - Any `EvalError` returned by the special form's `eval()` method
    ///
    /// # Example
    ///
    /// ```ignore
    /// // In the evaluator's dispatch logic:
    /// if let Value::Symbol(sym) = &first_value {
    ///     if self.special_form_registry.contains(sym.as_ref()) {
    ///         return self.special_form_registry.eval(
    ///             sym.as_ref(),
    ///             &rest_value,
    ///             self,
    ///             env,
    ///             in_tail_position,
    ///         );
    ///     }
    /// }
    /// ```
    pub fn eval(
        &self,
        name: &str,
        args: &Value,
        evaluator: &Evaluator,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let form = self
            .get(name)
            .ok_or_else(|| EvalError::InvalidSyntax(format!("Unknown special form: {}", name)))?;

        form.eval(evaluator, args, env, in_tail_position)
    }

    /// List all registered special forms (sorted by name)
    ///
    /// This is useful for introspection and help systems.
    ///
    /// # Returns
    ///
    /// A vector of special form names, sorted alphabetically.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let forms = registry.list_forms();
    /// // => ["begin", "define", "if", "lambda", "quote", "set!", ...]
    /// ```
    pub fn list_forms(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.forms.keys().copied().collect();
        names.sort();
        names
    }

    /// Get the number of registered special forms
    ///
    /// # Example
    ///
    /// ```ignore
    /// println!("Registered {} special forms", registry.len());
    /// ```
    pub fn len(&self) -> usize {
        self.forms.len()
    }

    /// Check if the registry is empty
    ///
    /// # Returns
    ///
    /// `true` if no special forms are registered, `false` otherwise.
    pub fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }
}

impl Default for SpecialFormRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock special form for testing
    struct MockForm {
        name: &'static str,
    }

    impl SpecialForm for MockForm {
        fn name(&self) -> &'static str {
            self.name
        }

        fn eval(
            &self,
            _evaluator: &Evaluator,
            _args: &Value,
            _env: &Rc<Environment>,
            _in_tail_position: bool,
        ) -> Result<EvalResult, EvalError> {
            Ok(EvalResult::Value(Value::Integer(42)))
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = SpecialFormRegistry::new();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_register_and_contains() {
        let mut registry = SpecialFormRegistry::new();

        registry.register(Box::new(MockForm { name: "test" }));

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        assert!(registry.contains("test"));
        assert!(!registry.contains("other"));
    }

    #[test]
    fn test_list_forms() {
        let mut registry = SpecialFormRegistry::new();

        registry.register(Box::new(MockForm { name: "if" }));
        registry.register(Box::new(MockForm { name: "quote" }));
        registry.register(Box::new(MockForm { name: "lambda" }));

        let forms = registry.list_forms();
        assert_eq!(forms, vec!["if", "lambda", "quote"]); // Sorted
    }

    #[test]
    fn test_get_special_form() {
        let mut registry = SpecialFormRegistry::new();
        registry.register(Box::new(MockForm { name: "test" }));

        let form = registry.get("test");
        assert!(form.is_some());
        assert_eq!(form.unwrap().name(), "test");

        let not_found = registry.get("missing");
        assert!(not_found.is_none());
    }
}
