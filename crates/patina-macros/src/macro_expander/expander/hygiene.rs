//! Hygiene support for template expansion
//!
//! This module implements Racket-style scope-based hygiene for macro expansion,
//! including identifier renaming and marking substituted values.

use super::Expander;
use crate::macro_expander::template::Identifier;
use crate::macro_expander::utils::is_macro_definition_form;
use patina_runtime::Value;
use std::cell::RefCell;
use std::rc::Rc;

impl Expander {
    /// Rename an identifier for hygiene using Racket-style scope sets
    ///
    /// All identifiers become `Identifier` with appropriate scopes:
    /// - Free variables: use definition scopes (for binding resolution)
    /// - Introduced identifiers: empty scopes (macro_scope will be added by flip on output)
    /// - Special forms/keywords: also get empty scopes (macro_scope added by flip)
    ///
    /// The actual hygiene discrimination happens via flip-scope:
    /// 1. Before expansion: flip macro_scope on INPUT (adds to use-site identifiers)
    /// 2. Template symbols get their definition_scopes here
    /// 3. After expansion: flip macro_scope on OUTPUT
    ///    - Use-site (from pattern vars): macro_scope removed (was added, then flipped off)
    ///    - Introduced (from template): macro_scope added (wasn't there, then flipped on)
    ///
    /// NOTE: Special forms like `let`, `if` are NOT special-cased anymore.
    /// They are treated like introduced identifiers and will get the macro_scope.
    /// This allows the desugarer to distinguish macro-introduced keywords from
    /// user variables with the same name. For example, in:
    ///   (let ((let odd?)) (my-or (let 8)))
    /// The macro's `let` (from template) will have macro_scope, while the user's
    /// `(let 8)` (from input) will NOT have macro_scope, so they can be distinguished.
    pub(super) fn rename_identifier(&self, id: &Identifier) -> Value {
        let name = id.name();

        // Get scopes for the identifier
        // NOTE: Special forms are NOT special-cased here - they get scopes like any other identifier
        let scopes = if let Some(def_scopes) = id.definition_scopes() {
            // FREE VARIABLE - use definition-time scopes
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Free variable '{}' with scopes {}",
                    name, def_scopes
                );
            }
            def_scopes.clone()
        } else {
            // INTRODUCED IDENTIFIER (including keywords like `let`, `if`)
            // Empty scopes for now - the macro_scope will be added when we flip on the output
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Introduced '{}' (will get macro scope {} on output flip)",
                    name, self.macro_scope
                );
            }
            patina_runtime::ScopeSet::new()
        };

        // Return Identifier with scopes (Racket-style hygiene)
        Value::Identifier(Box::new(patina_runtime::IdentifierData {
            name: name.clone(),
            scopes,
        }))
    }

    /// Mark a substituted value from a pattern variable with the macro scope.
    ///
    /// This is crucial for nested macro hygiene. When a macro generates another
    /// `define-syntax`, symbols substituted from pattern variables need to be
    /// distinguishable from fresh pattern variables in the inner macro.
    ///
    /// IMPORTANT: We do NOT recurse into `syntax-rules` or `define-syntax` forms.
    /// These forms define their own macro context and their identifiers should
    /// not be marked with the current macro scope. They will be compiled later
    /// when the define-syntax is processed, with their own hygiene context.
    pub(super) fn mark_substituted_value(&self, value: Value) -> Value {
        // Check if this is a syntax-rules or define-syntax form - don't mark inside
        if self.is_macro_definition(&value) {
            return value;
        }

        // Check if this is a (quote ...) form - quoted data should not have scopes added
        if self.is_quote_form(&value) {
            return value;
        }

        match value {
            // Convert Symbol to Identifier with macro_scope
            // This marks it as "came from outer macro expansion"
            Value::Symbol(name) => {
                let mut scopes = patina_runtime::ScopeSet::new();
                scopes = scopes.with_scope(self.macro_scope);
                Value::Identifier(Box::new(patina_runtime::IdentifierData { name, scopes }))
            }

            // Identifiers already have scopes - add macro_scope to them
            Value::Identifier(id) => {
                let new_scopes = id.scopes.with_scope(self.macro_scope);
                Value::Identifier(Box::new(patina_runtime::IdentifierData {
                    name: id.name.clone(),
                    scopes: new_scopes,
                }))
            }

            // Recursively mark pairs (but is_macro_definition check above handles special forms)
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                let new_car = self.mark_substituted_value(borrowed.0.clone());
                let new_cdr = self.mark_substituted_value(borrowed.1.clone());
                Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
            }

            // Vectors are self-quoting data - don't mark their contents
            // Symbols inside vectors should remain as symbols, not identifiers
            Value::Vector(_) => value,

            // Other values pass through unchanged
            _ => value,
        }
    }

    /// Check if a value is a (quote <datum>) form
    /// Quoted data should remain as-is without scope marking
    pub(super) fn is_quote_form(&self, value: &Value) -> bool {
        if let Value::Pair(pair) = value {
            let borrowed = pair.borrow();
            if let Value::Symbol(s) = &borrowed.0 {
                return s.as_ref() == "quote";
            }
            if let Value::Identifier(id) = &borrowed.0 {
                return id.name.as_ref() == "quote";
            }
        }
        false
    }

    /// Check if a value is a form whose contents should not be marked.
    /// This includes:
    /// - Macro definitions (syntax-rules, define-syntax, etc.) - they have their own context
    /// - Quote forms - quoted data should remain as-is without scope marking
    pub(super) fn is_macro_definition(&self, value: &Value) -> bool {
        is_macro_definition_form(value)
    }

    /// Check if a name is bound to a macro in the expansion environment
    #[allow(dead_code)]
    pub(super) fn is_macro(&self, name: &Rc<str>) -> bool {
        matches!(self.expansion_env.get(name), Some(Value::Macro(_)))
    }
}
