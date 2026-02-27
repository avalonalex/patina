//! Hygiene support for template expansion
//!
//! This module implements Racket-style scope-based hygiene for macro expansion,
//! including identifier renaming and marking substituted values.
//! All methods return TaggedValue directly.

use super::Expander;
use crate::macro_expander::Identifier;
use patina_core::TaggedValue;
use std::rc::Rc;

impl Expander {
    /// Rename an identifier for hygiene using Racket-style scope sets
    /// Returns TaggedValue directly.
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
    pub(super) fn rename_identifier_tagged(&self, id: &Identifier) -> TaggedValue {
        let name = id.name();

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
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Introduced '{}' (will get macro scope {} on output flip)",
                    name, self.macro_scope
                );
            }
            patina_runtime::ScopeSet::new()
        };

        // Create native Identifier with scopes (Racket-style hygiene)
        let mut heap = self.heap().borrow_mut();
        heap.alloc_identifier(name.clone(), scopes)
    }

    /// Mark a substituted TaggedValue from a pattern variable with the macro scope.
    ///
    /// This is crucial for nested macro hygiene. When a macro generates another
    /// `define-syntax`, symbols substituted from pattern variables need to be
    /// distinguishable from fresh pattern variables in the inner macro.
    ///
    /// IMPORTANT: We do NOT recurse into `syntax-rules` or `define-syntax` forms.
    /// These forms define their own macro context and their identifiers should
    /// not be marked with the current macro scope. They will be compiled later
    /// when the define-syntax is processed, with their own hygiene context.
    pub(super) fn mark_substituted_tagged(&self, tv: TaggedValue) -> TaggedValue {
        // Fast path: immediate values don't need marking
        if tv.is_fixnum() || tv.is_char() || tv.is_special() {
            return tv;
        }

        let heap = self.heap();

        // Check if it's an identifier (native or boxed) - add macro_scope
        {
            let heap_ref = heap.borrow();
            if let Some((name, scopes)) = heap_ref.get_identifier_data_any(tv) {
                let new_scopes = scopes.with_scope(self.macro_scope);
                drop(heap_ref);
                return heap.borrow_mut().alloc_identifier(name, new_scopes);
            }
        }

        // Check if it's a symbol - convert to identifier with macro_scope
        {
            let heap_ref = heap.borrow();
            if let Some(name) = heap_ref.get_symbol_name(tv) {
                let name_rc: Rc<str> = name.into();
                let scopes = patina_runtime::ScopeSet::new().with_scope(self.macro_scope);
                drop(heap_ref);
                return heap.borrow_mut().alloc_identifier(name_rc, scopes);
            }
        }

        // Handle native pairs recursively
        if tv.is_pair() {
            let (car, cdr) = heap.borrow().get_pair(tv);

            // Check if this is a macro definition form or quote form (don't mark inside)
            if self.is_macro_definition_tagged(car) || self.is_quote_form_tagged(car) {
                return tv;
            }

            let new_car = self.mark_substituted_tagged(car);
            let new_cdr = self.mark_substituted_tagged(cdr);
            return heap.borrow_mut().alloc_pair(new_car, new_cdr);
        }

        // Handle boxed pairs
        let is_boxed_pair = tv.is_pair();
        if is_boxed_pair && let Some((car_tv, cdr_tv)) = heap.borrow().try_pair(tv) {
            // Check if this is a macro definition form or quote form
            if self.is_macro_definition_tagged(car_tv) || self.is_quote_form_tagged(car_tv) {
                return tv;
            }

            let new_car = self.mark_substituted_tagged(car_tv);
            let new_cdr = self.mark_substituted_tagged(cdr_tv);
            return heap.borrow_mut().alloc_pair(new_car, new_cdr);
        }

        // Other values (vectors, etc.) pass through unchanged
        tv
    }

    /// Check if a TaggedValue is a macro definition form
    fn is_macro_definition_tagged(&self, tv: TaggedValue) -> bool {
        let heap = self.heap().borrow();
        matches!(
            heap.get_symbol_or_identifier_name(tv),
            Some("syntax-rules" | "define-syntax" | "let-syntax" | "letrec-syntax")
        )
    }

    /// Check if a TaggedValue is a quote form
    fn is_quote_form_tagged(&self, tv: TaggedValue) -> bool {
        let heap = self.heap().borrow();
        heap.get_symbol_or_identifier_name(tv) == Some("quote")
    }
}
