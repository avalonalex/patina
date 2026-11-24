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
//! **Walks.** A scope edit that gives many identifiers the set it makes of
//! theirs keeps a memo for its walk, from each set's id to the id of the
//! set it made of it ([`ScopeSetMemo`], [`ScopeSetTable::edited`]), and so
//! interns each distinct set once: `Cx::rescope_identifiers` for one call,
//! and `Cx::add_scope_copying` for as long as the caller keeps its
//! `ScopeAddition`. Each memo lives beneath a context, where nothing
//! collects (#702, #707), and a `ScopeAddition` is branded with the
//! context's lifetime, so no id a memo holds is freed while it lives.
//!
//! A free entry holds the empty set, which no other entry but id 0 holds,
//! so in debug and `gc-check` builds a read of a free entry panics, as
//! #621's checks do for a freed slot: an id read through an identifier the
//! sweep freed, or an id kept past a collection.
//!
//! **Charge** (M3). What the table holds beyond the empty set's entry, made
//! with it, is charged as external bytes (`heap/account.rs`): its entries,
//! the index, the free list, and each set's spill, once for its entry and
//! once for its index key. The table charges as it grows, through its own
//! handle, and gives back as the sweep frees sets and it shrinks, before
//! the sweep reads the external bytes to set L, so L counts the live sets'.
//! An identifier's slot is charged nothing for its set. A table that holds
//! the empty set alone charges nothing, so a heap whose identifiers carry
//! no scopes, or whose last collection freed every set, has no charge.
//!
//! A plain field of `Heap`, read and written under the heap's borrow beside
//! `symbol_table` (decision 18 on #782). No code outside `patina-core` names
//! an id, so every access already holds that borrow, and
//! `Heap::get_identifier_data` lends a set out of the table.
//!
//! [`MarkBits`]: super::gc::MarkBits

use std::mem::size_of;

use rustc_hash::FxHashMap;

use super::gc::BitSet;
use super::{ExternalBytes, GC_CHECK};
use crate::environment::hash_table_bytes;
use crate::scope::{ScopeId, ScopeSet};

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

/// A walk's memo (#788): for each set's id it has met, the id of the set its
/// edit made of it. Valid only while nothing collects.
pub(crate) type ScopeSetMemo = FxHashMap<ScopeSetId, ScopeSetId>;

/// One heap's scope sets. See the [module docs](self).
#[derive(Debug)]
pub(crate) struct ScopeSetTable {
    /// Each id's set. Entry 0 is the empty set; a free entry holds it too.
    sets: Vec<ScopeSet>,
    /// Each set but the empty one, by its contents.
    index: FxHashMap<ScopeSet, ScopeSetId>,
    /// The free entries, the lowest last, which `intern` reuses first.
    free: Vec<ScopeSetId>,
    /// The spills of the sets held: each entry's, and each index key's.
    spilled: usize,
    /// The external bytes the table has charged ([`Self::bytes`] when it
    /// last settled), through `account`.
    charged: usize,
    account: ExternalBytes,
}

impl ScopeSetTable {
    /// A table holding the empty set alone, at id 0, which charges what it
    /// holds beyond that through `account`, its heap's external bytes.
    pub(crate) fn new(account: ExternalBytes) -> Self {
        Self {
            sets: vec![ScopeSet::new()],
            index: FxHashMap::default(),
            free: Vec::new(),
            spilled: 0,
            charged: 0,
            account,
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
        let entry = set.clone();
        self.spilled += entry.heap_bytes() + set.heap_bytes();
        let id = match self.free.pop() {
            Some(id) => {
                self.sets[id.index()] = entry;
                id
            }
            None => {
                let id = ScopeSetId::at(self.sets.len());
                self.sets.push(entry);
                id
            }
        };
        self.index.insert(set, id);
        self.settle_charge();
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

    /// The id of the set `edit` makes of the set at `from`, through `memo`:
    /// `edit` runs, and its set is interned, once for each `from` the memo
    /// has not met. So one memo serves one edit, which depends on the set
    /// alone, and lives while nothing collects.
    pub(crate) fn edited(
        &mut self,
        memo: &mut ScopeSetMemo,
        from: ScopeSetId,
        edit: impl FnOnce(&ScopeSet) -> ScopeSet,
    ) -> ScopeSetId {
        if let Some(&to) = memo.get(&from) {
            return to;
        }
        let set = edit(self.get(from));
        let to = self.intern(set);
        memo.insert(from, to);
        to
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
                if let Some((key, _)) = self.index.remove_entry(&set) {
                    self.spilled -= set.heap_bytes() + key.heap_bytes();
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
        self.shrink();
        self.settle_charge();
    }

    /// Give back the room the sweep left: all of it, but the empty set's
    /// entry, once no other set is held, so that the table charges nothing
    /// again; otherwise each buffer that holds four times what it uses.
    fn shrink(&mut self) {
        if self.index.is_empty() {
            self.sets.shrink_to_fit();
            self.index = FxHashMap::default();
            self.free = Vec::new();
            return;
        }
        let roomy = |capacity: usize, len: usize| capacity > (len * 4).max(64);
        if roomy(self.sets.capacity(), self.sets.len()) {
            self.sets.shrink_to(self.sets.len() * 2);
        }
        if roomy(self.index.capacity(), self.index.len()) {
            self.index.shrink_to(self.index.len() * 2);
        }
        if roomy(self.free.capacity(), self.free.len()) {
            self.free.shrink_to(self.free.len() * 2);
        }
    }

    /// What the table holds outside itself beyond the empty set's entry: the
    /// external bytes it charges (the module docs).
    fn bytes(&self) -> usize {
        self.sets.capacity().saturating_sub(1) * size_of::<ScopeSet>()
            + hash_table_bytes::<ScopeSet, ScopeSetId>(self.index.capacity())
            + self.free.capacity() * size_of::<ScopeSetId>()
            + self.spilled
    }

    /// Charge what the table has grown by since it last settled, or give
    /// back what it has shrunk by.
    fn settle_charge(&mut self) {
        let bytes = self.bytes();
        if bytes > self.charged {
            self.account.charge(bytes - self.charged);
        } else if bytes < self.charged {
            self.account.release(self.charged - bytes);
        }
        self.charged = bytes;
    }

    /// The sets held, not counting the empty one: `(gc-stats)`'s
    /// `scope-sets`.
    pub(crate) fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether a set the table holds contains `scope`: `Heap::scope_is_carried`
    /// (#789), a scan of every entry when asked. A free entry holds the
    /// empty set, which contains nothing, so the scan reads each entry
    /// as it is, with no free-list test and no check build's refusal.
    pub(crate) fn carries(&self, scope: ScopeId) -> bool {
        self.sets.iter().any(|set| set.contains(&scope))
    }

    /// The external bytes the table has charged.
    #[cfg(test)]
    pub(crate) fn charged(&self) -> usize {
        self.charged
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

    /// A table charging an account of its own.
    fn table() -> ScopeSetTable {
        ScopeSetTable::new(crate::heap::Heap::new().external_bytes_handle())
    }

    fn set(scopes: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for &scope in scopes {
            set.add_scope(ScopeId(scope));
        }
        set
    }

    #[test]
    fn the_empty_set_is_id_zero_and_takes_no_entry() {
        let mut table = table();
        assert_eq!(table.intern(ScopeSet::new()), ScopeSetId::EMPTY);
        assert!(table.get(ScopeSetId::EMPTY).is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn a_set_reads_back_as_interned_inline_or_spilled() {
        let mut table = table();
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
        let mut table = table();
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

    /// `carries` answers whether any held set contains the scope: one of two
    /// sets, a spilled one included, and not a scope neither holds; once a
    /// collection frees a set its scopes are carried no more, by the free
    /// entry it leaves between two live ones as by the trailing entry the
    /// sweep drops; and the empty set, id 0, carries nothing.
    #[test]
    fn carries_reads_every_held_set_and_no_freed_one() {
        let mut heap = crate::heap::Heap::new();
        assert!(
            !heap.scope_sets.carries(ScopeId(951)),
            "the empty set alone"
        );
        let low = heap.alloc_identifier("low".into(), set(&[951]));
        let dead = heap.alloc_identifier("dead".into(), set(&[952, 953, 954, 955]));
        let high = heap.alloc_identifier("high".into(), set(&[956, 957]));
        let last = heap.alloc_identifier("last".into(), set(&[958]));
        for scope in [951, 952, 955, 956, 958] {
            assert!(heap.scope_sets.carries(ScopeId(scope)), "S{scope}");
        }
        assert!(!heap.scope_sets.carries(ScopeId(959)));
        crate::heap::trace_sentinels::collect_from(&mut heap, &[low, high]);
        crate::heap::sentinels::assert_freed(&heap, dead, "the middle identifier");
        crate::heap::sentinels::assert_freed(&heap, last, "the last identifier");
        for scope in [951, 956, 957] {
            assert!(heap.scope_sets.carries(ScopeId(scope)), "S{scope}");
        }
        for scope in [952, 953, 954, 955, 958] {
            assert!(
                !heap.scope_sets.carries(ScopeId(scope)),
                "S{scope} was freed"
            );
        }
    }

    /// The table charges what it holds as external bytes, a spilled set's
    /// buffer twice (its entry's and its key's), and gives all of it back
    /// once a collection frees every set; L counts the charge while a set
    /// lives.
    #[test]
    fn the_table_charges_its_sets_and_gives_them_back() {
        let mut heap = crate::heap::Heap::new();
        assert_eq!(heap.external_bytes(), 0, "a table of the empty set alone");
        let bare = heap.alloc_identifier("bare".into(), ScopeSet::new());
        assert_eq!(heap.external_bytes(), 0, "the empty set is charged nothing");
        let spilled = set(&[1, 2, 3, 4]);
        let identifier = heap.alloc_identifier("x".into(), spilled.clone());
        let charged = heap.scope_sets.charged();
        assert!(charged >= 2 * spilled.heap_bytes(), "{charged}");
        assert_eq!(heap.external_bytes(), charged);
        crate::heap::trace_sentinels::collect_from(&mut heap, &[identifier, bare]);
        assert_eq!(heap.scope_sets.charged(), charged);
        assert_eq!(heap.external_bytes(), charged);
        assert!(heap.live_bytes() > charged);
        crate::heap::trace_sentinels::collect_from(&mut heap, &[]);
        assert_eq!(heap.scope_sets.len(), 0);
        assert_eq!(heap.scope_sets.charged(), 0);
        assert_eq!(heap.external_bytes(), 0);
    }
}
