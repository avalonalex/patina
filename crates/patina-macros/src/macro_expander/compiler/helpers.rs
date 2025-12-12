//! Helper methods for the pattern/template compiler
//!
//! This module contains utility functions used throughout pattern and template
//! compilation, including symbol extraction, literal checking, and list traversal.

use super::Compiler;
use crate::error::MacroError;
use patina_runtime::{PVRef, Value};
use std::rc::Rc;

impl Compiler {
    /// Add a pattern variable and assign it a PVREF
    ///
    /// # Arguments
    /// - `name`: Variable name
    /// - `level`: Ellipsis nesting level
    ///
    /// # Returns
    /// The assigned PVREF
    pub(super) fn add_pvar(&mut self, name: Rc<str>, level: usize) -> Result<PVRef, MacroError> {
        if self.pvars.contains_key(&name) {
            return Err(MacroError::InvalidSyntax(format!(
                "Duplicate pattern variable: {}",
                name
            )));
        }

        if level > 255 {
            return Err(MacroError::InvalidSyntax(
                "Ellipsis nesting too deep (max 255 levels)".to_string(),
            ));
        }

        if self.pvar_count >= 255 {
            return Err(MacroError::InvalidSyntax(
                "Too many pattern variables (max 255)".to_string(),
            ));
        }

        let pvref = PVRef::new(level as u8, self.pvar_count as u8);
        self.pvars.insert(name, pvref);
        self.pvar_count += 1;

        Ok(pvref)
    }

    /// Check if a symbol is a literal identifier
    pub(super) fn is_literal(&self, sym: &Rc<str>) -> bool {
        self.literals.contains(sym)
    }

    /// Check if a value is the ellipsis symbol
    ///
    /// Recognizes both plain Symbol and Identifier (with marks/scopes)
    /// to support nested macro definitions where ellipsis may be wrapped.
    ///
    /// R7RS 4.3.2: "Literals have priority over ellipsis."
    /// If the ellipsis symbol is also in the literals list, return false.
    pub(super) fn is_ellipsis(&self, form: &Value) -> bool {
        match &self.ellipsis {
            None => false, // Ellipsis disabled
            Some(elli) => {
                let is_elli = match form {
                    Value::Symbol(s) => s == elli,
                    Value::Identifier(id) => &id.name == elli,
                    _ => false,
                };
                // Literals have priority over ellipsis (SRFI-46, R7RS 4.3.2)
                if is_elli && self.literals.contains(elli) {
                    return false;
                }
                is_elli
            }
        }
    }

    /// Extract symbol name from any identifier type
    ///
    /// Returns Some(name) for Symbol or Identifier.
    /// Returns None for other value types.
    pub(super) fn extract_symbol_name(form: &Value) -> Option<&Rc<str>> {
        match form {
            Value::Symbol(s) => Some(s),
            Value::Identifier(id) => Some(&id.name),
            _ => None,
        }
    }

    /// Check if a value is a SUBSTITUTED identifier from outer macro expansion.
    ///
    /// After the flip-scope algorithm in macro expansion:
    /// - Substituted values (from pattern variables): end up with EMPTY scopes
    ///   (they had the macro_scope before flip, which got removed)
    /// - Introduced identifiers (from template): end up with NON-EMPTY scopes
    ///   (they had empty scopes before flip, macro_scope got added)
    ///
    /// For nested macro hygiene, we need to distinguish:
    /// 1. Substituted values (EMPTY scopes): Should be treated as literals in inner patterns
    ///    to prevent the substituted symbol from being reinterpreted as a pattern variable.
    ///    This is the key for proper nested macro definitions.
    /// 2. Introduced identifiers (NON-EMPTY scopes): Should become pattern variables
    ///    in inner patterns. They're new identifiers introduced by the outer template.
    ///
    /// This function returns true ONLY for substituted values (Identifier with empty scopes).
    pub(super) fn is_substituted_from_outer_macro(form: &Value) -> bool {
        match form {
            // Identifier with EMPTY scopes = substituted from outer pattern variable
            // Should be treated as literal in inner patterns
            Value::Identifier(id) => id.scopes.is_empty(),
            // Symbols are fresh - they should become pattern variables
            // Identifiers with non-empty scopes are introduced - should also become pattern variables
            _ => false,
        }
    }

    /// Collect items from a list Value
    ///
    /// Returns (items, tail) where tail is Some(value) for improper lists
    pub(super) fn collect_list_items(
        &self,
        expr: &Value,
    ) -> Result<(Vec<Value>, Option<Value>), MacroError> {
        let mut items = Vec::new();
        let mut current = expr.clone();

        loop {
            match current {
                Value::Null => return Ok((items, None)),
                Value::Pair(pair) => {
                    let borrowed = pair.borrow();
                    items.push(borrowed.0.clone());
                    current = borrowed.1.clone();
                }
                _ => {
                    // Improper list: (a b . c)
                    return Ok((items, Some(current)));
                }
            }
        }
    }

    /// Check if the item at the given index is followed by an ellipsis
    pub(super) fn is_followed_by_ellipsis(&self, items: &[Value], index: usize) -> bool {
        index + 1 < items.len() && self.is_ellipsis(&items[index + 1])
    }

    /// Check if a value contains any pattern variables
    /// Used to determine if a quoted expression needs template expansion
    pub(super) fn contains_pattern_vars(&self, value: &Value) -> bool {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        if let Some(s) = Self::extract_symbol_name(value) {
            // Skip identifiers with scopes - they came from outer expansion
            // and shouldn't be treated as pattern variables
            if Self::is_substituted_from_outer_macro(value) {
                return false;
            }
            return self.pvars.contains_key(s);
        }

        match value {
            Value::Pair(_) => {
                let items = match self.collect_list_items(value) {
                    Ok((items, _)) => items,
                    Err(_) => return false,
                };
                items.iter().any(|item| self.contains_pattern_vars(item))
            }
            Value::Vector(v) => v
                .borrow()
                .iter()
                .any(|item| self.contains_pattern_vars(item)),
            _ => false,
        }
    }
}
