//! The heap's scope-set table (#788, GC stage 4c.2): every scope set an
//! identifier carries, held once, under a [`ScopeSetId`].
//!
//! An identifier used to own its set, a sorted `SmallVec` of scope ids that
//! spilled to the allocator past three, and each scope edit built a set for
//! each identifier it changed. Now an identifier holds the id of its set's
//! entry here. Equal sets share an entry, so a set spilled past three scopes
//! is allocated once however many identifiers carry it, and a walk that
//! gives many identifiers one new set interns it once.
//!
//! Id 0 is the empty set, made with the table and never given up: the
//! reader's identifiers carry it, and take it with no lookup.
//!
//! **Liveness.** The table holds no value and is no root. Marking an
//! identifier sets its id's bit in the collection's [`MarkBits`], a bitmap
//! sized to the table when the collection starts, with id 0's bit set
//! before anything is marked. The sweep, beside the `syntax_sources` prune,
//! frees every entry whose bit is clear ([`ScopeSetTable::sweep`]): its set
//! leaves the index and its spill is dropped, and its id is reused, the
//! lowest first. So right after a collection the table holds exactly the
//! sets the marked identifiers carry, and between collections those and
//! the ones interned since (decision 19 on #782). A `ScopeId` is never
//! reused; only a set's id is.
//!
//! A free entry holds the empty set, which no other entry but id 0 holds,
//! so in debug and `gc-check` builds a read of a free entry panics, as
//! #621's checks do for a freed slot: an id read through an identifier the
//! sweep freed, or an id kept past a collection.
//!
//! [`MarkBits`]: super::gc::MarkBits
//!
//! A plain field of `Heap`, read and written under the heap's borrow beside
//! `symbol_table` (decision 18 on #782). No code outside `patina-core` names
//! an id, so every access already holds that borrow, and
//! `Heap::get_identifier_data` lends a set out of the table.

use rustc_hash::FxHashMap;

use super::GC_CHECK;
use super::gc::BitSet;
use crate::scope::ScopeSet;

/// What an identifier holds for its scopes: the number of its set's entry in
/// its heap's [`ScopeSetTable`]. Opaque: outside core nothing can make one or
/// read its number, so no id names an entry it was not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeSetId(u32);

impl ScopeSetId {
    /// The empty set's entry, made with the table and never given up.
    pub(crate) const EMPTY: ScopeSetId = ScopeSetId(0);

    /// The entry's place in the table.
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }

    fn at(index: usize) -> Self {
        ScopeSetId(u32::try_from(index).expect("more than 2^32 scope sets"))
    }
}

/// One heap's scope sets. See the [module docs](self).
#[derive(Debug)]
pub(crate) struct ScopeSetTable {
    /// Each id's set. Entry 0 is the empty set; a free entry holds it too.
    sets: Vec<ScopeSet>,
    /// Each set but the empty one, by its contents.
    index: FxHashMap<ScopeSet, ScopeSetId>,
    /// The free entries, the lowest last, which `intern` reuses first.
    free: Vec<ScopeSetId>,
}

impl ScopeSetTable {
    /// A table holding the empty set alone, at id 0.
    pub(crate) fn new() -> Self {
        Self {
            sets: vec![ScopeSet::new()],
            index: FxHashMap::default(),
            free: Vec::new(),
        }
    }

    /// The id of `set`'s entry, made if the table has none, in a free entry
    /// if there is one: the empty set's with no lookup.
    pub(crate) fn intern(&mut self, set: ScopeSet) -> ScopeSetId {
        if set.is_empty() {
            return ScopeSetId::EMPTY;
        }
        if let Some(&id) = self.index.get(&set) {
            return id;
        }
        let id = match self.free.pop() {
            Some(id) => {
                self.sets[id.index()] = set.clone();
                id
            }
            None => {
                let id = ScopeSetId::at(self.sets.len());
                self.sets.push(set.clone());
                id
            }
        };
        self.index.insert(set, id);
        id
    }

    /// The set at `id`. In a check build, a free entry's read panics.
    #[inline]
    pub(crate) fn get(&self, id: ScopeSetId) -> &ScopeSet {
        let set = &self.sets[id.index()];
        if GC_CHECK && id != ScopeSetId::EMPTY && set.is_empty() {
            free_entry_read(id);
        }
        set
    }

    /// The entries the table has made, free ones and id 0 included: what a
    /// collection's bitmap is sized to.
    pub(crate) fn entries(&self) -> usize {
        self.sets.len()
    }

    /// The sweep's pass (#788): free every entry `marked` lacks. Its set
    /// leaves the index and drops its spill, the free entries at the end go,
    /// and the free list is made again from the rest. Id 0's bit is set
    /// when the bitmap is made, so the empty set is never freed. A free
    /// entry's bit is never set, since no identifier holds a free id, so it
    /// is found free again and holds the empty set already.
    pub(crate) fn sweep(&mut self, marked: &BitSet) {
        debug_assert_eq!(
            self.sets.len(),
            marked.len(),
            "a scope set was interned while marking"
        );
        for index in 0..self.sets.len() {
            if !marked.get(index) {
                let set = std::mem::take(&mut self.sets[index]);
                if !set.is_empty() {
                    self.index.remove(&set);
                }
            }
        }
        let kept = (0..self.sets.len())
            .rposition(|index| marked.get(index))
            .map_or(0, |last| last + 1);
        self.sets.truncate(kept);
        self.free.clear();
        self.free.extend(
            (0..kept)
                .rev()
                .filter(|&index| !marked.get(index))
                .map(ScopeSetId::at),
        );
    }

    /// The sets held, not counting the empty one.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.index.len()
    }
}

/// The panic of a check build's read of a free entry, out of line so the
/// read stays a load and a branch.
#[cold]
#[inline(never)]
fn free_entry_read(id: ScopeSetId) -> ! {
    panic!(
        "scope set {} is free: an identifier the sweep freed, or an id kept past a collection",
        id.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopeId;

    fn set(scopes: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for &scope in scopes {
            set.add_scope(ScopeId(scope));
        }
        set
    }

    #[test]
    fn the_empty_set_is_id_zero_and_takes_no_entry() {
        let mut table = ScopeSetTable::new();
        assert_eq!(table.intern(ScopeSet::new()), ScopeSetId::EMPTY);
        assert!(table.get(ScopeSetId::EMPTY).is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn a_set_reads_back_as_interned_inline_or_spilled() {
        let mut table = ScopeSetTable::new();
        // Three scopes fit inline; a fourth spills.
        let inline = set(&[3, 1, 2]);
        let spilled = set(&[5, 4, 7, 6]);
        assert_eq!(inline.heap_bytes(), 0);
        assert_ne!(spilled.heap_bytes(), 0);
        let inline_id = table.intern(inline.clone());
        let spilled_id = table.intern(spilled.clone());
        assert_ne!(inline_id, ScopeSetId::EMPTY);
        assert_ne!(spilled_id, inline_id);
        assert_eq!(table.get(inline_id), &inline);
        assert_eq!(table.get(spilled_id), &spilled);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn equal_sets_share_an_id() {
        let mut table = ScopeSetTable::new();
        let first = table.intern(set(&[1, 2, 3, 4]));
        // Built in another order, and so by another path to the same set.
        let second = table.intern(set(&[4, 3, 2, 1]));
        assert_eq!(first, second);
        let other = table.intern(set(&[1, 2, 3]));
        assert_ne!(first, other);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn an_identifier_reads_back_the_set_it_was_given() {
        let mut heap = crate::heap::Heap::new();
        let spilled = set(&[9, 8, 7, 6]);
        let first = heap.alloc_identifier("a".into(), spilled.clone());
        let second = heap.alloc_identifier("b".into(), spilled.clone());
        let written = heap.alloc_source_identifier("c".into());
        let (name, scopes) = heap.get_identifier_data(first).unwrap();
        assert_eq!((&**name, scopes), ("a", &spilled));
        assert_eq!(
            heap.get_identifier_data_any(second),
            Some(("b".into(), spilled))
        );
        assert!(heap.get_identifier_data(written).unwrap().1.is_empty());
        // The two identifiers share one entry; the written one takes id 0.
        assert_eq!(heap.scope_sets.len(), 1);
    }
}
