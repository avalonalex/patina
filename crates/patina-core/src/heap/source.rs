//! Non-owning syntax provenance: where each occurrence of program syntax was
//! read or expanded. Documents contain no Scheme values and are not GC
//! roots.
//!
//! An identifier holds the id of its location's entry in the heap's
//! document tables (`heap/documents.rs`, #791): a copy takes its
//! original's id, and the sweep frees the entries no marked identifier
//! carries. A list, a vector, a string or another non-symbol object has an
//! entry of `syntax_sources`, keyed by its raw bits, which the sweep prunes
//! before a slot can be reused, until #792 and #793. The reader used to
//! record the location of each element of a list or vector too, immediate
//! datums included, but nothing outside tests read those spans (#643).

use super::scope_sets::{ScopeSetId, ScopeSetMemo};
use super::{Heap, IdentifierData};
use crate::{ScopeId, ScopeSet, SourceLocation, TaggedValue};
use std::rc::Rc;

impl Heap {
    /// Where `value` was read or expanded: an identifier's location, built
    /// from its entry in the document tables (#791), or a container's, from
    /// `syntax_sources`. Owned, since an identifier's is built and cannot
    /// be lent.
    pub fn source(&self, value: TaggedValue) -> Option<SourceLocation> {
        if let Some(identifier) = self.identifier(value) {
            return self.documents.location(identifier.source);
        }
        self.syntax_sources
            .get(&value.raw_bits())
            .map(|location| (**location).clone())
    }

    /// Locate `value` at `location`. An identifier takes a new entry of the
    /// document tables with that span and chain, its form span kept, and
    /// its payload is written: so only an identifier made in the calling
    /// context is recorded so (`heap/documents.rs`; one another context
    /// made is copied, [`Heap::alloc_identifier_relocated`]).
    pub fn record_source(&mut self, value: TaggedValue, location: SourceLocation) {
        if let Some(identifier) = self.identifier(value) {
            let source = self.documents.relocated(identifier.source, &location);
            self.set_identifier_source(value, source);
            return;
        }
        if value.is_pair() || value.is_vector() || value.is_string() || value.is_object() {
            // Interned symbols denote a name, never one occurrence.
            if self.get_symbol_name(value).is_none() {
                self.syntax_sources
                    .insert(value.raw_bits(), Rc::new(location));
            }
        }
    }

    /// Locate `copy` where `original` is, if `original` has a location. An
    /// identifier's copy takes its original's source id, with no write to
    /// the document tables (#791); the copy was made in the calling
    /// context, as [`Heap::record_source`] requires.
    pub fn inherit_source(&mut self, original: TaggedValue, copy: TaggedValue) {
        if original == copy {
            return;
        }
        if let Some(copied) = self.identifier(copy) {
            if let Some(identifier) = self.identifier(original)
                && !identifier.source.is_none()
                && identifier.source != copied.source
            {
                self.set_identifier_source(copy, identifier.source);
            }
            return;
        }
        // Scope edits often copy the same syntax repeatedly while a library
        // load defers GC. Share its immutable provenance; record_source
        // replaces a copy's entry only when that occurrence is updated.
        if original != copy
            && (copy.is_pair() || copy.is_vector() || copy.is_string() || copy.is_object())
            && self.get_symbol_name(copy).is_none()
            && let Some(source) = self.syntax_sources.get(&original.raw_bits()).cloned()
        {
            self.syntax_sources.insert(copy.raw_bits(), source);
        }
    }
}

impl Heap {
    /// Rewrite syntax identifiers without changing unaffected data. The walk
    /// is iterative and copies a changed graph once, including cycles and
    /// shared tails. `None` means this identifier stays as it is.
    pub fn map_syntax_identifiers(
        &mut self,
        root: TaggedValue,
        transform: impl FnMut(
            &mut Heap,
            TaggedValue,
            std::rc::Rc<str>,
            crate::ScopeSet,
        ) -> Option<TaggedValue>,
    ) -> TaggedValue {
        self.map_syntax_identifiers_memo(root, &mut std::collections::HashMap::new(), transform)
    }

    /// Share one rewrite across multiple roots, for quoted datums that share
    /// structure within a form. The caller must use the same transformation
    /// and prevent collection for the lifetime of this temporary memo.
    pub fn map_syntax_identifiers_memo(
        &mut self,
        root: TaggedValue,
        replacements: &mut std::collections::HashMap<u64, TaggedValue>,
        mut transform: impl FnMut(
            &mut Heap,
            TaggedValue,
            std::rc::Rc<str>,
            crate::ScopeSet,
        ) -> Option<TaggedValue>,
    ) -> TaggedValue {
        self.walk_syntax_identifiers(root, replacements, |heap, value| {
            let (name, scopes) = heap.get_identifier_data_any(value)?;
            transform(heap, value, name, scopes)
        })
    }

    /// `Cx::rescope_identifiers`' walk (#788): each identifier `picks` picks,
    /// other than one the reader wrote, copied with the set `rescope` makes
    /// of its own, which is made and interned once for each distinct set the
    /// walk meets, by its id. `picks` reads each set where the table holds
    /// it, with no copy. The memo lives for this call, beneath the context
    /// that called it, where nothing collects.
    pub(crate) fn rescope_identifiers(
        &mut self,
        root: TaggedValue,
        picks: impl Fn(&str, &ScopeSet) -> bool,
        rescope: impl Fn(&ScopeSet) -> ScopeSet,
    ) -> TaggedValue {
        let mut made = ScopeSetMemo::default();
        self.walk_syntax_identifiers(
            root,
            &mut std::collections::HashMap::new(),
            |heap, value| {
                let Some(IdentifierData {
                    name,
                    scopes: from,
                    source,
                    written,
                    relinked,
                }) = heap.identifier(value)
                else {
                    unreachable!("the walk hands over identifiers only");
                };
                if written || !picks(heap.symbol_spelling(name), heap.scope_sets.get(from)) {
                    return None;
                }
                let to = heap.scope_sets.edited(&mut made, from, &rescope);
                Some(heap.alloc_identifier_copy(name, to, relinked, source))
            },
        )
    }

    /// A copy of the identifier `value` with `scope` added to its set, the
    /// set made through `made` (#788), or `None` if `value` is no
    /// identifier: `Cx::add_scope_copying`'s. The copy is no written name,
    /// whatever the original was, and holds the original's name (#790) and
    /// its location (#791).
    pub(crate) fn identifier_adding_scope(
        &mut self,
        made: &mut ScopeSetMemo,
        scope: ScopeId,
        value: TaggedValue,
    ) -> Option<TaggedValue> {
        let IdentifierData {
            name,
            scopes: from,
            source,
            relinked,
            ..
        } = self.identifier(value)?;
        let to = self
            .scope_sets
            .edited(made, from, |set| set.with_scope(scope));
        Some(self.alloc_identifier_copy(name, to, relinked, source))
    }

    /// An identifier for the interned symbol `symbol` whose set is `scope`
    /// alone: the empty set's with `scope` added, made through `made`
    /// (#788).
    pub(crate) fn identifier_in_scope(
        &mut self,
        made: &mut ScopeSetMemo,
        scope: ScopeId,
        symbol: TaggedValue,
    ) -> TaggedValue {
        let to = self
            .scope_sets
            .edited(made, ScopeSetId::EMPTY, |set| set.with_scope(scope));
        self.alloc_identifier_with_set(symbol, to)
    }

    /// The walk under [`Heap::map_syntax_identifiers_memo`] and
    /// [`Heap::rescope_identifiers`]: `copy_of` is asked of each identifier
    /// it meets, and answers its copy or `None` to leave it as it is.
    fn walk_syntax_identifiers(
        &mut self,
        root: TaggedValue,
        replacements: &mut std::collections::HashMap<u64, TaggedValue>,
        mut copy_of: impl FnMut(&mut Heap, TaggedValue) -> Option<TaggedValue>,
    ) -> TaggedValue {
        use std::collections::{HashMap, HashSet};
        if let Some(&copy) = replacements.get(&root.raw_bits()) {
            return copy;
        }
        let mut parents = HashMap::<u64, Vec<TaggedValue>>::new();
        let mut seen = HashSet::new();
        let mut pending = vec![root];
        let mut changed = Vec::new();
        while let Some(value) = pending.pop() {
            if !seen.insert(value.raw_bits()) {
                continue;
            }
            if let Some(&copy) = replacements.get(&value.raw_bits()) {
                if copy != value {
                    changed.push(value);
                }
                continue;
            }
            if self.is_identifier(value) {
                if let Some(copy) = copy_of(self, value) {
                    if self.get_symbol_name(copy).is_none() {
                        self.inherit_source(value, copy);
                    }
                    replacements.insert(value.raw_bits(), copy);
                    changed.push(value);
                }
                continue;
            }
            let children = if value.is_pair() {
                let (car, cdr) = self.get_pair(value);
                vec![car, cdr]
            } else if value.is_vector() {
                self.vector_slice(value).to_vec()
            } else {
                continue;
            };
            for child in children {
                if child.is_pair() || child.is_vector() || self.is_identifier(child) {
                    parents.entry(child.raw_bits()).or_default().push(value);
                    pending.push(child);
                }
            }
        }
        let mut containers = Vec::new();
        while let Some(child) = changed.pop() {
            if let Some(ancestors) = parents.get(&child.raw_bits()) {
                for &parent in ancestors {
                    if replacements.contains_key(&parent.raw_bits()) {
                        continue;
                    }
                    let copy = if parent.is_pair() {
                        self.alloc_pair(TaggedValue::UNSPECIFIED, TaggedValue::NULL)
                    } else {
                        let len = self.vector_len(parent);
                        self.alloc_vector(vec![TaggedValue::UNSPECIFIED; len])
                    };
                    self.inherit_source(parent, copy);
                    replacements.insert(parent.raw_bits(), copy);
                    containers.push((parent, copy));
                    changed.push(parent);
                }
            }
        }
        let mapped = |value: TaggedValue| {
            replacements
                .get(&value.raw_bits())
                .copied()
                .unwrap_or(value)
        };
        for (original, copy) in containers {
            if original.is_pair() {
                let (car, cdr) = self.get_pair(original);
                self.set_car(copy, mapped(car));
                self.set_cdr(copy, mapped(cdr));
            } else {
                for i in 0..self.vector_len(original) {
                    self.vector_set(copy, i, mapped(self.vector_ref(original, i)));
                }
            }
        }
        mapped(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::gc::{Collector, MarkSweepCollector};
    use crate::{RootProvider, ScopeSet, SlotVisitor, SourceMap};

    #[test]
    fn syntax_copies_keep_independent_locations() {
        let mut heap = Heap::new();
        let mut map = SourceMap::new();
        map.set_source_text("(1) (2)".into());
        let original = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let copy = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let first = map.location("test.scm", 1, 1, 1, 4);
        let second = map.location("test.scm", 1, 5, 1, 8);
        heap.record_source(original, first.clone());
        heap.inherit_source(original, copy);
        assert_eq!(heap.source(copy), Some(&first));

        heap.record_source(copy, second.clone());
        assert_eq!(heap.source(original), Some(&first));
        assert_eq!(heap.source(copy), Some(&second));
        heap.record_source(original, map.location("test.scm", 1, 2, 1, 3));
        assert_eq!(heap.source(copy), Some(&second));
    }

    #[test]
    fn sweep_removes_non_owning_provenance_before_slot_reuse() {
        struct Keep(TaggedValue);
        impl RootProvider for Keep {
            fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
                visitor.pinned(self.0);
            }
        }
        let mut heap = Heap::new();
        let mut map = SourceMap::new();
        map.set_source_text("live dead".into());
        let live = heap.alloc_identifier("live".into(), ScopeSet::new());
        let dead = heap.alloc_identifier("dead".into(), ScopeSet::new());
        heap.record_source(live, map.location("test.scm", 1, 1, 1, 5));
        heap.record_source(dead, map.location("test.scm", 1, 6, 1, 10));
        let retained = heap.source(dead).unwrap().clone();
        // The new identifier's spelling is interned first, so that its
        // symbol, which the intern table keeps, takes no freed slot (#790).
        heap.intern_symbol("new");
        MarkSweepCollector::new().collect(&mut heap, &[&Keep(live)]);
        assert!(heap.source(live).is_some());
        assert!(heap.source(dead).is_none());
        let reused = heap.alloc_identifier("new".into(), ScopeSet::new());
        // The same slot. In a check build the new tenant's reference carries
        // a newer generation stamp, so it is deliberately not `dead` (#621).
        assert_eq!(reused.heap_index(), dead.heap_index());
        assert!(heap.source(reused).is_none());
        assert!(
            SourceMap::new()
                .format_context(&retained)
                .unwrap()
                .contains("live dead")
        );
    }
}
