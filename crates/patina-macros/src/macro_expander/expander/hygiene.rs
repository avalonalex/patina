//! Hygiene support for template expansion
//!
//! This module implements Racket-style scope-based hygiene for macro expansion,
//! including identifier renaming and marking substituted values.
//! All methods return TaggedValue directly.

use super::{ExpandError, Expander};
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
        let value = heap.alloc_identifier(name.clone(), scopes);
        if let Some(source) = &id.source {
            heap.record_source(value, source.clone());
        }
        value
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
    ///
    /// The walk is a loop over a stack of the lists it is inside, not a
    /// recursion: what a pattern variable holds can be the rest of a program
    /// nested thousands deep, and the recursion overflowed the native stack
    /// (#617). A value that nests deeper than the expansion has left of
    /// [`MAX_FORM_DEPTH`] ([`Self::at_depth`]) is refused, since the desugarer
    /// would refuse the expansion that holds it — and before it got there,
    /// each level of such a form would copy all the levels below.
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    pub(super) fn mark_substituted_tagged(
        &self,
        tv: TaggedValue,
    ) -> Result<TaggedValue, ExpandError> {
        let mut open = OpenNodes::default();
        // The lists being marked, innermost last.
        let mut lists: Vec<MarkedList> = Vec::new();
        let mut next = tv;
        loop {
            let mut marked = match self.mark_node(next) {
                Mark::Done(marked) => marked,
                Mark::Circular => return Ok(tv),
                Mark::List { elements, tail } => {
                    if open.depth() >= self.depth_limit {
                        return Err(ExpandError::NestedTooDeeply);
                    }
                    if !open.enter(next) {
                        return Ok(tv);
                    }
                    let list = next;
                    next = elements[0];
                    lists.push(MarkedList {
                        list,
                        elements,
                        done: 0,
                        tail,
                    });
                    continue;
                }
            };
            // Hand `marked` to the list it belongs to, and on up through every
            // list it completes.
            loop {
                let Some(list) = lists.last_mut() else {
                    return Ok(marked);
                };
                if list.done < list.elements.len() {
                    // An element, marked in place; then the next, or the tail.
                    list.elements[list.done] = marked;
                    list.done += 1;
                    next = list.elements.get(list.done).copied().unwrap_or(list.tail);
                    break;
                }
                // The tail: the list is done.
                let list = lists.pop().expect("the list just marked");
                open.leave();
                marked = self.rebuild_marked(&list, marked);
            }
        }
    }

    /// Mark one node, or say it is a list to walk.
    fn mark_node(&self, tv: TaggedValue) -> Mark {
        // Fast path: immediate values don't need marking
        if tv.is_fixnum() || tv.is_char() || tv.is_special() {
            return Mark::Done(tv);
        }

        let heap = self.heap();

        // Check if it's an identifier (native or boxed) - add macro_scope
        {
            let heap_ref = heap.borrow();
            if let Some((name, scopes)) = heap_ref.get_identifier_data_any(tv) {
                let new_scopes = scopes.with_scope(self.macro_scope);
                drop(heap_ref);
                let mut heap = heap.borrow_mut();
                let copy = heap.alloc_identifier(name, new_scopes);
                heap.inherit_source(tv, copy);
                return Mark::Done(copy);
            }
        }

        // Check if it's a symbol - convert to identifier with macro_scope
        {
            let heap_ref = heap.borrow();
            if let Some(name) = heap_ref.get_symbol_name(tv) {
                let name_rc: Rc<str> = name.into();
                let scopes = patina_runtime::ScopeSet::new().with_scope(self.macro_scope);
                drop(heap_ref);
                return Mark::Done(heap.borrow_mut().alloc_identifier(name_rc, scopes));
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
            let Some((elements, tail)) = self.spine_of(tv) else {
                return Mark::Circular;
            };
            let head = elements[0];
            if self.is_macro_definition_tagged(head) || self.is_quote_form_tagged(head) {
                return Mark::Done(tv);
            }
            return Mark::List { elements, tail };
        }

        // Other values (vectors, etc.) pass through unchanged
        Mark::Done(tv)
    }

    /// The marked copy of `list`, from its marked elements and `tail`.
    fn rebuild_marked(&self, list: &MarkedList, tail: TaggedValue) -> TaggedValue {
        let mut heap = self.heap().borrow_mut();
        let mut out = tail;
        for &element in list.elements.iter().rev() {
            out = heap.alloc_pair(element, out);
        }
        heap.inherit_source(list.list, out);
        out
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

/// What marking one node found ([`Expander::mark_substituted_tagged`]).
enum Mark {
    /// The node's marked value: a copy, or the node itself where nothing in
    /// it is marked.
    Done(TaggedValue),
    /// A list to walk: its elements and whatever ends its spine.
    List {
        elements: Vec<TaggedValue>,
        tail: TaggedValue,
    },
    /// A spine that comes back on itself: the whole value goes unmarked.
    Circular,
}

/// A list being marked: its elements, the first `done` of them replaced by
/// their marked copies, and the tail, marked after them.
struct MarkedList {
    list: TaggedValue,
    elements: Vec<TaggedValue>,
    done: usize,
    tail: TaggedValue,
}
