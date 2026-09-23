//! Hygiene support for template expansion
//!
//! This module implements Racket-style scope-based hygiene for macro expansion,
//! including identifier renaming and marking substituted values.
//! All methods return TaggedValue directly.

use super::Expander;
use crate::macro_expander::Identifier;
use patina_core::walk::OpenNodes;
use patina_core::{SpineEnd, TaggedValue};
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
    ///
    /// A substituted value can be circular, because the reader accepts datum
    /// labels (#459). One that comes back to itself anywhere, through a
    /// spine or through an element, is returned whole and unmarked, rather
    /// than walked until the stack overflows. Whole, because marking copies:
    /// the copy of a node a cycle passes through is not the node the cycle
    /// comes back to, so any marked copy of `#0=(a #0#)` unrolls it, where
    /// chibi and Gauche hand back the datum itself — `eq?` to its own
    /// `cadr`. As code the desugarer refuses it; as data a macro quotes it,
    /// and quoted data carries no marks.
    pub(super) fn mark_substituted_tagged(&self, tv: TaggedValue) -> TaggedValue {
        let mut walk = MarkWalk::default();
        let marked = self.mark_substituted_in(tv, &mut walk);
        if walk.circular { tv } else { marked }
    }

    /// [`Self::mark_substituted_tagged`], inside the pairs `walk` holds open.
    fn mark_substituted_in(&self, tv: TaggedValue, walk: &mut MarkWalk) -> TaggedValue {
        if walk.circular {
            return tv;
        }

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

        // Pairs: walk the *form*, not each tail.
        //
        // This used to recurse on the cdr and re-read its head, so a
        // substituted value shaped like `(f quote y)` had its tail `(quote y)`
        // read as a quote form: `y`, and everything after it, never received
        // the macro scope. A tail is not a form — the same defect #68 fixed in
        // the desugarer's `rewrite_form` and the audit's C1 fixed in its dotted
        // case. Flatten the spine once and decide head-ness at element 0,
        // which is what `compile_template` and `rewrite_form` already do.
        if tv.is_pair() {
            let Some((elems, tail)) = self.spine_of(tv) else {
                walk.circular = true;
                return tv;
            };
            let head = elems[0];
            if self.is_macro_definition_tagged(head) || self.is_quote_form_tagged(head) {
                return tv;
            }
            if !walk.open.enter(tv) {
                walk.circular = true;
                return tv;
            }
            let marked: Vec<TaggedValue> = elems
                .into_iter()
                .map(|e| self.mark_substituted_in(e, walk))
                .collect();
            let mut out = self.mark_substituted_in(tail, walk);
            walk.open.leave();
            let mut heap = heap.borrow_mut();
            for e in marked.into_iter().rev() {
                out = heap.alloc_pair(e, out);
            }
            return out;
        }

        // Other values (vectors, etc.) pass through unchanged
        tv
    }

    /// Flatten a pair's spine into its elements and whatever ends it — `()`
    /// for a proper list, the final atom for a dotted one — or `None` for a
    /// circular one. Non-empty by construction: the caller has already
    /// established `tv` is a pair.
    fn spine_of(&self, tv: TaggedValue) -> Option<(Vec<TaggedValue>, TaggedValue)> {
        match self.heap().borrow().spine(tv) {
            (elems, SpineEnd::Null) => Some((elems, TaggedValue::NULL)),
            (elems, SpineEnd::Improper(tail)) => Some((elems, tail)),
            (_, SpineEnd::Circular) => None,
        }
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

/// One marking of a substituted value: the pairs it is inside, and whether it
/// has met a cycle (`Expander::mark_substituted_tagged`).
#[derive(Default)]
struct MarkWalk {
    open: OpenNodes,
    circular: bool,
}
