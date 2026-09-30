//! Non-owning syntax provenance. Entries are removed by sweep, before slots
//! can be reused. Documents contain no Scheme values and are not GC roots.

use super::Heap;
use crate::{SourceLocation, TaggedValue};
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub(super) struct SyntaxSource {
    pub location: Option<SourceLocation>,
    pub children: Option<Rc<[Option<SourceLocation>]>>,
}

impl Heap {
    pub fn source(&self, value: TaggedValue) -> Option<&SourceLocation> {
        self.syntax_sources
            .get(&value.raw_bits())?
            .location
            .as_ref()
    }

    pub fn record_source(&mut self, value: TaggedValue, location: SourceLocation) {
        if value.is_pair() || value.is_vector() || value.is_string() || value.is_object() {
            // Interned symbols denote a name, never one occurrence.
            if self.get_symbol_name(value).is_none() {
                Rc::make_mut(self.syntax_sources.entry(value.raw_bits()).or_default()).location =
                    Some(location);
            }
        }
    }

    /// Pair car/cdr or vector element positions, including immediate datums.
    pub fn record_source_children(
        &mut self,
        value: TaggedValue,
        children: Vec<Option<SourceLocation>>,
    ) {
        Rc::make_mut(self.syntax_sources.entry(value.raw_bits()).or_default()).children =
            Some(children.into());
    }

    pub fn child_source(&self, value: TaggedValue, index: usize) -> Option<&SourceLocation> {
        self.syntax_sources
            .get(&value.raw_bits())?
            .children
            .as_ref()?
            .get(index)?
            .as_ref()
    }

    pub fn inherit_source(&mut self, original: TaggedValue, copy: TaggedValue) {
        // Scope edits often copy the same syntax repeatedly while a library
        // load defers GC. Share its immutable provenance; record_source and
        // record_source_children detach only when an occurrence is updated.
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
    use crate::{Collector, GcRoots, GcVisitor, MarkSweepCollector, ScopeSet, SourceMap};

    #[test]
    fn syntax_copies_keep_independent_locations_and_child_spans() {
        let mut heap = Heap::new();
        let mut map = SourceMap::new();
        map.set_source_text("(1) (2)".into());
        let original = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let copy = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let first = map.location("test.scm", 1, 1, 1, 4);
        let second = map.location("test.scm", 1, 5, 1, 8);
        heap.record_source(original, first.clone());
        heap.record_source_children(original, vec![Some(first.clone()), None]);
        heap.inherit_source(original, copy);

        heap.record_source(copy, second.clone());
        assert_eq!(heap.source(original), Some(&first));
        assert_eq!(heap.source(copy), Some(&second));
        assert_eq!(heap.child_source(copy, 0), Some(&first));

        heap.record_source_children(copy, vec![Some(second.clone()), None]);
        assert_eq!(heap.child_source(original, 0), Some(&first));
        assert_eq!(heap.child_source(copy, 0), Some(&second));
        heap.record_source_children(original, vec![None, Some(first.clone())]);
        assert_eq!(heap.child_source(copy, 0), Some(&second));
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
        let pair = heap.alloc_pair(dead, TaggedValue::NULL);
        heap.record_source_children(pair, vec![heap.source(dead).cloned(), None]);
        let retained = heap.source(dead).unwrap().clone();
        MarkSweepCollector::new().collect(&mut heap, &[&Keep(live)]);
        assert!(heap.source(live).is_some());
        assert!(heap.source(dead).is_none());
        assert!(heap.child_source(pair, 0).is_none());
        let reused = heap.alloc_identifier("new".into(), ScopeSet::new());
        assert_eq!(reused, dead);
        assert!(heap.source(reused).is_none());
        assert!(
            SourceMap::new()
                .format_context(&retained)
                .unwrap()
                .contains("live dead")
        );
    }
}
