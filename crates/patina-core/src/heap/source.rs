//! Non-owning syntax provenance: where each occurrence of program syntax was
//! read or expanded. Documents contain no Scheme values and are not GC
//! roots.
//!
//! An identifier holds the id of its location's entry in the heap's
//! document tables (`heap/documents.rs`, #791): a copy takes its
//! original's id, and the sweep frees the entries no marked identifier
//! carries. A list is located by its head (#792, DESIGN E.1): the form span
//! of the identifier it starts with, if that identifier has one. A list
//! whose head is no identifier, a vector, a string and every other object
//! has no location: lost, never misattributed.
//!
//! A list, a vector and a string used to have an entry of a map in `Heap`
//! keyed by raw value bits, which the sweep pruned before a slot could be
//! reused; nothing wrote it after #792, and #793 deleted it, its prune and
//! `TaggedValue`'s `from_raw` with it. The reader
//! used to record the location of each element of a list or vector too,
//! immediate datums included, but nothing outside tests read those spans
//! (#643).

use super::scope_sets::{ScopeSetId, ScopeSetMemo};
use super::{Heap, IdentifierData};
use crate::{ScopeId, ScopeSet, SourceLocation, TaggedValue};

impl Heap {
    /// Where `value` was read or expanded, built from the document tables
    /// (#791): an identifier's own location, or a list's, which is the form
    /// span of the identifier at its head (#792). `None` for a list whose
    /// head is no identifier or has no form span, and for every other value.
    /// Owned, since a location is built and cannot be lent.
    pub fn source(&self, value: TaggedValue) -> Option<SourceLocation> {
        if let Some(identifier) = self.identifier(value) {
            return self.documents.location(identifier.source);
        }
        if value.is_pair() {
            return self.form_span(self.car(value));
        }
        None
    }

    /// Locate the identifier `value` at `location`: it takes a new entry of
    /// the document tables with that span and chain, its form span kept,
    /// and its payload is written. So only an identifier made in the calling
    /// context is recorded so (`heap/documents.rs`; one another context made
    /// is copied, [`Heap::alloc_identifier_relocated`]). Any other value is
    /// left as it is: a list is located by its head, and nothing else is
    /// located (#792).
    pub fn record_source(&mut self, value: TaggedValue, location: SourceLocation) {
        if let Some(identifier) = self.identifier(value) {
            let source = self.documents.relocated(identifier.source, &location);
            self.set_identifier_source(value, source);
        }
    }

    /// Formerly located `copy` where `original` was: a container through
    /// the raw-bits provenance table, which #793 deleted, and an identifier
    /// through its source id. Does nothing.
    #[deprecated(
        note = "a no-op since #793: an identifier holds its location as a source id, which every \
                copy the heap makes of it takes (Cx::rescope_identifiers, \
                Heap::map_syntax_identifiers, Heap::alloc_identifier_relocated, \
                Cx::alloc_relinked_copy), and a list is located by its head identifier"
    )]
    pub fn inherit_source(&mut self, _original: TaggedValue, _copy: TaggedValue) {}

    /// Locate the identifier `copy`, made in the calling context, where the
    /// identifier `original` is: it takes its original's source id, with no
    /// write to the document tables (#791). A no-op for a pair of values
    /// that are not both identifiers. The syntax walks' (#793), for a copy a
    /// caller's transform made.
    pub(crate) fn locate_copy(&mut self, original: TaggedValue, copy: TaggedValue) {
        if original == copy {
            return;
        }
        if let (Some(identifier), Some(copied)) = (self.identifier(original), self.identifier(copy))
            && !identifier.source.is_none()
            && identifier.source != copied.source
        {
            self.set_identifier_source(copy, identifier.source);
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
                    self.locate_copy(value, copy);
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
    use crate::SourceMap;

    /// Syntax is `Drop`-free (`PRD/GC_PRD.md` §19, "Value on its own";
    /// #793): an identifier's payload holds its name, its scope set and its
    /// location as words and ids, and a list's location is its head's, so
    /// no syntax object owns a Rust value that a sweep would drop.
    #[test]
    fn syntax_is_drop_free() {
        assert!(!std::mem::needs_drop::<IdentifierData>());
    }

    /// A list is located by its head (#792): the form span of the
    /// identifier it starts with, which a copy of the list with the same
    /// head shares. A list whose head is no identifier, a vector and a
    /// string have none, and recording one on them writes nothing.
    #[test]
    fn a_list_is_located_by_its_head() {
        let mut heap = Heap::new();
        let mut map = SourceMap::new();
        map.set_source_text("(car 5) (1 2) #(1) \"s\"".into());
        let car = heap.intern_symbol("car");
        let head = heap.alloc_source_identifier_for(car);
        let own = map.location("test.scm", 1, 2, 1, 5);
        let form = map.location("test.scm", 1, 1, 1, 8);
        heap.record_head_source(head, &own, &form);
        let tail = heap.alloc_pair(TaggedValue::fixnum(5), TaggedValue::NULL);
        let list = heap.alloc_pair(head, tail);
        let copy = heap.alloc_pair(head, tail);
        assert_eq!(heap.source(head), Some(own));
        assert_eq!(heap.source(list), Some(form.clone()));
        assert_eq!(heap.source(copy), Some(form.clone()));

        heap.record_source(list, map.location("test.scm", 1, 9, 1, 14));
        assert_eq!(
            heap.source(list),
            Some(form),
            "a list's location is its head's"
        );

        let numbers = heap.alloc_pair(TaggedValue::fixnum(1), tail);
        let vector = heap.alloc_vector(vec![TaggedValue::fixnum(1)]);
        let string = heap.alloc_string("s".into());
        for (value, at) in [(numbers, (9, 14)), (vector, (15, 19)), (string, (19, 22))] {
            heap.record_source(value, map.location("test.scm", 1, at.0, 1, at.1));
            assert_eq!(heap.source(value), None);
        }
    }

    /// The head of a list a template builds takes the use as its form span
    /// (decision 16 on #782, #792), its own span and chain kept: a copy for
    /// a head another context made, and the head itself in place for one
    /// made in this context. A use with no location leaves no form span,
    /// never the one the head had; a head with no location of its own is
    /// located at the use as well.
    #[test]
    fn a_template_built_head_takes_the_use_as_its_form_span() {
        let mut heap = Heap::new();
        let mut map = SourceMap::new();
        map.set_source_text("(define-syntax m (car x))\n(m 5)".into());
        let car = heap.intern_symbol("car");
        let template = heap.alloc_source_identifier_for(car);
        let own = map.location("test.scm", 1, 19, 1, 22);
        let written = map.location("test.scm", 1, 18, 1, 25);
        heap.record_head_source(template, &own, &written);
        let at_use = map.location("test.scm", 2, 1, 2, 6);

        let copy = heap
            .alloc_identifier_formed(template, Some(&at_use))
            .unwrap();
        assert_ne!(copy, template);
        assert_eq!(heap.source(copy), Some(own.clone()));
        assert_eq!(heap.form_span(copy), Some(at_use.clone()));
        assert_eq!(
            heap.form_span(template),
            Some(written),
            "the original is untouched"
        );
        assert_eq!(
            heap.identifier(copy).unwrap().written,
            heap.identifier(template).unwrap().written
        );

        let unlocated = heap.alloc_identifier_formed(template, None).unwrap();
        assert_eq!(heap.source(unlocated), Some(own.clone()));
        assert_eq!(heap.form_span(unlocated), None);

        heap.record_form(copy, None);
        assert_eq!(heap.form_span(copy), None);
        heap.record_form(copy, Some(&at_use));
        assert_eq!(heap.form_span(copy), Some(at_use.clone()));

        let bare = heap.alloc_identifier_for(car, ScopeSet::new());
        assert_eq!(heap.source(bare), None);
        let located = heap.alloc_identifier_formed(bare, Some(&at_use)).unwrap();
        assert_eq!(heap.source(located), Some(at_use.clone()));
        assert_eq!(heap.form_span(located), Some(at_use));
        assert_eq!(
            heap.alloc_identifier_formed(bare, None)
                .map(|v| heap.source(v)),
            Some(None)
        );
    }
}
