//! Non-owning syntax provenance: where each occurrence of program syntax was
//! read or expanded. Entries are removed by sweep, before slots can be
//! reused. Documents contain no Scheme values and are not GC roots.
//!
//! Only containers and identifiers have an entry. The reader used to record
//! the location of each element of a list or vector too, immediate datums
//! included, but nothing outside tests read those spans (#643).

use super::Heap;
use crate::{SourceLocation, TaggedValue};
use std::rc::Rc;

impl Heap {
    pub fn source(&self, value: TaggedValue) -> Option<&SourceLocation> {
        self.syntax_sources.get(&value.raw_bits()).map(Rc::as_ref)
    }

    pub fn record_source(&mut self, value: TaggedValue, location: SourceLocation) {
        if value.is_pair() || value.is_vector() || value.is_string() || value.is_object() {
            // Interned symbols denote a name, never one occurrence.
            if self.get_symbol_name(value).is_none() {
                self.syntax_sources
                    .insert(value.raw_bits(), Rc::new(location));
            }
        }
    }

    pub fn inherit_source(&mut self, original: TaggedValue, copy: TaggedValue) {
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
            if let Some((name, scopes)) = self.get_identifier_data_any(value) {
                if let Some(copy) = transform(self, value, name, scopes) {
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
    use crate::{GcRoots, GcVisitor, ScopeSet, SourceMap};

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
        impl GcRoots for Keep {
            fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
                visitor.visit(self.0);
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
