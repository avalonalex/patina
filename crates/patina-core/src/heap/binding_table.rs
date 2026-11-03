//! SPIKE 4b.1 (#829), home (c): a per-heap table of binding records and
//! global cells, by id and generation, behind [`HeapTable`] (C3). Not for
//! merge: a prototype that measures the read and the bytes a name.
//!
//! A namespace's slot (`Bindings` in `environment.rs`) holds a record
//! reference encoded in its `TaggedValue` ([`RecordRef::word`]); the record
//! names its cell; the cell holds the value. A read goes
//! record → cell → value through [`BindingTable::read`], a safe accessor
//! over [`HeapTable::with_unguarded`], as `PortTable::get` reads the port
//! table: no heap borrow and no table borrow marked.
//!
//! What the prototype leaves out, by the spike's scope: no entry is ever
//! freed (no marks, no sweep, no prune), so every entry is live; and the
//! table traces every cell's value at every collection, as a root of the
//! handle table's (`HandleTable.bindings`), on both backends.
//!
//! The table's bytes are charged to the heap's external-bytes account (M3)
//! as its chunks are allocated, at their capacity, so `live-bytes` counts
//! them at every collection.

use std::rc::Rc;

use super::account::ExternalBytes;
use super::table::HeapTable;
use super::trace::{Trace, TraceThroughRc};
use crate::heap::gc::SlotVisitor;
use crate::tagged_value::TaggedValue;

/// Entries a chunk holds. A chunk is allocated whole and charged whole, so a
/// table's charge trails its entries by at most one chunk of each kind.
const CHUNK_BITS: u32 = 8;
const CHUNK: usize = 1 << CHUNK_BITS;
const CHUNK_MASK: usize = CHUNK - 1;

/// The low byte of a record word: a special (tag `001`) no constructor
/// makes. `FORWARDED` is `0xF1`; the two differ only in bit 4, so a slot's
/// "is this a marker" test is one mask and one compare for both.
const RECORD_MARK: u8 = 0xE1;
const MARKER_MASK: u8 = 0xEF;

/// A record's id and generation, as a namespace slot and a VM link entry
/// hold it: the id in the high 32 bits, the generation in bits 8–31, and
/// [`RECORD_MARK`] in the low byte. Opaque outside core: nothing outside it
/// can make one but [`RecordRef::EMPTY`], which every read refuses, and the
/// word it encodes is never handed to a program (see `environment.rs`'s
/// slot reads, which all resolve it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordRef(u64);

impl RecordRef {
    /// No record: a link entry not yet filled. Raw 0, the fixnum 0's bits,
    /// which no record word has.
    pub const EMPTY: RecordRef = RecordRef(0);

    /// Whether this names a record (a filled link entry).
    #[inline(always)]
    pub fn is_linked(self) -> bool {
        self.0 != 0
    }

    #[inline(always)]
    fn id(self) -> usize {
        (self.0 >> 32) as usize
    }

    /// The low 32 bits, which a record's entry keeps as its stamp: the
    /// generation and the mark, compared in one go.
    #[inline(always)]
    fn stamp(self) -> u32 {
        self.0 as u32
    }

    fn new(id: u32, generation: u32) -> Self {
        debug_assert!(generation < (1 << 24));
        RecordRef((u64::from(id) << 32) | (u64::from(generation) << 8) | u64::from(RECORD_MARK))
    }

    /// The word a namespace slot holds for this record.
    #[inline(always)]
    pub(crate) fn word(self) -> TaggedValue {
        TaggedValue::from_raw(self.0)
    }

    /// The record a slot's word names, if it names one.
    #[inline(always)]
    pub(crate) fn from_word(word: TaggedValue) -> Option<Self> {
        is_record_word(word).then_some(RecordRef(word.raw()))
    }

    /// The record a slot's word names, which the caller has tested is one.
    #[inline(always)]
    pub(crate) fn from_word_unchecked(word: TaggedValue) -> Self {
        debug_assert!(is_record_word(word));
        RecordRef(word.raw())
    }
}

/// Whether a slot's word is a record reference.
#[inline(always)]
pub(crate) fn is_record_word(word: TaggedValue) -> bool {
    word.raw() as u8 == RECORD_MARK
}

/// Whether a slot's word is a record reference or `FORWARDED`: what every
/// slot read tests before it hands the word out as a value.
#[inline(always)]
pub(crate) fn is_slot_marker(word: TaggedValue) -> bool {
    (word.raw() as u8) & MARKER_MASK == RECORD_MARK
}

/// A binding record: its cell, and its stamp (generation and mark). 8 B.
/// §6's record carries a cell reference, a name symbol and flags; this one
/// carries the cell reference only (the name is the namespace slot's).
#[derive(Clone, Copy)]
struct Record {
    stamp: u32,
    cell: u32,
}

impl Record {
    const FREE: Record = Record { stamp: 0, cell: 0 };
}

/// A global cell: its value, its stamp, and the id of the record that owns
/// it (the record it was made for). 16 B. §6's cell carries a value, a name
/// symbol, a namespace id and flags; this one carries the value, a
/// generation, and the owning record, which stands in for the namespace id:
/// a `define` over an import is told from a `define` over a definition by
/// whether the record owns its cell. No flags (`WATCHED` has no user yet).
#[derive(Clone, Copy)]
struct GlobalCell {
    value: TaggedValue,
    /// Checked against a cell reference from outside the table (4b.6's
    /// `Library.exports`), which the prototype does not make: carried for
    /// its bytes.
    #[allow(dead_code)]
    stamp: u32,
    owner: u32,
}

impl GlobalCell {
    const FREE: GlobalCell = GlobalCell {
        value: TaggedValue::UNSPECIFIED,
        stamp: 0,
        owner: u32::MAX,
    };
}

/// The cell stamp's mark, in the low byte: not a record's.
const CELL_MARK: u32 = 0xC1;

struct Tables {
    records: Vec<Box<[Record; CHUNK]>>,
    record_count: u32,
    cells: Vec<Box<[GlobalCell; CHUNK]>>,
    cell_count: u32,
    /// What the table has charged the account and not given back.
    charged: usize,
}

/// One heap's records and cells. See the [module docs](self).
pub struct BindingTable {
    state: HeapTable<Tables>,
    account: ExternalBytes,
}

impl BindingTable {
    pub(crate) fn new(account: ExternalBytes) -> Rc<Self> {
        Rc::new(BindingTable {
            state: HeapTable::new(Tables {
                records: Vec::new(),
                record_count: 0,
                cells: Vec::new(),
                cell_count: 0,
                charged: 0,
            }),
            account,
        })
    }

    /// The account the table charges.
    pub(crate) fn account(&self) -> ExternalBytes {
        self.account.clone()
    }

    /// The value of the cell `record` names: the read of every global, on
    /// both backends. Refuses a record reference that does not name a live
    /// record of this table, in every build.
    #[inline(always)]
    pub fn read(&self, record: RecordRef) -> TaggedValue {
        // SAFETY: the closure indexes the table and copies a word out of it,
        // or panics: none of it can reach the table.
        unsafe { self.state.with_unguarded(|tables| tables.read(record)) }
    }

    /// The id of the cell `record` names.
    pub(crate) fn cell_of(&self, record: RecordRef) -> u32 {
        self.state.with(|tables| tables.record(record).cell)
    }

    /// Whether `record` owns its cell: a definition of its namespace's own,
    /// rather than an import.
    pub(crate) fn owns_cell(&self, record: RecordRef) -> bool {
        self.state.with(|tables| {
            let cell = tables.record(record).cell;
            tables.cell(cell).owner == record.id() as u32
        })
    }

    /// Store `value` into the cell `record` names, whoever owns it: an
    /// assignment, which reaches an imported binding's owner.
    pub(crate) fn store(&self, record: RecordRef, value: TaggedValue) {
        self.state.with_mut(|tables| {
            let cell = tables.record(record).cell;
            tables.cell_mut(cell).value = value;
        });
    }

    /// A `define` over the binding `record` is: a store into its cell when
    /// the record owns it, and otherwise (a define over an import) a new
    /// cell of the record's own, re-pointed to, which leaves the exporter's
    /// cell as it was.
    pub(crate) fn define_over(&self, record: RecordRef, value: TaggedValue) {
        let account = &self.account;
        self.state.with_mut(|tables| {
            let cell = tables.record(record).cell;
            let id = record.id() as u32;
            if tables.cell(cell).owner == id {
                tables.cell_mut(cell).value = value;
            } else {
                let fresh = tables.alloc_cell(account, value, id);
                tables.record_mut(record).cell = fresh;
            }
        });
    }

    /// A fresh definition: a new cell holding `value`, and a new record that
    /// owns it.
    pub(crate) fn new_binding(&self, value: TaggedValue) -> RecordRef {
        let account = &self.account;
        self.state.with_mut(|tables| {
            let id = tables.record_count;
            let cell = tables.alloc_cell(account, value, id);
            tables.alloc_record(account, cell)
        })
    }

    /// A new record naming `cell`, owned elsewhere: an import.
    pub(crate) fn new_import(&self, cell: u32) -> RecordRef {
        let account = &self.account;
        self.state
            .with_mut(|tables| tables.alloc_record(account, cell))
    }

    /// Re-point `record` to `cell`: an import over a binding.
    pub(crate) fn repoint(&self, record: RecordRef, cell: u32) {
        self.state
            .with_mut(|tables| tables.record_mut(record).cell = cell);
    }

    /// Records and cells in use: (records, cells).
    pub fn counts(&self) -> (u32, u32) {
        self.state
            .with(|tables| (tables.record_count, tables.cell_count))
    }
}

impl Tables {
    #[inline(always)]
    fn record(&self, record: RecordRef) -> &Record {
        let id = record.id();
        match self.records.get(id >> CHUNK_BITS) {
            Some(chunk) => {
                let entry = &chunk[id & CHUNK_MASK];
                if entry.stamp == record.stamp() {
                    entry
                } else {
                    stale_record(record)
                }
            }
            None => stale_record(record),
        }
    }

    fn record_mut(&mut self, record: RecordRef) -> &mut Record {
        let id = record.id();
        match self.records.get_mut(id >> CHUNK_BITS) {
            Some(chunk) if chunk[id & CHUNK_MASK].stamp == record.stamp() => {
                &mut chunk[id & CHUNK_MASK]
            }
            _ => stale_record(record),
        }
    }

    /// The cell a record names. The record's edge is the table's own, made
    /// by it and never stale while the record lives, so it is not checked
    /// against the cell's generation; a cell id from outside the table (4b.6's
    /// `Library.exports`) would be.
    #[inline(always)]
    fn cell(&self, cell: u32) -> &GlobalCell {
        let cell = cell as usize;
        &self.cells[cell >> CHUNK_BITS][cell & CHUNK_MASK]
    }

    fn cell_mut(&mut self, cell: u32) -> &mut GlobalCell {
        let cell = cell as usize;
        &mut self.cells[cell >> CHUNK_BITS][cell & CHUNK_MASK]
    }

    #[inline(always)]
    fn read(&self, record: RecordRef) -> TaggedValue {
        self.cell(self.record(record).cell).value
    }

    fn alloc_record(&mut self, account: &ExternalBytes, cell: u32) -> RecordRef {
        let id = self.record_count;
        let index = id as usize;
        if index >> CHUNK_BITS == self.records.len() {
            let capacity = self.records.capacity();
            self.records.push(Box::new([Record::FREE; CHUNK]));
            self.charge(
                account,
                CHUNK * size_of::<Record>()
                    + (self.records.capacity() - capacity) * size_of::<Box<[Record; CHUNK]>>(),
            );
        }
        let record = RecordRef::new(id, 0);
        self.records[index >> CHUNK_BITS][index & CHUNK_MASK] = Record {
            stamp: record.stamp(),
            cell,
        };
        self.record_count = id.checked_add(1).expect("more than 2^32 binding records");
        record
    }

    fn alloc_cell(&mut self, account: &ExternalBytes, value: TaggedValue, owner: u32) -> u32 {
        let id = self.cell_count;
        let index = id as usize;
        if index >> CHUNK_BITS == self.cells.len() {
            let capacity = self.cells.capacity();
            self.cells.push(Box::new([GlobalCell::FREE; CHUNK]));
            self.charge(
                account,
                CHUNK * size_of::<GlobalCell>()
                    + (self.cells.capacity() - capacity) * size_of::<Box<[GlobalCell; CHUNK]>>(),
            );
        }
        self.cells[index >> CHUNK_BITS][index & CHUNK_MASK] = GlobalCell {
            value,
            stamp: CELL_MARK,
            owner,
        };
        self.cell_count = id.checked_add(1).expect("more than 2^32 global cells");
        id
    }

    fn charge(&mut self, account: &ExternalBytes, bytes: usize) {
        self.charged += bytes;
        account.charge(bytes);
    }
}

impl Drop for BindingTable {
    fn drop(&mut self) {
        let charged = self.state.with(|tables| tables.charged);
        self.account.release(charged);
    }
}

/// A record reference that names no live record of the table.
#[cold]
#[inline(never)]
fn stale_record(record: RecordRef) -> ! {
    panic!("a stale or foreign binding record reference: {record:?} (#829)")
}

/// The table's part of the root set (the prototype's stand-in for marking):
/// every cell's value, live or orphaned, at every collection.
impl Trace for BindingTable {
    fn trace<V: SlotVisitor + ?Sized>(&self, visitor: &mut V) {
        self.state.with(|tables| {
            let mut left = tables.cell_count as usize;
            for chunk in &tables.cells {
                let n = left.min(CHUNK);
                for cell in &chunk[..n] {
                    visitor.pinned(cell.value);
                }
                left -= n;
            }
        });
    }
}

impl TraceThroughRc for BindingTable {}

impl std::fmt::Debug for BindingTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (records, cells) = self.counts();
        f.debug_struct("BindingTable")
            .field("records", &records)
            .field("cells", &cells)
            .finish()
    }
}
