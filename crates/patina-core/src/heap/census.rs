//! The heap's side of the GC census (#651, `crate::census`): each
//! allocation's kind and sizes, and the survival scan a collection runs
//! between marking and sweeping, with its count of `syntax_sources`'s keys
//! (#787). Compiled only with the `gc-census` feature.

use super::account::{OBJECT_SLOT_BYTES, PAIR_SLOT_BYTES, STRING_SLOT_BYTES, VECTOR_SLOT_BYTES};
use super::gc::MarkBits;
use super::{Heap, HeapObjectData};
use crate::census::imp::{self, Arena, Scan, Variant, round8, string_bytes, vector_bytes};
use crate::tagged_value::TaggedValue;
use num_bigint::BigInt;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

/// An object's variant, its length (free variables, fields, values), and
/// its size in the headered layout of `PRD/GC_PRD.md` §6.
pub(super) fn object_info(data: &HeapObjectData) -> (Variant, usize, u64) {
    use HeapObjectData as H;
    fn digits(b: &BigInt) -> u64 {
        b.bits().div_ceil(64).max(1)
    }
    let (variant, len, bytes) = match data {
        H::BigInt(b) => (0, 0, 8 + 8 * digits(b)),
        H::Rational(r) => (1, 0, 8 + 8 * (digits(r.numer()) + digits(r.denom()))),
        H::Real(_) => (2, 0, 16),
        H::Complex { .. } => (3, 0, 24),
        H::Symbol(_) => (4, 0, 16),
        H::Bytevector(v) => (5, v.len(), 8 + round8(v.len() as u64)),
        H::Exception { irritants, .. } => {
            (6, irritants.len(), 8 + 8 * (2 + irritants.len() as u64))
        }
        H::Procedure(_) => (7, 0, 16),
        H::Port(_) => (8, 0, 16),
        // Its literals and its carried relinks' records (#837), a word
        // each: the headered layout models the record's word alone, where
        // the byte account charges a relink its whole `CarriedRelink`, key
        // and all, since that layout has no place for the key's two
        // spellings and says nothing yet of where they go. A program makes
        // a relink where a generated macro keeps an identifier its
        // generator's expansion relinked (#837's second PR); this module's
        // test holds the term, whatever the lane's programs reach.
        H::Macro {
            literals, relinks, ..
        } => {
            let n = literals.len() + relinks.len();
            (9, n, 16 + 8 * n as u64)
        }
        H::RecordType(_) => (10, 0, 16),
        H::Record { fields, .. } => {
            let n = fields.borrow().len();
            (11, n, 8 + 8 * n as u64)
        }
        H::Identifier { .. } => (12, 0, 24),
        H::Continuation { .. } => (13, 0, 16),
        H::Parameter { .. } => (14, 0, 24),
        H::Promise(_) => (15, 0, 24),
        // 16 was `Library`, a variant nothing allocated (#930): reserved,
        // so that the others keep the numbers the study gave them.
        H::Values(v) => (17, v.len(), 8 + 8 * v.len() as u64),
        H::EnvironmentSpecifier { .. } => (18, 0, 16),
        H::PromptTag(_) => (19, 0, 16),
        H::LabelPlaceholder(_) => (20, 0, 16),
        H::MutableCell(_) => (21, 0, 16),
        H::Ephemeron(_) => (22, 0, 24),
        H::VmClosure { free_vars, .. } => (23, free_vars.len(), 16 + 8 * free_vars.len() as u64),
        H::VmContinuationRef { .. } => (24, 0, 16),
        H::VmDelimitedContinuationRef { .. } => (25, 0, 16),
        H::CoreSyntax(_) => (26, 0, 16),
        H::Free => (27, 0, 0),
    };
    (Variant(variant), len, bytes)
}

impl Heap {
    /// Record a pair, vector or string just allocated as `value`.
    pub(super) fn census_alloc(&self, value: TaggedValue) {
        if !self.census.on {
            return;
        }
        let index = value.heap_index();
        if value.is_pair() {
            imp::alloc(
                Arena::Pair,
                index,
                Variant(0),
                0,
                PAIR_SLOT_BYTES as u64,
                16,
            );
        } else if value.is_vector() {
            let elements = &self.vectors[index as usize];
            let today = VECTOR_SLOT_BYTES + super::account::vector_payload(elements);
            let len = elements.len();
            imp::alloc(
                Arena::Vector,
                index,
                Variant(0),
                len,
                today as u64,
                vector_bytes(len),
            );
        } else if value.is_string() {
            let chars = &self.strings[index as usize];
            let today = STRING_SLOT_BYTES + super::account::string_payload(chars);
            let len = chars.len();
            imp::alloc(
                Arena::String,
                index,
                Variant(0),
                len,
                today as u64,
                string_bytes(len),
            );
        }
    }

    /// Record an object just allocated as `value`, from what
    /// [`object_info`] and the byte account said of it before it moved
    /// into its slot.
    pub(super) fn census_alloc_object(
        &self,
        value: TaggedValue,
        info: Option<(Variant, usize, u64)>,
        payload: usize,
    ) {
        if let Some((variant, len, headered)) = info {
            let today = (OBJECT_SLOT_BYTES + payload) as u64;
            imp::alloc(
                Arena::Object,
                value.heap_index(),
                variant,
                len,
                today,
                headered,
            );
        }
    }

    /// Classify every occupied slot as live or dead, young or old, before
    /// the sweep frees the dead, and record the collection.
    pub(super) fn census_collection(&self, marks: &MarkBits) {
        if !self.census.on {
            return;
        }
        let start = Instant::now();
        let mut scan = Scan::default();
        imp::scan_arena(
            &mut scan,
            Arena::Pair,
            self.pairs.len(),
            |i| marks.pairs.get(i),
            |_| (16, 0),
        );
        imp::scan_arena(
            &mut scan,
            Arena::Vector,
            self.vectors.len(),
            |i| marks.vectors.get(i),
            |i| (vector_bytes(self.vectors[i].len()), 1),
        );
        imp::scan_arena(
            &mut scan,
            Arena::String,
            self.strings.len(),
            |i| marks.strings.get(i),
            |i| (string_bytes(self.strings[i].len()), 2),
        );
        imp::scan_arena(
            &mut scan,
            Arena::Object,
            self.objects.len(),
            |i| marks.objects.get(i),
            |i| {
                let (variant, _, bytes) = object_info(&self.objects[i]);
                (bytes, imp::class_of(Arena::Object, variant))
            },
        );
        self.census_sources(&mut scan, marks);
        scan.scan_us = start.elapsed().as_micros() as u64;
        imp::collected(scan);
    }

    /// `syntax_sources`'s keys by kind, the distinct locations they share,
    /// and how many the prune that follows keeps: what GC stage 4c's
    /// per-document tables replace (#787, #791). Run before the sweep's
    /// prune, so every key is a value of an occupied slot.
    fn census_sources(&self, scan: &mut Scan, marks: &MarkBits) {
        let mut entries = HashSet::new();
        let mut identifier_entries = HashSet::new();
        for (&bits, location) in &self.syntax_sources {
            // The prune reads its keys the same way, by their tag and index.
            let value = TaggedValue::from_raw(bits);
            let kind = if value.is_pair() {
                0
            } else if value.is_vector() {
                1
            } else if value.is_string() {
                2
            } else if value.is_object()
                && matches!(
                    self.objects.get(value.heap_index() as usize),
                    Some(HeapObjectData::Identifier { .. })
                )
            {
                3
            } else {
                4
            };
            scan.sources[kind] += 1;
            let entry = Rc::as_ptr(location);
            entries.insert(entry);
            if kind == 3 {
                identifier_entries.insert(entry);
            }
            if marks.is_marked(value) == Some(true) {
                scan.sources_kept += 1;
            }
        }
        scan.source_entries = entries.len() as u64;
        scan.source_identifier_entries = identifier_entries.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding_record::{BindingCell, BindingLink, CarriedRelink, Record};
    use crate::compiled_macro::CompiledMacro;
    use crate::scope::{ScopeId, ScopeSet};
    use std::collections::{HashMap, HashSet};

    /// The census sizes a macro object by its header and a word for each
    /// literal and each carried relink's record (#837): a plain macro is 16
    /// bytes and no length, and one with a literal and a relink is 32 and
    /// 2. A program makes a relink only in a generated macro that keeps a
    /// relinked identifier, which the census's own lane, which runs
    /// programs, need not reach.
    #[test]
    fn a_macros_relinks_are_words_of_the_object() {
        let body = || {
            Rc::new(CompiledMacro {
                name: Rc::from("m"),
                rules: vec![],
                max_pvars: 0,
                definition_scopes: ScopeSet::new(),
                template_symbols: HashSet::new(),
                inherited_identifiers: HashMap::new(),
                definition_env: None,
                foreign_expansions: vec![],
            })
        };
        let mut heap = Heap::new();
        let cell = BindingCell::free(&mut heap, TaggedValue::fixnum(1));
        let record = Record::new(&mut heap, cell);
        let relink = CarriedRelink::new(
            Rc::from("secret.1"),
            ScopeId(1),
            Rc::from("secret"),
            BindingLink::new(record),
        );
        let sized = |literals: Vec<TaggedValue>, relinks: Vec<CarriedRelink>| {
            let (_, len, bytes) = object_info(&HeapObjectData::Macro {
                body: body(),
                literals: literals.into_boxed_slice(),
                scopes: crate::heap::scope_sets::ScopeSetId::EMPTY,
                relinks: relinks.into_boxed_slice(),
            });
            (len, bytes)
        };
        assert_eq!(sized(vec![], vec![]), (0, 16));
        assert_eq!(sized(vec![TaggedValue::fixnum(7)], vec![relink]), (2, 32));
    }
}
