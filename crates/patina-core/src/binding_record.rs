//! Binding records and the cells they name (#834, 4b.6; `PRD/GC_PRD.md`
//! §11.6, home (a′)).
//!
//! A **cell** is a binding's location: the pair `(value . owner)`. A
//! **record** is a namespace's hold on a location: the pair
//! `(cell . flags)`, the word in one of the namespace's slots or scoped
//! entries. A namespace's own binding is a record over a cell of its own; an
//! import is a record over the exporter's cell, so the two share one
//! location and what either assigns the other sees (R7RS §5.2, #406). A
//! definition over an import, or an import over a definition, re-points the
//! record and never moves it, so a slot names one record for the
//! namespace's life.
//!
//! Both are ordinary pairs in the pair arena, so the collector marks
//! record → cell → value with no trace code of its own, and every read and
//! write goes through [`Heap::car`], [`Heap::cdr`], [`Heap::store_cell`] or
//! [`Heap::set_record_cell`], each of which the stale-reference check (#621)
//! guards: a freed record or cell panics with "use-after-free" in a debug or
//! `gc-check` build. Only this crate makes or reads one; a program never
//! sees either as a value, since every read path of `Environment`
//! dereferences it.

use crate::Trace;
use crate::census::Site;
use crate::heap::Heap;
use crate::tagged_value::TaggedValue;

/// What a cell of the namespace numbered `env_id` holds in its second word:
/// the fixnum `env_id << 4`, the low bits left for 4b.9's cell flags. A free
/// cell holds the fixnum 0, which no namespace's tag is, since ids start
/// at 1.
#[inline(always)]
fn owner_tag(env_id: u64) -> TaggedValue {
    debug_assert!(
        env_id != 0 && env_id < 1 << 56,
        "an environment id fits a cell's owner tag"
    );
    TaggedValue::fixnum((env_id << 4) as i64)
}

/// The identity of the pair `word` names, less a check build's generation
/// stamp (#621): a pair made in a swept pair's slot has the swept pair's key
/// in every build.
#[inline(always)]
fn stamp_free(word: TaggedValue) -> u64 {
    TaggedValue::pair(word.heap_index()).raw()
}

/// A namespace's binding record (#834, 4b.6, home (a′)): the pair
/// `(cell . flags)`, where flags is the fixnum 0 until 4b.9 uses it.
/// A namespace slot's word, and a namespace scoped entry's word, is one of
/// these. Never leaves this crate, and a program never sees one as a value:
/// every read path of `Environment` dereferences it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Trace)]
pub(crate) struct Record(TaggedValue);

/// A location: the pair `(value . owner)`, where owner is the fixnum
/// `env_id << 4` for the namespace whose cell it is, or 0 for a free cell
/// (an export with no binding behind it, a declaration's location).
///
/// Opaque outside this crate: no public constructor and no public reader. A
/// crate gets one from this crate and can only hand it back. Not branded: a
/// holder outside the heap's roots keeps it only while the registry entry, a
/// record or the context that made it does. A program never sees one as a
/// value; every read path of `Environment` dereferences it.
///
/// **Why an import is the exporter's cell.** R7RS §5.2 has an import name a
/// *binding*, so an importer and the library share one location: what the
/// library assigns later, the importer sees. Installing the value the
/// variable held at import time instead was #406 — every importer read a
/// stale copy, silently, and no suite caught it because none assigns a
/// variable another library imported.
///
/// **Every import shares, a primitive registered from Rust included.** A
/// first version copied those, reasoning that nothing can assign them and
/// that one program's `(set! car …)` should stay out of every other library.
/// Both halves were wrong. A Scheme library can re-export a primitive and
/// assign it later, and its importers then held exactly the stale copy this
/// sharing exists to remove — before or after the import deciding which. And
/// for a procedure they do not inline, chibi 0.12 and Gauche 0.9.15 agree
/// that the location is one: a program's `(set! list-copy …)` is seen by a
/// library that imported `list-copy`, and a library's by the program
/// (measured 2026-09-19; both rows are in `stdlib/library-bindings.scm`).
/// They differ from each other only where each inlines at compile time —
/// `car` is an opcode in chibi, `length` is inlined by Gauche — and Patina's
/// VM deoptimizes a `CallPrimitive` site when its primitive is rebound
/// precisely so that inlining is never observable. R7RS calls assigning an
/// imported binding "an error", so all of this is latitude; one location is
/// the answer that does not depend on what happened first. It was free when
/// measured the same day, interleaved with `main`: copying primitives
/// against sharing them made no difference on either backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Trace)]
pub struct BindingCell(TaggedValue);

impl BindingCell {
    /// A cell of the namespace numbered `env_id`, holding `value`.
    pub(crate) fn new_own(heap: &mut Heap, value: TaggedValue, env_id: u64) -> Self {
        BindingCell(heap.alloc_pair(value, owner_tag(env_id)))
    }

    /// A free cell, which no namespace owns, holding `value`.
    pub(crate) fn free(heap: &mut Heap, value: TaggedValue) -> Self {
        BindingCell(heap.alloc_pair(value, TaggedValue::fixnum(0)))
    }

    /// What the cell holds now.
    #[inline(always)]
    pub(crate) fn value(self, heap: &Heap) -> TaggedValue {
        heap.car(self.0)
    }

    /// Store `value` into the cell, logged at `site` with the cell as holder
    /// when the caller logs one ([`Heap::store_cell`]).
    #[inline(always)]
    pub(crate) fn store(self, heap: &mut Heap, value: TaggedValue, site: Option<Site>) {
        heap.store_cell(self.0, value, site);
    }

    /// Whether this is a cell of the namespace numbered `env_id`'s own.
    #[inline]
    pub(crate) fn owned_by(self, heap: &Heap, env_id: u64) -> bool {
        heap.cdr(self.0) == owner_tag(env_id)
    }

    /// The cell's identity: its word less a check build's generation stamp.
    /// Compared, never dereferenced: a cell made in a swept cell's slot has
    /// the swept cell's key in every build, so a holder of keys must let a
    /// key go before its cell can be swept (the rulings of 2026-11-03).
    #[inline]
    pub(crate) fn key(self) -> u64 {
        stamp_free(self.0)
    }

    /// The location this cell is, as `Environment::binding_location`
    /// answers it: its word, stamp and all, as an integer identity.
    #[inline]
    pub(crate) fn location(self) -> crate::environment::BindingLocation {
        crate::environment::BindingLocation::Cell(self.0.raw())
    }

    /// The cell's word, stamp and all: for the tests that watch its slot
    /// in the pair arena.
    #[cfg(test)]
    pub(crate) fn word(self) -> TaggedValue {
        self.0
    }
}

impl Record {
    /// A record over `cell`, with no flags set.
    pub(crate) fn new(heap: &mut Heap, cell: BindingCell) -> Self {
        Record(heap.alloc_pair(cell.0, TaggedValue::fixnum(0)))
    }

    /// The record a namespace's slot or scoped entry holds as `word`.
    #[inline(always)]
    pub(crate) fn from_word(word: TaggedValue) -> Self {
        debug_assert!(word.is_pair(), "a namespace's word is its record");
        Record(word)
    }

    /// The word a namespace's slot or scoped entry holds for this record.
    #[inline(always)]
    pub(crate) fn word(self) -> TaggedValue {
        self.0
    }

    /// The cell this record names now.
    #[inline(always)]
    pub(crate) fn cell(self, heap: &Heap) -> BindingCell {
        BindingCell(heap.car(self.0))
    }

    /// What the cell this record names holds now.
    #[inline(always)]
    pub(crate) fn value(self, heap: &Heap) -> TaggedValue {
        self.cell(heap).value(heap)
    }

    /// Point this record at `cell` instead ([`Heap::set_record_cell`]): a
    /// definition over an import, or an import over a binding.
    #[inline]
    pub(crate) fn repoint(self, heap: &mut Heap, cell: BindingCell) {
        heap.set_record_cell(self.0, cell.0);
    }

    /// The record's identity: its word less a check build's generation
    /// stamp, as [`BindingCell::key`].
    #[inline]
    pub(crate) fn key(self) -> u64 {
        stamp_free(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::GC_CHECK;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::trace::Trace as _;
    use crate::heap::trace_sentinels::{collect_from, collect_only};

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A cell's key is its slot's, whoever holds the slot: a cell made in a
    /// swept cell's slot has the swept cell's key, in a check build too,
    /// where the two words differ by their generation stamps. So a table
    /// keyed by cells must drop a key before its cell can be swept, and a
    /// test of that fails in CI's debug lane as in release (the rulings of
    /// 2026-11-03).
    #[test]
    fn a_cells_key_survives_slot_reuse_and_carries_no_stamp() {
        let mut heap = Heap::new();
        let old = BindingCell::new_own(&mut heap, n(1), 7);
        let key = old.key();
        assert_eq!(key, TaggedValue::pair(old.0.heap_index()).raw());

        collect_from(&mut heap, &[]);
        assert!(
            heap.free_pair_slots().contains(&old.0.heap_index()),
            "nothing holds the cell, so the collection swept it"
        );
        while heap.free_pair_slots().last() != Some(&old.0.heap_index()) {
            heap.alloc_pair(TaggedValue::UNSPECIFIED, TaggedValue::UNSPECIFIED);
        }
        let fresh = BindingCell::new_own(&mut heap, n(2), 9);
        assert_eq!(
            fresh.0.heap_index(),
            old.0.heap_index(),
            "the slot is reused"
        );
        assert_eq!(fresh.key(), key, "and the key with it");
        if GC_CHECK {
            assert_ne!(
                fresh.0, old.0,
                "a check build stamps the reused slot's word"
            );
        }

        // A record's key is stamp-free the same way.
        let record = Record::new(&mut heap, fresh);
        assert_eq!(record.key(), TaggedValue::pair(record.0.heap_index()).raw());
    }

    #[test]
    fn a_cell_is_owned_by_the_namespace_that_made_it_alone() {
        let mut heap = Heap::new();
        let cell = BindingCell::new_own(&mut heap, n(10), 5);
        assert!(cell.owned_by(&heap, 5));
        assert!(!cell.owned_by(&heap, 6));
        assert!(!cell.owned_by(&heap, 5 << 1));
        assert_eq!(cell.value(&heap), n(10));
        assert_eq!(
            heap.cdr(cell.0),
            n(5 << 4),
            "the owner tag leaves four bits"
        );
    }

    #[test]
    fn a_free_cell_belongs_to_no_namespace() {
        let mut heap = Heap::new();
        let cell = BindingCell::free(&mut heap, n(3));
        assert_eq!(heap.cdr(cell.0), n(0));
        for env_id in [1, 2, 1 << 20, (1 << 56) - 1] {
            assert!(!cell.owned_by(&heap, env_id), "env {env_id}");
        }
        assert_eq!(cell.value(&heap), n(3));
        cell.store(&mut heap, n(4), None);
        assert_eq!(cell.value(&heap), n(4));
        assert_eq!(heap.cdr(cell.0), n(0), "a store leaves the owner word");
    }

    /// `store_cell` writes the value word alone, with a census site or none,
    /// and a record reads the store through its cell. Its census site is
    /// `Site`'s, so the new `EnvRecordRepoint` is appended: the sites before
    /// it keep the indices the summary's arrays are laid out by.
    #[test]
    fn a_store_lands_in_the_cell_under_its_census_site_or_none() {
        let mut heap = Heap::new();
        let cell = BindingCell::new_own(&mut heap, n(1), 3);
        let record = Record::new(&mut heap, cell);
        for (site, value) in [
            (Some(Site::EnvSetSlot), n(2)),
            (Some(Site::EnvDefine), n(3)),
            (Some(Site::EnvSetScoped), n(4)),
            (None, n(5)),
        ] {
            cell.store(&mut heap, value, site);
            assert_eq!(cell.value(&heap), value, "{site:?}");
            assert_eq!(record.value(&heap), value, "{site:?}");
            assert!(
                cell.owned_by(&heap, 3),
                "{site:?}: the owner word is untouched"
            );
        }

        assert_eq!(Site::ALL.len(), 18);
        for (index, site) in Site::ALL.iter().enumerate() {
            assert_eq!(*site as usize, index, "{site:?}");
        }
        assert_eq!(Site::TwContinuationDefine as usize, 16);
        assert_eq!(Site::ALL.last(), Some(&Site::EnvRecordRepoint));
        assert_eq!(Site::EnvRecordRepoint.name(), "env_record_repoint");
    }

    #[test]
    fn a_record_names_its_cell_until_it_is_repointed() {
        let mut heap = Heap::new();
        let own = BindingCell::new_own(&mut heap, n(1), 4);
        let foreign = BindingCell::new_own(&mut heap, n(2), 8);
        let record = Record::new(&mut heap, own);
        assert_eq!(heap.cdr(record.word()), n(0), "no flags are set");
        assert_eq!(Record::from_word(record.word()), record);
        assert_eq!(record.cell(&heap), own);
        assert_eq!(record.value(&heap), n(1));

        let word = record.word();
        record.repoint(&mut heap, foreign);
        assert_eq!(record.word(), word, "a re-point moves no record");
        assert_eq!(record.cell(&heap), foreign);
        assert_eq!(record.value(&heap), n(2));
        assert_eq!(own.value(&heap), n(1), "the old cell is left alone");
    }

    /// A record's derived trace pins its word, and the collector marks
    /// record → cell → value transitively, with no trace code of its own;
    /// a cell's trace keeps its value the same way.
    #[test]
    fn a_record_keeps_its_cell_and_the_cell_its_value() {
        let mut heap = Heap::new();
        let mut s = Sentinels::new(&mut heap);
        let value = s.pair(&mut heap, "a record's cell's value");
        let cell = BindingCell::new_own(&mut heap, value, 2);
        let record = Record::new(&mut heap, cell);
        collect_only(&mut heap, |visitor| record.trace(visitor));
        s.assert_survived(&heap);
        assert_eq!(record.value(&heap), value);

        let mut s = Sentinels::new(&mut heap);
        let value = s.vector(&mut heap, "a free cell's value");
        let free = BindingCell::free(&mut heap, value);
        collect_only(&mut heap, |visitor| free.trace(visitor));
        s.assert_survived(&heap);
        assert_eq!(free.value(&heap), value);
    }

    /// The stale-reference check (#621) guards every access: a store into a
    /// freed cell, or a re-point of a freed record, panics in a check build.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "use-after-free: pair slot")]
    fn a_store_into_a_freed_cell_is_refused_in_a_check_build() {
        let mut heap = Heap::new();
        let cell = BindingCell::new_own(&mut heap, n(1), 2);
        collect_from(&mut heap, &[]);
        cell.store(&mut heap, n(2), Some(Site::EnvSetSlot));
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "use-after-free: pair slot")]
    fn a_repoint_of_a_freed_record_is_refused_in_a_check_build() {
        let mut heap = Heap::new();
        let cell = BindingCell::free(&mut heap, n(1));
        let record = Record::new(&mut heap, cell);
        collect_from(&mut heap, &[cell.0]);
        record.repoint(&mut heap, cell);
    }
}
