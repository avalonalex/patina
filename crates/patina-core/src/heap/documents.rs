//! The heap's document tables (#791, GC stage 4c.5): where each identifier
//! was read or expanded, held once, under a [`SourceId`].
//!
//! An identifier's location used to be an entry of `Heap.syntax_sources`, a
//! map keyed by the identifier's raw bits to an `Rc<SourceLocation>`: every
//! copy a scope edit made wrote the map again, and the sweep pruned it key
//! by key. Now an identifier holds the id of an entry here, a copy takes its
//! original's id with no write, and the sweep frees the entries no marked
//! identifier carries. Pairs, vectors and strings keep their
//! `syntax_sources` entries until #792 and #793.
//!
//! **Slots.** One for each document the heap's identifiers refer to,
//! holding the `Arc<SourceDocument>` and the source name its locations
//! report. A location with no document (`span: None`) has a slot of its
//! name alone, so it keeps its line and column. A slot lives while an
//! entry names it, as the entry's own or as its form's, and is dropped at
//! the sweep that frees the last of them. The document itself lives on in
//! whatever `SourceLocation` still holds it: the table owns no text.
//!
//! **Entries.** An identifier's own span (line, column, length, and the
//! exclusive end where it has a document), its expansion chain, and, for an
//! identifier at the head of a list, the list's span too: its *form span*
//! (decision 16 on #782, ruled 2026-11-26: both spans are kept, since a
//! reference a template introduces reports its own text and a form the
//! template builds reports the use). A form span has a slot of its own,
//! because the two can be in different documents. Entries are interned by
//! all of that, the chain by its newest link's identity (decision 14: chains
//! are not interned), so the identifiers an expansion gives one location
//! and one chain share an entry.
//!
//! Id 0 is no location, and no entry.
//!
//! **Liveness.** The table holds no value and is no root. Marking an
//! identifier sets its entry's bit in the collection's [`MarkBits`], a
//! bitmap sized to the table when the collection starts. The sweep, beside
//! the scope-set table's, frees every entry whose bit is clear
//! ([`DocumentTable::sweep`]): it leaves the index, drops its chain and
//! counts out of its slots. A slot left with no entry is dropped then. The
//! entries kept are numbered again from 1, in their order, and the heap
//! gives every live identifier its entry's new id in the same sweep, so the
//! table holds what lives and no more: a few entries made late no longer
//! keep it at its high-water mark.
//!
//! **Ids are set before a collection sees them.** A source id is given when
//! its identifier is made, or by the reader or an expansion's stamp within
//! the context that made the identifier, beneath which nothing collects. So
//! an entry made since the last collection is carried only by identifiers
//! made since then, which a minor collection that prunes young entries
//! alone relies on from stage 7 (`PRD/GC_PRD.md` §9.9).
//!
//! In debug and `gc-check` builds a read of an entry the sweep freed
//! panics, as #621's checks do for a freed slot, where the table no longer
//! reaches that far: an id read through an identifier the sweep freed, or an
//! id kept past a collection. Within the table such an id reads another
//! entry, which is why no id is kept outside an identifier.
//!
//! **Charge** (M3). The table's buffers are charged as external bytes
//! (`heap/account.rs`): the entries, the index, the slots and their maps,
//! and the free slots. It charges as it grows, through its own handle, and
//! gives back as the sweep frees entries and it shrinks, before the sweep
//! reads the external bytes to set L. A table with no entry charges
//! nothing. A chain's links and a document's text are not the table's: a
//! link is shared with every location that extends it, and a document with
//! the locations that hold it.
//!
//! A plain field of `Heap`, read and written under the heap's borrow beside
//! the scope-set table (decision 18 on #782, ruled 2026-11-26). No code
//! outside `patina-core` names an id.
//!
//! [`MarkBits`]: super::gc::MarkBits

use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};

use super::gc::BitSet;
use super::{ExternalBytes, GC_CHECK};
use crate::SourceLocation;
use crate::environment::hash_table_bytes;
use crate::source_document::{ExpansionChain, SourceDocument, SourceSpan};

/// What an identifier holds for its location: the number of its entry in
/// its heap's [`DocumentTable`], or 0 for none. Opaque: outside core
/// nothing can make one or read its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(u32);

impl SourceId {
    /// No location.
    pub(crate) const NONE: SourceId = SourceId(0);

    /// The entry's place in the table.
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }

    /// Whether this is [`SourceId::NONE`].
    #[inline]
    pub(crate) fn is_none(self) -> bool {
        self.0 == 0
    }

    fn at(index: usize) -> Self {
        SourceId(u32::try_from(index).expect("more than 2^32 source entries"))
    }
}

/// How a sweep that compacted the table numbered the entries it kept
/// ([`DocumentTable::sweep`]): each old id's new one, and 0 for an entry it
/// freed.
pub(crate) struct Renumbering(Box<[u32]>);

impl Renumbering {
    /// `id`'s new id. A live identifier's entry is never freed, since
    /// marking it marked the entry; in a check build an id the sweep freed
    /// panics.
    #[inline]
    pub(crate) fn apply(&self, id: SourceId) -> SourceId {
        let new = SourceId(self.0[id.index()]);
        if GC_CHECK && new.is_none() && !id.is_none() {
            free_entry_read(id);
        }
        new
    }
}

/// A length that is none: `SourceLocation.length` is an `Option<u32>`.
const NO_LENGTH: u32 = u32::MAX;

/// A form span's slot when the identifier heads no list.
const NO_FORM: u32 = 0;

/// A form span's slot when the identifier heads more than one list, which
/// only a datum label can make of it: it has no form span, and never takes
/// one ("lost, never misattributed").
const SHARED_FORM: u32 = u32::MAX;

/// A span in a slot: where it starts, how long it is, and its exclusive
/// end, which means something only where the slot has a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Span {
    slot: u32,
    line: u32,
    column: u32,
    length: u32,
    end_line: u32,
    end_column: u32,
}

impl Span {
    /// A span in no slot: a free entry's own span, and the form span of an
    /// identifier that heads no list.
    const NONE: Span = Span {
        slot: 0,
        line: 0,
        column: 0,
        length: 0,
        end_line: 0,
        end_column: 0,
    };

    /// The form span of an identifier that heads more than one list.
    const SHARED: Span = Span {
        slot: SHARED_FORM,
        ..Span::NONE
    };

    /// Whether the span is in a slot of the table.
    fn in_slot(&self) -> bool {
        self.slot != NO_FORM && self.slot != SHARED_FORM
    }
}

/// One entry: an identifier's own span, its chain, and its form span. A
/// free entry's own span is [`Span::NONE`], which no live entry's is.
#[derive(Debug, Clone)]
struct Entry {
    own: Span,
    form: Span,
    chain: Option<ExpansionChain>,
}

impl Entry {
    const FREE: Entry = Entry {
        own: Span::NONE,
        form: Span::NONE,
        chain: None,
    };

    fn is_free(&self) -> bool {
        self.own.slot == 0
    }

    /// The chain's identity: its newest link's address, which nothing else
    /// has while the entry holds the chain. 0 for no chain.
    fn chain_id(&self) -> usize {
        self.chain.as_ref().map_or(0, ExpansionChain::id)
    }

    /// Whether two entries are one: the same spans, and chains that are
    /// one link (decision 14 on #782).
    fn same(&self, other: &Entry) -> bool {
        self.own == other.own && self.form == other.form && self.chain_id() == other.chain_id()
    }

    fn hash(&self) -> u64 {
        let mut hasher = FxHasher::default();
        self.own.hash(&mut hasher);
        self.form.hash(&mut hasher);
        self.chain_id().hash(&mut hasher);
        hasher.finish()
    }
}

/// One slot: a document and the name its locations report, or a name alone.
/// A free slot holds no document, an empty name and no entry.
#[derive(Debug)]
struct Slot {
    /// Whether the slot is in use: slot 0 and a dropped slot are not.
    held: bool,
    name: Arc<str>,
    document: Option<Arc<SourceDocument>>,
    /// The entries that name the slot, as their own span's or their form's
    /// (an entry whose two spans are in one slot counts twice).
    entries: u32,
}

impl Slot {
    fn free() -> Self {
        Slot {
            held: false,
            name: Arc::from(""),
            document: None,
            entries: 0,
        }
    }

    /// The document's identity: its address, which nothing else has while
    /// the slot holds it. 0 for no document.
    fn document_id(&self) -> usize {
        document_id(self.document.as_ref())
    }
}

fn document_id(document: Option<&Arc<SourceDocument>>) -> usize {
    document.map_or(0, |document| Arc::as_ptr(document) as usize)
}

/// The entries by their contents: an open-addressed table of entry ids,
/// probed linearly. It holds ids only, four bytes a bucket, since an
/// entry's contents are in the table's `entries`; a map keyed by the
/// contents would hold each a second time. Nothing is removed from it: the
/// sweep makes it again from the entries it kept.
#[derive(Debug, Default)]
struct Index {
    /// Each bucket an entry's id, or 0 for none. A power of two, or empty.
    buckets: Vec<u32>,
    len: usize,
}

impl Index {
    /// The id of the entry in `entries` that is `entry`, if one is.
    fn find(&self, entries: &[Entry], entry: &Entry, hash: u64) -> Option<u32> {
        if self.buckets.is_empty() {
            return None;
        }
        let mask = self.buckets.len() - 1;
        let mut at = hash as usize & mask;
        loop {
            let id = self.buckets[at];
            if id == 0 {
                return None;
            }
            if entries[id as usize].same(entry) {
                return Some(id);
            }
            at = (at + 1) & mask;
        }
    }

    /// Put `id` in, an entry of `entries` the index does not hold, growing
    /// first if that would fill more than half of it.
    fn insert(&mut self, entries: &[Entry], id: u32) {
        if (self.len + 1) * 2 > self.buckets.len() {
            // The new entry is in `entries` already, so making the index
            // again places it.
            self.resize(entries, (self.len + 1) * 4);
            return;
        }
        self.place(entries[id as usize].hash(), id);
        self.len += 1;
    }

    fn place(&mut self, hash: u64, id: u32) {
        let mask = self.buckets.len() - 1;
        let mut at = hash as usize & mask;
        while self.buckets[at] != 0 {
            at = (at + 1) & mask;
        }
        self.buckets[at] = id;
    }

    /// Make the index again, of every live entry of `entries`, with room
    /// for at least `room` buckets: none at all for no entry.
    fn resize(&mut self, entries: &[Entry], room: usize) {
        let live = entries.iter().filter(|entry| !entry.is_free()).count();
        if live == 0 && room == 0 {
            *self = Index::default();
            return;
        }
        let buckets = room.max(live * 2).max(8).next_power_of_two();
        self.buckets = vec![0; buckets];
        self.len = live;
        for (id, entry) in entries.iter().enumerate() {
            if !entry.is_free() {
                self.place(entry.hash(), id as u32);
            }
        }
    }

    fn bytes(&self) -> usize {
        self.buckets.capacity() * size_of::<u32>()
    }
}

/// One heap's document tables. See the [module docs](self).
#[derive(Debug)]
pub(crate) struct DocumentTable {
    /// Each id's entry. Entry 0 is no entry, and free; outside a sweep no
    /// other is, since the sweep compacts the table.
    entries: Vec<Entry>,
    index: Index,
    /// Each slot. Slot 0 is no slot, and free; so is a dropped one.
    slots: Vec<Slot>,
    /// The slots of each document, by its address: one, but for a document
    /// whose locations report more than one name.
    by_document: FxHashMap<usize, Vec<u32>>,
    /// The slot of each name that has locations with no document.
    by_name: FxHashMap<Arc<str>, u32>,
    free_slots: Vec<u32>,
    /// The slot the last lookup found: a run of locations is of one
    /// document.
    last_slot: u32,
    /// The slots held: `(gc-stats)`'s `documents`.
    held_slots: usize,
    /// The external bytes the table has charged ([`Self::bytes`] when it
    /// last settled), through `account`.
    charged: usize,
    account: ExternalBytes,
}

impl DocumentTable {
    /// A table with no entry and no slot, which charges what it comes to
    /// hold through `account`, its heap's external bytes.
    pub(crate) fn new(account: ExternalBytes) -> Self {
        Self {
            entries: vec![Entry::FREE],
            index: Index::default(),
            slots: vec![Slot::free()],
            by_document: FxHashMap::default(),
            by_name: FxHashMap::default(),
            free_slots: Vec::new(),
            last_slot: 0,
            held_slots: 0,
            charged: 0,
            account,
        }
    }

    /// The id of the entry for `location` as an identifier's own span and
    /// chain, with no form span, made if the table has none.
    pub(crate) fn intern(&mut self, location: &SourceLocation) -> SourceId {
        let entry = Entry {
            own: self.span_of(location),
            form: Span::NONE,
            chain: chain_of(location),
        };
        self.intern_entry(entry)
    }

    /// [`Self::intern`] for an identifier at the head of the list at
    /// `form`: the reader's, when the list closes.
    pub(crate) fn intern_head(
        &mut self,
        location: &SourceLocation,
        form: &SourceLocation,
    ) -> SourceId {
        let entry = Entry {
            own: self.span_of(location),
            form: self.span_of(form),
            chain: chain_of(location),
        };
        self.intern_entry(entry)
    }

    /// The id of the entry that is `id`'s with `location` as its own span
    /// and chain: its form span is kept. An expansion's stamp, which gives
    /// an identifier its chain, and so must not lose the list it heads.
    /// [`Self::intern`] for no entry.
    pub(crate) fn relocated(&mut self, id: SourceId, location: &SourceLocation) -> SourceId {
        if id.is_none() {
            return self.intern(location);
        }
        let entry = Entry {
            own: self.span_of(location),
            form: self.get(id).form,
            chain: chain_of(location),
        };
        self.intern_entry(entry)
    }

    /// The id of the entry that is `id`'s with the list at `form` as its
    /// form span: for an identifier found at the head of a list after it
    /// was given its own entry. One that heads a list already heads two,
    /// through a datum label, and has no form span from here on. No entry
    /// for no entry: a form span hangs on an identifier's own location.
    pub(crate) fn heading(&mut self, id: SourceId, form: &SourceLocation) -> SourceId {
        if id.is_none() {
            return id;
        }
        let old = self.get(id).clone();
        let form = if old.form == Span::NONE {
            self.span_of(form)
        } else {
            Span::SHARED
        };
        self.intern_entry(Entry { form, ..old })
    }

    /// The location at `id`: its own span and chain, as the
    /// `SourceLocation` it was interned from. `None` for no entry.
    pub(crate) fn location(&self, id: SourceId) -> Option<SourceLocation> {
        if id.is_none() {
            return None;
        }
        let entry = self.get(id);
        Some(self.location_of(&entry.own, entry.chain.as_ref()))
    }

    /// The form span at `id`, as a location with the entry's chain: the
    /// list the identifier heads. `None` for no entry, and for an
    /// identifier that heads no list, or more than one.
    pub(crate) fn form_location(&self, id: SourceId) -> Option<SourceLocation> {
        if id.is_none() {
            return None;
        }
        let entry = self.get(id);
        entry
            .form
            .in_slot()
            .then(|| self.location_of(&entry.form, entry.chain.as_ref()))
    }

    /// The entries the table has made, free ones and id 0 included: what a
    /// collection's bitmap is sized to.
    pub(crate) fn entries(&self) -> usize {
        self.entries.len()
    }

    /// The entries held.
    pub(crate) fn len(&self) -> usize {
        self.index.len
    }

    /// The slots held: `(gc-stats)`'s `documents`. After a collection each
    /// has a live entry.
    pub(crate) fn documents(&self) -> usize {
        self.held_slots
    }

    /// The sweep's pass (#791): free every entry `marked` lacks, drop every
    /// slot that leaves with no entry, and number the entries kept again
    /// from 1, in their order. A freed entry drops its chain and counts out
    /// of its slots; the table is cut to the entries kept, their room set by
    /// their number ([`Self::shrink`]), and the index is made again. Bit 0
    /// is never read: entry 0 is no entry.
    ///
    /// Answers the renumbering when an entry moved, which the heap applies
    /// to every live identifier before anything reads an id again, and
    /// `None` when none did. Without it a few live entries made late held
    /// the table at its high-water mark: in #791's gate, 17 live entries at
    /// ids up to 1,838 kept 1,839 entries and up to 440 KB charged, and the
    /// charge followed how many identifiers had died since the last
    /// collection, not what lived.
    #[must_use = "a renumbering is applied to every live identifier"]
    pub(crate) fn sweep(&mut self, marked: &BitSet) -> Option<Renumbering> {
        debug_assert_eq!(
            self.entries.len(),
            marked.len(),
            "a source entry was interned while marking"
        );
        let mut freed = false;
        for index in 1..self.entries.len() {
            if marked.get(index) {
                continue;
            }
            let entry = std::mem::replace(&mut self.entries[index], Entry::FREE);
            self.slots[entry.own.slot as usize].entries -= 1;
            if entry.form.in_slot() {
                self.slots[entry.form.slot as usize].entries -= 1;
            }
            freed = true;
        }
        if !freed {
            return None;
        }
        for slot in 1..self.slots.len() {
            if self.slots[slot].held && self.slots[slot].entries == 0 {
                self.drop_slot(slot as u32);
            }
        }
        let mut renumbered = vec![0u32; self.entries.len()].into_boxed_slice();
        let mut kept = 1;
        let mut moved = false;
        for index in 1..self.entries.len() {
            if self.entries[index].is_free() {
                continue;
            }
            if kept != index {
                // Every entry between `kept` and `index` is free.
                self.entries.swap(kept, index);
                moved = true;
            }
            renumbered[index] = kept as u32;
            kept += 1;
        }
        self.entries.truncate(kept);
        self.shrink();
        self.index.resize(&self.entries, 0);
        self.settle_charge();
        moved.then_some(Renumbering(renumbered))
    }

    /// The entry at `id`, which is not [`SourceId::NONE`]. In a check
    /// build, the read of an id past the table, which a sweep freed and
    /// compacted away, panics.
    #[inline]
    fn get(&self, id: SourceId) -> &Entry {
        if GC_CHECK {
            match self.entries.get(id.index()) {
                Some(entry) if !entry.is_free() => entry,
                _ => free_entry_read(id),
            }
        } else {
            &self.entries[id.index()]
        }
    }

    /// `entry`'s id: the one the index has, or a new one at the end. A new
    /// entry counts into its slots.
    fn intern_entry(&mut self, entry: Entry) -> SourceId {
        let hash = entry.hash();
        if let Some(id) = self.index.find(&self.entries, &entry, hash) {
            return SourceId(id);
        }
        self.slots[entry.own.slot as usize].entries += 1;
        if entry.form.in_slot() {
            self.slots[entry.form.slot as usize].entries += 1;
        }
        let id = SourceId::at(self.entries.len()).0;
        self.entries.push(entry);
        self.index.insert(&self.entries, id);
        self.settle_charge();
        SourceId(id)
    }

    /// `location`'s span, in the slot of its document and name, which is
    /// made if the table has none.
    fn span_of(&mut self, location: &SourceLocation) -> Span {
        let span = location.span.as_ref();
        Span {
            slot: self.slot_for(&location.source, span.map(|span| &span.document)),
            line: location.line,
            column: location.column,
            length: location.length.unwrap_or(NO_LENGTH),
            end_line: span.map_or(0, |span| span.end_line),
            end_column: span.map_or(0, |span| span.end_column),
        }
    }

    /// The location `span` and `chain` were interned from.
    fn location_of(&self, span: &Span, chain: Option<&ExpansionChain>) -> SourceLocation {
        let slot = &self.slots[span.slot as usize];
        SourceLocation {
            source: Arc::clone(&slot.name),
            line: span.line,
            column: span.column,
            length: (span.length != NO_LENGTH).then_some(span.length),
            span: slot.document.as_ref().map(|document| SourceSpan {
                document: Arc::clone(document),
                end_line: span.end_line,
                end_column: span.end_column,
                expansion_chain: chain.cloned(),
            }),
        }
    }

    /// The slot of `document` under `name`, or of `name` alone, made if the
    /// table has none. The last slot found is tried first.
    fn slot_for(&mut self, name: &Arc<str>, document: Option<&Arc<SourceDocument>>) -> u32 {
        let id = document_id(document);
        let is = |slot: &Slot| {
            slot.document_id() == id && (Arc::ptr_eq(&slot.name, name) || slot.name == *name)
        };
        if self.last_slot != 0 && is(&self.slots[self.last_slot as usize]) {
            return self.last_slot;
        }
        let found = if id == 0 {
            self.by_name.get(&**name).copied()
        } else {
            self.by_document.get(&id).and_then(|slots| {
                slots
                    .iter()
                    .copied()
                    .find(|&slot| self.slots[slot as usize].name == *name)
            })
        };
        let slot = found.unwrap_or_else(|| self.make_slot(name, document));
        self.last_slot = slot;
        slot
    }

    fn make_slot(&mut self, name: &Arc<str>, document: Option<&Arc<SourceDocument>>) -> u32 {
        let made = Slot {
            held: true,
            name: Arc::clone(name),
            document: document.cloned(),
            entries: 0,
        };
        let slot = match self.free_slots.pop() {
            Some(slot) => {
                self.slots[slot as usize] = made;
                slot
            }
            None => {
                let slot = u32::try_from(self.slots.len()).expect("more than 2^32 documents");
                assert!(slot != SHARED_FORM, "more than 2^32 documents");
                self.slots.push(made);
                slot
            }
        };
        match document {
            Some(document) => self
                .by_document
                .entry(document_id(Some(document)))
                .or_default()
                .push(slot),
            None => {
                self.by_name.insert(Arc::clone(name), slot);
            }
        }
        self.held_slots += 1;
        slot
    }

    /// Drop `slot`, which no entry names: it leaves its map, lets go of its
    /// document and its name, and is free.
    fn drop_slot(&mut self, slot: u32) {
        let dropped = std::mem::replace(&mut self.slots[slot as usize], Slot::free());
        match &dropped.document {
            Some(document) => {
                let id = document_id(Some(document));
                if let Some(slots) = self.by_document.get_mut(&id) {
                    slots.retain(|&held| held != slot);
                    if slots.is_empty() {
                        self.by_document.remove(&id);
                    }
                }
            }
            None => {
                self.by_name.remove(&*dropped.name);
            }
        }
        if self.last_slot == slot {
            self.last_slot = 0;
        }
        self.held_slots -= 1;
        self.free_slots.push(slot);
    }

    /// Give back the room the sweep left: all of it once no entry is held,
    /// so that the table charges nothing again. Otherwise the entries, which
    /// hold what lives once compacted, get room for twice their number,
    /// rounded up to a power of two and at least 64: a function of what
    /// lives, so that two collections that leave the same entries leave the
    /// same charge, whatever was read between them. Not to fit: the entries
    /// the next forms read would grow the buffer again after every
    /// collection, and each growth is charged as bytes allocated, which no
    /// collection reclaims, since giving back external bytes is not a
    /// reclamation (`ExternalBytes::release`). Each map is cut to twice what
    /// it uses once it holds four times that.
    fn shrink(&mut self) {
        if self.entries.len() == 1 {
            debug_assert_eq!(self.held_slots, 0, "a slot with no entry");
            self.entries.shrink_to_fit();
            self.slots.truncate(1);
            self.slots.shrink_to_fit();
            self.by_document = FxHashMap::default();
            self.by_name = FxHashMap::default();
            self.free_slots = Vec::new();
            self.last_slot = 0;
            return;
        }
        let room = (self.entries.len() * 2).next_power_of_two().max(64);
        if self.entries.capacity() > room {
            self.entries.shrink_to(room);
        } else {
            self.entries.reserve_exact(room - self.entries.len());
        }
        let roomy = |capacity: usize, len: usize| capacity > (len * 4).max(64);
        if roomy(self.by_document.capacity(), self.by_document.len()) {
            self.by_document.shrink_to(self.by_document.len() * 2);
        }
        if roomy(self.by_name.capacity(), self.by_name.len()) {
            self.by_name.shrink_to(self.by_name.len() * 2);
        }
    }

    /// What the table holds outside itself beyond entry 0 and slot 0: the
    /// external bytes it charges (the module docs).
    fn bytes(&self) -> usize {
        self.entries.capacity().saturating_sub(1) * size_of::<Entry>()
            + self.index.bytes()
            + self.slots.capacity().saturating_sub(1) * size_of::<Slot>()
            + hash_table_bytes::<usize, Vec<u32>>(self.by_document.capacity())
            // Each document's list of slots: one slot, in a buffer of four.
            + self.by_document.len() * 4 * size_of::<u32>()
            + hash_table_bytes::<Arc<str>, u32>(self.by_name.capacity())
            + self.free_slots.capacity() * size_of::<u32>()
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

    /// The external bytes the table has charged: `(gc-stats)`'s
    /// `document-bytes`.
    pub(crate) fn charged(&self) -> usize {
        self.charged
    }
}

/// The chain `location` carries, which lives in its span.
fn chain_of(location: &SourceLocation) -> Option<ExpansionChain> {
    location.span.as_ref()?.expansion_chain.clone()
}

/// The panic of a check build's read of a freed entry, out of line so the
/// read stays a load and a branch.
#[cold]
#[inline(never)]
fn free_entry_read(id: SourceId) -> ! {
    panic!(
        "source entry {} is free, or past the compacted table: an identifier the sweep freed, \
         or an id kept past a collection",
        id.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceMap;

    /// A table charging an account of its own.
    fn table() -> DocumentTable {
        DocumentTable::new(crate::heap::Heap::new().external_bytes_handle())
    }

    /// A map over one document, and a location in it.
    fn map(text: &str) -> SourceMap {
        let mut map = SourceMap::new();
        map.set_source_text(text.into());
        map
    }

    /// A bitmap of `table`'s size with `ids` marked.
    fn marks(table: &DocumentTable, ids: &[SourceId]) -> BitSet {
        let mut marked = BitSet::new(table.entries());
        for id in ids {
            marked.set(id.index());
        }
        marked
    }

    #[test]
    fn a_location_reads_back_as_interned() {
        let mut table = table();
        let map = map("(alpha beta)\n(gamma)");
        let alpha = map.location("a.scm", 1, 2, 1, 7);
        let gamma = map.location("a.scm", 2, 2, 2, 7);
        let bare = SourceLocation::new("<string>", 3, 4);
        let ids = [
            table.intern(&alpha),
            table.intern(&gamma),
            table.intern(&bare),
        ];
        assert_eq!(table.location(ids[0]), Some(alpha.clone()));
        assert_eq!(table.location(ids[1]), Some(gamma));
        assert_eq!(table.location(ids[2]), Some(bare));
        assert_eq!(table.location(SourceId::NONE), None);
        // The same location again is the same entry.
        assert_eq!(table.intern(&alpha), ids[0]);
        assert_eq!(table.len(), 3);
        // One slot for the document and one for the bare name.
        assert_eq!(table.documents(), 2);
    }

    #[test]
    fn a_chain_is_part_of_an_entry_by_its_link() {
        let mut table = table();
        let map = map("(m x)");
        let plain = map.location("a.scm", 1, 4, 1, 5);
        let chained = |chain: &ExpansionChain| {
            let mut location = plain.clone();
            location.span.as_mut().unwrap().expansion_chain = Some(chain.clone());
            location
        };
        let first = ExpansionChain::extend(None, "m".into(), 1);
        let second = ExpansionChain::extend(None, "m".into(), 1);
        let a = table.intern(&chained(&first));
        // The same link is the same entry; an equal chain of another link
        // is another (decision 14: chains are not interned).
        assert_eq!(table.intern(&chained(&first)), a);
        let b = table.intern(&chained(&second));
        assert_ne!(a, b);
        assert_ne!(table.intern(&plain), a);
        assert_eq!(table.location(a), Some(chained(&first)));
        let read = table.location(b).unwrap().span.unwrap().expansion_chain;
        assert_eq!(read.map(|chain| chain.id()), Some(second.id()));
    }

    #[test]
    fn a_head_keeps_its_form_span_through_a_stamp_and_loses_it_to_a_second_list() {
        let mut table = table();
        let map = map("(f 1) (g 2)");
        let f = map.location("a.scm", 1, 2, 1, 3);
        let list = map.location("a.scm", 1, 1, 1, 6);
        let other = map.location("a.scm", 1, 7, 1, 12);
        let head = table.intern_head(&f, &list);
        assert_eq!(table.location(head), Some(f.clone()));
        assert_eq!(table.form_location(head), Some(list.clone()));
        // A head and a non-head at one place are two entries.
        let plain = table.intern(&f);
        assert_ne!(plain, head);
        assert_eq!(table.form_location(plain), None);
        // Found at a list's head later, it takes the list.
        assert_eq!(table.heading(plain, &list), head);
        // A stamp's new location keeps the form span, with the new chain.
        let chain = ExpansionChain::extend(None, "m".into(), 1);
        let mut stamped = f.clone();
        stamped.span.as_mut().unwrap().expansion_chain = Some(chain.clone());
        let after = table.relocated(head, &stamped);
        assert_eq!(table.location(after), Some(stamped.clone()));
        let form = table.form_location(after).unwrap();
        assert_eq!((form.line, form.column, form.length), (1, 1, Some(5)));
        assert_eq!(form.span.unwrap().expansion_chain, Some(chain));
        // Heading a second list, it has no form span, and takes none again.
        let shared = table.heading(head, &other);
        assert_eq!(table.location(shared), Some(f));
        assert_eq!(table.form_location(shared), None);
        assert_eq!(table.heading(shared, &list), shared);
        // No entry takes no form span.
        assert_eq!(table.heading(SourceId::NONE, &list), SourceId::NONE);
        // And a stamp of no entry is the location alone.
        let bare = table.relocated(SourceId::NONE, &stamped);
        assert_eq!(bare, table.intern(&stamped));
        assert_ne!(bare, after);
    }

    #[test]
    fn a_form_span_may_be_in_another_document() {
        let mut table = table();
        let library = map("(define-syntax m (syntax-rules () ((_) (car 5))))");
        let program = map("(m)");
        let car = library.location("lib.sld", 1, 40, 1, 43);
        let use_site = program.location("main.scm", 1, 1, 1, 4);
        let head = table.intern_head(&car, &use_site);
        assert_eq!(table.location(head), Some(car));
        assert_eq!(table.form_location(head), Some(use_site));
        assert_eq!(table.documents(), 2);
        // Freed, both slots go.
        let marked = marks(&table, &[]);
        assert!(
            table.sweep(&marked).is_none(),
            "nothing kept, nothing moved"
        );
        assert_eq!((table.len(), table.documents()), (0, 0));
    }

    #[test]
    fn the_sweep_compacts_the_entries_it_keeps_and_renumbers_them() {
        let mut table = table();
        let map = map("a b c d");
        let at = |column| map.location("a.scm", 1, column, 1, column + 1);
        let ids: Vec<_> = [1, 3, 5, 7]
            .map(|column| table.intern(&at(column)))
            .to_vec();
        let marked = marks(&table, &[ids[1], ids[2]]);
        let renumbering = table.sweep(&marked).expect("two entries moved");
        assert_eq!(table.len(), 2);
        assert_eq!(table.entries(), 3, "id 0 and the two kept");
        // The kept entries are 1 and 2, in their order, and read as before.
        let kept = [renumbering.apply(ids[1]), renumbering.apply(ids[2])];
        assert_eq!(kept.map(SourceId::index), [1, 2]);
        assert_eq!(table.location(kept[0]), Some(at(3)));
        assert_eq!(table.location(kept[1]), Some(at(5)));
        assert_eq!(renumbering.apply(SourceId::NONE), SourceId::NONE);
        // The index was made again: the kept locations are their entries,
        // and a new one, or the freed one again, goes at the end.
        assert_eq!(table.intern(&at(3)), kept[0]);
        assert_eq!(table.intern(&at(9)).index(), 3);
        assert_eq!(table.intern(&at(7)).index(), 4);
        assert_eq!(table.documents(), 1);
    }

    #[test]
    fn a_sweep_that_moves_no_entry_answers_no_renumbering() {
        let mut table = table();
        let map = map("a b c");
        let at = |column| map.location("a.scm", 1, column, 1, column + 1);
        let ids: Vec<_> = [1, 3, 5].map(|column| table.intern(&at(column))).to_vec();
        // Nothing freed.
        assert!(table.sweep(&marks(&table, &ids)).is_none());
        // Only the last freed: the others stay where they are.
        assert!(table.sweep(&marks(&table, &ids[..2])).is_none());
        assert_eq!(table.entries(), 3);
        assert_eq!(table.location(ids[1]), Some(at(3)));
    }

    #[test]
    fn a_slot_with_no_entry_is_dropped_and_its_document_released() {
        let mut table = table();
        let kept = map("kept");
        let dropped = map("dropped");
        let live = table.intern(&kept.location("kept.scm", 1, 1, 1, 5));
        let location = dropped.location("dropped.scm", 1, 1, 1, 8);
        let document = Arc::clone(&location.span.as_ref().unwrap().document);
        let dead = table.intern(&location);
        drop(location);
        drop(dropped);
        assert_eq!(table.documents(), 2);
        assert!(Arc::strong_count(&document) >= 2, "the slot holds it");
        let marked = marks(&table, &[live]);
        assert!(table.sweep(&marked).is_none(), "the live entry is first");
        assert_eq!(table.documents(), 1);
        assert_eq!(Arc::strong_count(&document), 1, "the slot let go of it");
        let _ = dead;
        // The same document again takes a slot again.
        let again = SourceLocation {
            source: "dropped.scm".into(),
            line: 1,
            column: 1,
            length: Some(7),
            span: Some(SourceSpan {
                document,
                end_line: 1,
                end_column: 8,
                expansion_chain: None,
            }),
        };
        let id = table.intern(&again);
        assert_eq!(table.location(id), Some(again));
        assert_eq!(table.documents(), 2);
    }

    #[test]
    fn one_document_under_two_names_is_two_slots() {
        let mut table = table();
        let map = map("x");
        let first = map.location("a.scm", 1, 1, 1, 2);
        let mut second = first.clone();
        second.source = "b.scm".into();
        let a = table.intern(&first);
        let b = table.intern(&second);
        assert_ne!(a, b);
        assert_eq!(table.location(a), Some(first));
        assert_eq!(table.location(b), Some(second));
        assert_eq!(table.documents(), 2);
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "is free, or past the compacted table")]
    fn a_check_build_refuses_a_free_source_entry() {
        let mut table = table();
        let map = map("a b");
        let live = table.intern(&map.location("a.scm", 1, 1, 1, 2));
        let dead = table.intern(&map.location("a.scm", 1, 3, 1, 4));
        let marked = marks(&table, &[live]);
        assert!(table.sweep(&marked).is_none());
        // Compacted away: past the table's end.
        table.location(dead);
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "is free, or past the compacted table")]
    fn a_check_build_refuses_to_renumber_a_freed_entry() {
        let mut table = table();
        let map = map("a b");
        let dead = table.intern(&map.location("a.scm", 1, 1, 1, 2));
        let live = table.intern(&map.location("a.scm", 1, 3, 1, 4));
        let marked = marks(&table, &[live]);
        let renumbering = table.sweep(&marked).expect("the live entry moved");
        renumbering.apply(dead);
    }

    /// The table charges its buffers as external bytes and gives all of
    /// them back once a sweep frees every entry.
    #[test]
    fn the_table_charges_its_entries_and_gives_them_back() {
        let heap = crate::heap::Heap::new();
        let mut table = DocumentTable::new(heap.external_bytes_handle());
        assert_eq!(heap.external_bytes(), 0, "a table with no entry");
        let map = map("a b c");
        let ids: Vec<_> = (0..300)
            .map(|line| table.intern(&map.location("a.scm", line + 1, 1, line + 1, 2)))
            .collect();
        let charged = table.charged();
        assert!(charged >= 300 * size_of::<Entry>(), "{charged}");
        assert_eq!(heap.external_bytes(), charged);
        // The ten kept are the first ten, so nothing moves, and the
        // entries' room is the least there is, 64.
        let marked = marks(&table, &ids[..10]);
        assert!(table.sweep(&marked).is_none());
        assert!(
            table.charged() < charged,
            "{} of {charged}",
            table.charged()
        );
        assert_eq!(table.entries.capacity(), 64);
        // Ten kept from the end of 300 more move to the front: the table
        // holds twenty entries, in room bounded by them, not by the ids the
        // ten had.
        let more: Vec<_> = (300..600)
            .map(|line| table.intern(&map.location("a.scm", line + 1, 1, line + 1, 2)))
            .collect();
        let mut kept = ids[..10].to_vec();
        kept.extend_from_slice(&more[290..]);
        let renumbering = table.sweep(&marks(&table, &kept)).expect("ten moved");
        assert_eq!(renumbering.apply(more[299]).index(), 20);
        assert_eq!(table.entries(), 21);
        assert_eq!(table.entries.capacity(), 64);
        // And the room follows the entries kept: 100 of them, room for 256.
        let many: Vec<_> = (600..900)
            .map(|line| table.intern(&map.location("a.scm", line + 1, 1, line + 1, 2)))
            .collect();
        let kept: Vec<_> = (1..=20)
            .map(SourceId)
            .chain(many[..80].iter().copied())
            .collect();
        assert!(
            table.sweep(&marks(&table, &kept)).is_none(),
            "the kept are first"
        );
        assert_eq!((table.entries(), table.entries.capacity()), (101, 256));
        assert_eq!(heap.external_bytes(), table.charged());
        let marked = marks(&table, &[]);
        assert!(table.sweep(&marked).is_none());
        assert_eq!((table.len(), table.documents()), (0, 0));
        assert_eq!(table.charged(), 0);
        assert_eq!(heap.external_bytes(), 0);
    }

    #[test]
    fn an_entry_is_56_bytes() {
        assert_eq!(size_of::<Entry>(), 56);
    }
}
