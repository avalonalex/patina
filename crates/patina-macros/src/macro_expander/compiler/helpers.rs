//! Helper methods for the pattern/template compiler
//!
//! This module contains utility functions used throughout pattern and template
//! compilation, including symbol extraction, literal checking, and list traversal.

use super::Compiler;
use crate::error::MacroError;
use patina_core::TaggedValue;
use patina_runtime::{PVRef, ScopeSet};
use std::rc::Rc;

impl Compiler {
    /// Add a pattern variable and assign it a PVREF
    ///
    /// # Arguments
    /// - `name`: Variable name
    /// - `scopes`: Scopes the identifier carried, part of its identity
    /// - `level`: Ellipsis nesting level
    ///
    /// # Returns
    /// The assigned PVREF
    pub(super) fn add_pvar(
        &mut self,
        name: Rc<str>,
        scopes: ScopeSet,
        level: usize,
    ) -> Result<PVRef, MacroError> {
        let key = (name.clone(), scopes);
        if self.pvars.contains_key(&key) {
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
        self.pvars.insert(key, pvref);
        self.pvar_count += 1;

        Ok(pvref)
    }

    /// The scope set an identifier carries, normalizing a bare symbol to empty.
    ///
    /// Identifier identity is (name, scopes) throughout the compiler: literal
    /// membership, pattern-variable binding, and pattern-variable reference all
    /// use it, so they agree on when two identifiers spelled alike are the same.
    pub(super) fn identifier_scopes(&self, form: TaggedValue) -> ScopeSet {
        let heap = self.heap.borrow();
        heap.get_identifier_data_any(form)
            .map(|(_, scopes)| scopes)
            .unwrap_or_default()
    }

    /// Whether a pattern identifier is one of this macro's literals.
    ///
    /// R7RS 4.3.2 decides this by identifier identity, so the form must agree
    /// with a literal in **both** name and scopes. Comparing names alone breaks
    /// a macro that writes another macro: the inner literals list and the inner
    /// pattern can spell the same name while one is introduced by the template
    /// and the other is substituted from the use site, which makes them
    /// different identifiers. See [`patina_runtime::LiteralSpec`].
    ///
    /// A bare symbol carries no scopes and normalizes to an empty scope set.
    pub(super) fn is_literal_form(&self, form: TaggedValue, name: &str) -> bool {
        if self.literal_specs.is_empty() {
            return false;
        }
        let scopes = self.identifier_scopes(form);
        self.literal_specs
            .iter()
            .any(|spec| spec.matches(name, &scopes))
    }

    /// Check if a TaggedValue is the ellipsis symbol
    ///
    /// Recognizes both plain Symbol and Identifier (with marks/scopes)
    /// to support nested macro definitions where ellipsis may be wrapped.
    ///
    /// R7RS 4.3.2: "Literals have priority over ellipsis."
    /// If the ellipsis symbol is also in the literals list, return false.
    pub(super) fn is_ellipsis(&self, form: TaggedValue) -> bool {
        match &self.ellipsis {
            None => false, // Ellipsis disabled
            Some(ellipsis_sym) => {
                let is_ellipsis_match = {
                    let heap = self.heap.borrow();
                    if let Some(s) = heap.get_symbol_name(form) {
                        s == ellipsis_sym.as_ref()
                    } else if let Some((name, _)) = heap.get_identifier_data_any(form) {
                        &name == ellipsis_sym
                    } else {
                        false
                    }
                };
                // Literals have priority over ellipsis (SRFI-46, R7RS 4.3.2).
                // The borrow above is released first: `is_literal_form` takes
                // its own.
                if is_ellipsis_match && self.is_literal_form(form, ellipsis_sym) {
                    return false;
                }
                is_ellipsis_match
            }
        }
    }

    /// Extract symbol name from any identifier type (TaggedValue)
    ///
    /// Returns Some(name) for Symbol or Identifier.
    /// Returns None for other value types.
    pub(super) fn extract_symbol_name(&self, form: TaggedValue) -> Option<Rc<str>> {
        let heap = self.heap.borrow();
        if let Some(s) = heap.get_symbol_name(form) {
            Some(Rc::from(s))
        } else if let Some((name, _)) = heap.get_identifier_data_any(form) {
            Some(name)
        } else {
            None
        }
    }

    /// Check if a TaggedValue is a SUBSTITUTED identifier from outer macro expansion.
    ///
    /// After the flip-scope algorithm in macro expansion:
    /// - Substituted values (from pattern variables): end up with EMPTY scopes
    ///   (they had the macro_scope before flip, which got removed)
    /// - Introduced identifiers (from template): end up with NON-EMPTY scopes
    ///   (they had empty scopes before flip, macro_scope got added)
    ///
    /// Only the template compiler uses this now, to decide how to emit an
    /// identifier that is *not* this rule's pattern variable: a substituted one
    /// already carries the identity the outer macro gave it and is emitted
    /// verbatim, while any other identifier gets this macro's definition scopes.
    ///
    /// Pattern compilation does not consult it. Classification there is by the
    /// literals list, and pattern-variable identity is (name, scopes) -- which
    /// distinguishes substituted from introduced identifiers without a
    /// provenance test.
    pub(super) fn is_substituted_from_outer_macro(&self, form: TaggedValue) -> bool {
        let heap = self.heap.borrow();
        // Check identifier (native or boxed, unified)
        if let Some((_, scopes)) = heap.get_identifier_data_any(form) {
            return scopes.is_empty();
        }
        // Symbols are fresh - they should become pattern variables
        false
    }

    /// Collect items from a list TaggedValue
    ///
    /// Returns (items, tail) where tail is Some(value) for improper lists
    pub(super) fn collect_list_items(
        &self,
        expr: TaggedValue,
    ) -> Result<(Vec<TaggedValue>, Option<TaggedValue>), MacroError> {
        let mut items = Vec::new();
        let mut current = expr;

        loop {
            if current == TaggedValue::NULL {
                return Ok((items, None));
            }

            // Fast path: native pair
            if current.is_pair() {
                let heap = self.heap.borrow();
                let (car, cdr) = heap.get_pair(current);
                items.push(car);
                current = cdr;
                continue;
            }

            // Check if it's any pair (including boxed)
            let is_pair = current.is_pair();
            if is_pair {
                let heap = self.heap.borrow();
                if let Some((car, cdr)) = heap.try_pair(current) {
                    items.push(car);
                    current = cdr;
                    continue;
                }
            }

            // Improper list: (a b . c)
            return Ok((items, Some(current)));
        }
    }

    /// Check if the item at the given index is followed by an ellipsis
    pub(super) fn is_followed_by_ellipsis(&self, items: &[TaggedValue], index: usize) -> bool {
        index + 1 < items.len() && self.is_ellipsis(items[index + 1])
    }

    /// Check if a TaggedValue contains any pattern variables
    /// Used to determine if a quoted expression needs template expansion
    pub(super) fn contains_pattern_vars(&self, value: TaggedValue) -> bool {
        // Handle all identifier types (Symbol, Identifier)
        if let Some(s) = self.extract_symbol_name(value) {
            // A substituted identifier is not skipped: the pattern compiler can
            // now bind one as a pattern variable, and a quoted template that
            // mentions it does need expansion. The lookup is by identity, so a
            // substituted identifier does not collide with an introduced
            // pattern variable of the same name.
            return self.pvars.contains_key(&(s, self.identifier_scopes(value)));
        }

        // Check pairs
        if value.is_pair() {
            let items = match self.collect_list_items(value) {
                Ok((items, _)) => items,
                Err(_) => return false,
            };
            return items.iter().any(|item| self.contains_pattern_vars(*item));
        }

        // Check vectors
        if value.is_vector() {
            let heap = self.heap.borrow();
            let len = heap.vector_len(value);
            for i in 0..len {
                if self.contains_pattern_vars(heap.vector_ref(value, i)) {
                    return true;
                }
            }
            return false;
        }

        false
    }
}
