//! A namespace's weak entries (#836, R1, R7; `docs/GC_DESIGN.md` §4.5,
//! §4.7): the placeholders code compiled in a parentless namespace links for
//! the names it does not bind yet, and in a host namespace the read-through
//! records over a parent's placeholder.
//!
//! Neither is in a slot, so the namespace's derived trace never marks one,
//! and nothing here is traced: a link table that names an entry marks it as
//! a slot does, and the pass at the sweep's start drops every entry nothing
//! marked ([`PlaceholderList::take_dead`]), while the mark bits are complete
//! and before the pair arena's sweep frees its pairs. So a placeholder lives
//! exactly as long as code that links it, as the issue's R1 has it. A define
//! or an import of the name adopts the entry into a slot, keeping its word,
//! and takes it out of the store, so every entry the trace skips is one the
//! pass may drop (Rule 1): a placeholder in a store holds `UNBOUND`, which a
//! check build asserts after every sweep ([`PlaceholderList::check_swept`]).
//!
//! **Placement** (P2, #836's question 4, pending the owner's ruling): a
//! per-namespace [`PlaceholderStore`], out of the slots, which stay
//! append-only (D1), as a host namespace's strong read-through records are
//! kept out of them; and a per-heap [`PlaceholderList`] of the stores, on
//! the heap's `HandleTable`, for the pass to find them. The list keys each
//! store by its namespace's `env_id`, which is never reused, and holds no
//! namespace, by `Rc` or `Weak`: a `Weak<Environment>` is traced while it
//! upgrades. The namespace holds its store through a [`PlaceholderHandle`],
//! whose drop takes the store off the list at once (M2), so the list never
//! keeps a dead namespace's store, and the store then gives back its counts
//! and its charge as it drops.
//!
//! **Counted and charged borrow-free**, as a namespace's own tables are
//! (`NamespaceCharge` in `environment.rs`): each entry is a binding record in
//! `Heap::binding_records`, and a placeholder's cell one of `Heap::cells`,
//! through the namespace's `ExternalBytes` handle, raised as an entry is
//! made and lowered as one leaves; and the store charges its own allocation,
//! its table's buffer at its capacity and the names it holds, settled as
//! each entry comes and goes. A store's drop can be inside a sweep, which
//! holds the heap mutably, so nothing here borrows the heap to count.
//!
//! **`HeapTable`'s rule.** Both tables are behind `HeapTable`, and nothing
//! drops a name, a store or a namespace inside an access: the pass answers
//! the names it took out for its caller to drop, a store's removal from the
//! list drops the list's `Rc` after the access, and the handle keeps its own
//! `Rc`, so the store itself drops after that.

use std::fmt;
use std::mem::size_of;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use crate::binding_record::{PLACEHOLDER, Record};
use crate::environment::{RC_COUNTS, hash_table_bytes};
use crate::heap::table::HeapTable;
use crate::heap::{ExternalBytes, Heap};
use crate::tagged_value::TaggedValue;

/// What a store's entries are, fixed when it is made: a parentless
/// namespace's store holds placeholders alone, and a host namespace's its
/// weak read-through records alone, since a host namespace makes no
/// placeholder of its own (`Environment::link_record`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreKind {
    /// Placeholders, each `(cell . PLACEHOLDER)` over a cell of the
    /// namespace's own holding `UNBOUND`: a record and a cell each.
    Placeholders,
    /// A host namespace's read-through records over a parent placeholder
    /// (R7), each `(unbound . parent)`: a record each, over the host's one
    /// free cell, which no namespace counts.
    Children,
}

impl StoreKind {
    /// The cells an entry holds of the namespace's own.
    #[inline]
    fn cells_each(self) -> usize {
        match self {
            StoreKind::Placeholders => 1,
            StoreKind::Children => 0,
        }
    }
}

/// A namespace's weak entries (#836, R1, R7): its placeholders, or in a host
/// namespace its read-through records over a parent placeholder. Never
/// traced: a link table's trace marks an entry, and the pass at the sweep's
/// start drops each it did not. Counts and charges its own, borrow-free,
/// and gives them back when it drops.
pub(crate) struct PlaceholderStore {
    /// Its namespace's number, by which the list keys it.
    env_id: u64,
    /// The heap's account, its namespace's: what it counts and charges.
    account: ExternalBytes,
    /// What its entries are.
    kind: StoreKind,
    /// The entries, behind the wrapper every table the heap keeps in Rust
    /// takes (C3).
    state: HeapTable<StoreState>,
}

struct StoreState {
    /// Each name's entry. Its records are the heap's sums' (`records`, and
    /// `cells` for a placeholder's): the table's length is its running sum.
    entries: FxHashMap<Rc<str>, Record>,
    /// The bytes of the names held: each an `Rc<str>` of two counts and its
    /// bytes, counted whether or not something else shares it.
    names: usize,
    /// What the store has charged and not given back.
    charged: usize,
}

impl StoreState {
    /// What the store holds outside the arenas now: its allocation in its
    /// `Rc`, its table's buffer at its capacity, and the names. The records
    /// and cells are pairs, charged in the live bytes.
    fn bytes(&self) -> usize {
        RC_COUNTS
            + size_of::<PlaceholderStore>()
            + hash_table_bytes::<Rc<str>, Record>(self.entries.capacity())
            + self.names
    }

    /// Charge the difference between what the store holds now and what it
    /// charged before, through `account`, borrow-free.
    fn settle(&mut self, account: &ExternalBytes) {
        let bytes = self.bytes();
        let charged = std::mem::replace(&mut self.charged, bytes);
        if bytes > charged {
            account.charge(bytes - charged);
        } else {
            account.release(charged - bytes);
        }
    }
}

impl PlaceholderStore {
    /// An empty store of the namespace numbered `env_id`, counting and
    /// charging through `account`, its namespace's.
    pub(crate) fn new(env_id: u64, account: ExternalBytes, kind: StoreKind) -> Self {
        PlaceholderStore {
            env_id,
            account,
            kind,
            state: HeapTable::new(StoreState {
                entries: FxHashMap::default(),
                names: 0,
                charged: 0,
            }),
        }
    }

    /// What its entries are.
    #[inline]
    pub(crate) fn kind(&self) -> StoreKind {
        self.kind
    }

    /// `name`'s entry, with the name it is held under, shared.
    pub(crate) fn get_key_value(&self, name: &str) -> Option<(Rc<str>, Record)> {
        self.state.with(|state| {
            state
                .entries
                .get_key_value(name)
                .map(|(key, &record)| (Rc::clone(key), record))
        })
    }

    /// `name`'s entry.
    pub(crate) fn get(&self, name: &str) -> Option<Record> {
        self.state.with(|state| state.entries.get(name).copied())
    }

    /// Hold `record` for `key`, which has no entry here: counted in the
    /// heap's sums, a record and for a placeholder its cell, and charged.
    pub(crate) fn insert(&self, key: Rc<str>, record: Record) {
        self.state.with_mut(|state| {
            debug_assert!(
                !state.entries.contains_key(&*key),
                "a store holds one entry per name"
            );
            state.names += RC_COUNTS + key.len();
            state.entries.insert(key, record);
            state.settle(&self.account);
        });
        self.account.records_made(1);
        self.account.cells_made(self.kind.cells_each());
    }

    /// `name`'s entry, taken out: for a define or an import of `name` to
    /// adopt, or for a host namespace to promote (`Environment::link_record`),
    /// each of which counts it again where it goes. Its count leaves the
    /// heap's sums here and its name the charge; the name drops after the
    /// table's access.
    pub(crate) fn take(&self, name: &str) -> Option<Record> {
        let (key, record) = self.state.with_mut(|state| {
            let (key, record) = state.entries.remove_entry(name)?;
            state.names -= RC_COUNTS + key.len();
            state.settle(&self.account);
            Some((key, record))
        })?;
        drop(key);
        self.account.records_let_go(1);
        self.account.cells_let_go(self.kind.cells_each());
        Some(record)
    }

    /// Whether the store holds an entry for `name`: a name a placeholder or
    /// a weak read-through record holds is taken, for the minting of an
    /// alias (`Environment::import_alias`, `relink_alias`).
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.state.with(|state| state.entries.contains_key(name))
    }

    /// The pass's part for this store: take out every entry whose record
    /// `live` says is unmarked, its names pushed onto `dead` for the caller
    /// to drop after the accesses, its counts given back and its charge
    /// settled, borrow-free.
    fn take_dead(&self, live: &mut impl FnMut(TaggedValue) -> bool, dead: &mut Vec<Rc<str>>) {
        let gone = self.state.with_mut(|state| {
            let before = dead.len();
            dead.extend(
                state
                    .entries
                    .extract_if(|_, record| !live(record.word()))
                    .map(|(key, _)| key),
            );
            let gone = &dead[before..];
            if !gone.is_empty() {
                state.names -= gone.iter().map(|key| RC_COUNTS + key.len()).sum::<usize>();
                state.settle(&self.account);
            }
            gone.len()
        });
        if gone > 0 {
            self.account.records_let_go(gone);
            self.account.cells_let_go(gone * self.kind.cells_each());
        }
    }

    /// A check build's part of the sweep, after the arenas, for this store
    /// ([`PlaceholderList::check_swept`]).
    fn check_swept(&self, heap: &Heap) {
        self.state.with(|state| {
            for (name, &record) in &state.entries {
                check_entry(heap, self.kind, name, record);
            }
        });
    }

    /// The records and own cells the store holds, as the heap's sums count
    /// them: one record an entry, and a placeholder's cell.
    #[cfg(test)]
    pub(crate) fn counted(&self) -> (usize, usize) {
        let len = self.state.with(|state| state.entries.len());
        (len, len * self.kind.cells_each())
    }

    /// [`counted`](Self::counted), counted from the entries: each a record,
    /// and each whose cell is its namespace's own, a cell, as
    /// `Environment::recount` counts a namespace's.
    #[cfg(test)]
    pub(crate) fn recount(&self, heap: &crate::Heap) -> (usize, usize) {
        self.state.with(|state| {
            let cells = state
                .entries
                .values()
                .filter(|record| {
                    record.parent_record(heap).is_none()
                        && record.cell(heap).owned_by(heap, self.env_id)
                })
                .count();
            (state.entries.len(), cells)
        })
    }

    /// What the store has charged and not given back.
    #[cfg(test)]
    pub(crate) fn charged(&self) -> usize {
        self.state.with(|state| state.charged)
    }
}

impl Drop for PlaceholderStore {
    /// Give back what the store counted and charged, borrow-free: a store
    /// drops when its namespace does, often inside a sweep.
    fn drop(&mut self) {
        let (len, charged) = self
            .state
            .with(|state| (state.entries.len(), state.charged));
        self.account.records_let_go(len);
        self.account.cells_let_go(len * self.kind.cells_each());
        self.account.release(charged);
    }
}

/// The per-heap list of the stores of the namespaces that hold weak entries,
/// keyed by `env_id` (never reused; not `gc_identity`, an address), for the
/// pass at the sweep's start (#836). On the heap's `HandleTable`, by its own
/// `Rc`, never traced, and carried across teardown, so the teardown sweep's
/// pass empties every store a namespace that outlives its heap still holds.
/// Holds stores and never a namespace. Its own buffer is the heap's, at most
/// an entry a live store, and charged to no namespace.
pub(crate) struct PlaceholderList(HeapTable<FxHashMap<u64, Rc<PlaceholderStore>>>);

impl PlaceholderList {
    /// An empty list: a heap's.
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(PlaceholderList(HeapTable::new(FxHashMap::default())))
    }

    /// List `store`, made for its namespace's first weak entry.
    fn register(&self, store: &Rc<PlaceholderStore>) {
        let replaced = self
            .0
            .with_mut(|stores| stores.insert(store.env_id, Rc::clone(store)));
        debug_assert!(replaced.is_none(), "a namespace has one store");
        drop(replaced);
    }

    /// The pass at the sweep's start (#836), while the mark bits are
    /// complete: take every entry of every listed store whose record `live`
    /// says is unmarked out of its store, and answer their names, for the
    /// caller to drop once the accesses have ended (`HeapTable`'s rule).
    /// Each store gives back the records and cells it let go and settles its
    /// charge, borrow-free. One access over the list and one over each
    /// store, so it costs the stores' lengths; over an empty list, one
    /// access and no allocation.
    pub(crate) fn take_dead(&self, mut live: impl FnMut(TaggedValue) -> bool) -> Vec<Rc<str>> {
        self.0.with(|stores| {
            let mut dead = Vec::new();
            for store in stores.values() {
                store.take_dead(&mut live, &mut dead);
            }
            dead
        })
    }

    /// A check build's part of the sweep, after the arenas, beside the port
    /// table's: every entry the pass left names a live pair in its
    /// generation, and so do the pairs it reaches, a placeholder's cell, and
    /// a weak read-through record's cell and parent; and a placeholder is
    /// still one, flagged and over `UNBOUND`, as Rule 1 has every entry the
    /// trace skips (#836). A dead entry is one the pass left, which only a
    /// pass that skipped it or ran after the pair sweep would leave.
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check", test)),
        expect(
            dead_code,
            reason = "a check build's part of the sweep, and its control's"
        )
    )]
    pub(crate) fn check_swept(&self, heap: &Heap) {
        self.0.with(|stores| {
            for store in stores.values() {
                store.check_swept(heap);
            }
        });
    }

    /// Whether the list holds the store of the namespace numbered `env_id`.
    #[cfg(test)]
    pub(crate) fn holds(&self, env_id: u64) -> bool {
        self.0.with(|stores| stores.contains_key(&env_id))
    }
}

/// In a namespace's `RareTables`: its store, and the hook that takes the
/// store off its heap's list the moment the namespace drops (M2, exactly).
/// It runs borrow-free, wherever the namespace drops: inside a sweep's
/// tombstoning (a specifier's namespace), at teardown as the old handle
/// table goes, or wherever a host lets go of its last `Rc`. Never inside the
/// pass's access, which drops names alone.
pub(crate) struct PlaceholderHandle {
    list: Rc<PlaceholderList>,
    store: Rc<PlaceholderStore>,
}

impl PlaceholderHandle {
    /// `store`, made for its namespace's first weak entry, listed on `list`,
    /// its heap's.
    pub(crate) fn new(list: &Rc<PlaceholderList>, store: PlaceholderStore) -> Self {
        let store = Rc::new(store);
        list.register(&store);
        PlaceholderHandle {
            list: Rc::clone(list),
            store,
        }
    }

    /// The namespace's store.
    #[inline]
    pub(crate) fn store(&self) -> &PlaceholderStore {
        &self.store
    }
}

impl Drop for PlaceholderHandle {
    fn drop(&mut self) {
        // The list's `Rc` drops after the access; the store itself drops
        // with this handle's, after that.
        let gone = self
            .list
            .0
            .with_mut(|stores| stores.remove(&self.store.env_id));
        drop(gone);
    }
}

/// One entry's check ([`PlaceholderList::check_swept`]): its pairs live,
/// each read only once its liveness is known, so a dead one is reported
/// here rather than by the accessors' stale-reference check.
fn check_entry(heap: &Heap, kind: StoreKind, name: &str, record: Record) {
    let word = record.word();
    if !heap.pair_is_live(word) {
        dead_entry_left(name);
    }
    let (car, cdr) = (heap.car(word), heap.cdr(word));
    match kind {
        StoreKind::Placeholders => {
            if !heap.pair_is_live(car) {
                dead_entry_left(name);
            }
            if cdr != TaggedValue::fixnum(PLACEHOLDER) || heap.car(car) != TaggedValue::UNBOUND {
                bound_entry_kept(name);
            }
        }
        StoreKind::Children => {
            if !cdr.is_pair() {
                bound_entry_kept(name);
            }
            if !heap.pair_is_live(car) || !heap.pair_is_live(cdr) {
                dead_entry_left(name);
            }
        }
    }
}

/// A check build's failure: an entry the sweep's pass left in its store
/// names a pair the sweep freed.
#[cold]
#[inline(never)]
fn dead_entry_left(name: &str) -> ! {
    panic!(
        "the sweep's pass left a dead placeholder: {name:?}'s entry names a pair the sweep freed \
         (#836)"
    )
}

/// A check build's failure: a store holds an entry a define or an import
/// adopted, which the trace marks no more than a placeholder, though it is
/// bound (Rule 1).
#[cold]
#[inline(never)]
fn bound_entry_kept(name: &str) -> ! {
    panic!(
        "a placeholder store holds {name:?}'s entry, bound: an adopted record left the store, or \
         the pass could drop a binding (Rule 1, #836)"
    )
}

/// The namespace's number alone: a `Debug` that opens no access, for
/// `RareTables`' derived one.
impl fmt::Debug for PlaceholderHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlaceholderHandle")
            .field("env_id", &self.store.env_id)
            .field("kind", &self.store.kind)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding_record::BindingCell;
    use crate::heap::trace_sentinels::collect_from;

    /// `check_swept`'s positive control: a store entry over a pair a sweep
    /// freed, which only a pass that left it would leave. The store is on a
    /// list the heap's pass never sees, counted through the heap's account so
    /// that its drop as the panic unwinds gives back what it counted; the
    /// collection frees the placeholder's pairs, and the check refuses the
    /// entry with its own message, before any read of the freed pair could
    /// panic with the stale-reference check's.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "the sweep's pass left a dead placeholder")]
    fn a_check_build_refuses_a_dead_placeholder_the_pass_left() {
        let mut heap = Heap::new();
        let unseen = PlaceholderList::new();
        let handle = PlaceholderHandle::new(
            &unseen,
            PlaceholderStore::new(7, heap.external_bytes_handle(), StoreKind::Placeholders),
        );
        let cell = BindingCell::new_own(&mut heap, TaggedValue::UNBOUND, 7);
        let record = Record::placeholder(&mut heap, cell);
        handle.store().insert(Rc::from("later"), record);
        collect_from(&mut heap, &[]);
        unseen.check_swept(&heap);
    }
}
