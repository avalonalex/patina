//! The heap's side of the GC census (#651, `crate::census`): each
//! allocation's kind and sizes, and the survival scan a collection runs
//! between marking and sweeping. Compiled only with the `gc-census` feature.

use super::account::{OBJECT_SLOT_BYTES, PAIR_SLOT_BYTES, STRING_SLOT_BYTES, VECTOR_SLOT_BYTES};
use super::gc::MarkBits;
use super::{Heap, HeapObjectData};
use crate::census::imp::{self, Arena, Scan, Variant, round8, string_bytes, vector_bytes};
use crate::tagged_value::TaggedValue;
use num_bigint::BigInt;
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
        H::Macro(_) => (9, 0, 16),
        H::RecordType(_) => (10, 0, 16),
        H::Record { fields, .. } => {
            let n = fields.borrow().len();
            (11, n, 8 + 8 * n as u64)
        }
        H::Identifier { .. } => (12, 0, 24),
        H::Continuation(_) => (13, 0, 16),
        H::Parameter { .. } => (14, 0, 24),
        H::Promise(_) => (15, 0, 24),
        H::Library(_) => (16, 0, 16),
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
        scan.scan_us = start.elapsed().as_micros() as u64;
        imp::collected(scan);
    }
}
