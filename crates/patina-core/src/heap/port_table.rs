//! The `PortTable`: the ports a heap's port objects stand for (#785, GC
//! stage 4a; `PRD/GC_PRD.md` §6).
//!
//! A port object's slot holds a [`PortId`], a number, and the heap's table
//! holds the port itself, an `Rc<Port>` per entry. So a slot holds no `Rc`,
//! and what changes about a port, its buffers, its position, its file, lives
//! in Rust beside the heap, not in a payload stage 5 would have to take
//! apart (`SHARED_PAYLOADS`, #711). The table is behind [`HeapTable`] (C3),
//! and every read clones the `Rc` within one access, so a port operation
//! does its I/O after the access has ended.
//!
//! An entry need not hold its port's only `Rc`. The interpreter's `Instance`
//! holds the standard ports, and caches the current ones (#618, #608); a
//! host may keep a port it allocated, or one [`Cx::get_port`] handed it.
//! A dead object's entry lets go of its `Rc`, and the port flushes and
//! closes when that was the last.
//!
//! A dead object's entry leaves the table in one of two ways. A file
//! port's, whose drop flushes and closes a file, is the `FinalRegistry`'s
//! (`heap/finalize.rs`): the collection that finds the object dead queues
//! it, and the drain after the collection takes it out and drops the port,
//! with no heap borrow and no table access open. Every other port's drop
//! does no I/O, and the sweep takes its entry out, after the object arena's
//! sweep, and drops the port once the access has ended.
//!
//! The heap keeps one table for its life, on its `HandleTable`, by `Rc`;
//! teardown carries it over to the handle table it starts.
//!
//! # The registry of tables
//!
//! Exit writes out what a program left in the file ports it never closed,
//! whichever heap's they are (`PRD/GC_PRD.md` §9.6 F1, #786): the CLI ends
//! with `std::process::exit`, which runs no destructor. So every heap's
//! table is registered, once, when the heap is made ([`register`]), in a
//! `thread_local!` list that holds each weakly ([`PORT_TABLES`]), and
//! [`open_output_files`] reaches each live table through its own `Rc`,
//! never through its heap: exit runs from inside a running program as well
//! as after one, and no table access spans Scheme, so it never meets an
//! open one. Opening a file touches no thread-local. Nothing unregisters: a
//! dropped heap's entry stops upgrading, and registration prunes such
//! entries by an amortized rule ([`Tables::register`]), so the list follows
//! the live heaps.
//!
//! The list is per thread, so exit writes out the exiting thread's heaps
//! alone: an interpreter on another thread loses what it left in its open
//! file ports, until the `threaded` build (stage 9) widens the list to the
//! process (decision 3 on #782; an exception to §18.2 rule 1).
//!
//! [`Cx::get_port`]: crate::Cx::get_port

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::table::HeapTable;
use super::{FilePortCharge, GC_CHECK};
use crate::port::Port;
use crate::tagged_value::TaggedValue;

/// What a port object holds: the number of its entry in its heap's
/// `PortTable`. Opaque: outside core nothing can make one or read its
/// number, so no id names an entry it was not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortId(u32);

impl PortId {
    /// The entry's number: its id in the `FinalRegistry`.
    pub(crate) fn number(self) -> u32 {
        self.0
    }

    /// The id the `FinalRegistry` names by `number`.
    pub(crate) fn from_number(number: u32) -> Self {
        PortId(number)
    }
}

/// One heap's ports. See the [module docs](self).
pub(crate) struct PortTable {
    entries: HeapTable<Entries>,
}

#[derive(Default)]
struct Entries {
    /// Each object's port, and the free entries between them.
    ports: Vec<Option<Rc<Port>>>,
    /// The free entries, reused before the list grows.
    free: Vec<u32>,
    /// Whether each entry is the `FinalRegistry`'s: a file port's, which
    /// the drain takes out rather than the sweep.
    finalized: Vec<bool>,
    /// In a check build, the word each entry's object was allocated as,
    /// which a read compares with the word it was given, as the arenas'
    /// generation stamps are compared (#621): an id that does not name its
    /// own object's entry panics where it is read. Empty in a plain release.
    /// Compared by its bits, never traced: a trace through it would keep
    /// every port object alive, in the check lanes alone. A collector that
    /// moves objects or verifies every heap word must account for these.
    owners: Vec<TaggedValue>,
}

impl PortTable {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            entries: HeapTable::new(Entries::default()),
        })
    }

    /// An entry for `port`, which the object about to be allocated names:
    /// the `FinalRegistry`'s if `finalized`, which the caller registers.
    #[inline]
    pub(crate) fn insert(&self, port: Rc<Port>, finalized: bool) -> PortId {
        self.entries.with_mut(|entries| {
            let id = match entries.free.pop() {
                Some(id) => {
                    entries.ports[id as usize] = Some(port);
                    entries.finalized[id as usize] = finalized;
                    id
                }
                None => {
                    let id = u32::try_from(entries.ports.len()).expect("more than 2^32 ports");
                    entries.ports.push(Some(port));
                    entries.finalized.push(finalized);
                    if GC_CHECK {
                        entries.owners.push(TaggedValue::DEAD_SLOT);
                    }
                    id
                }
            };
            PortId(id)
        })
    }

    /// Record `owner`, the word the object naming `id` was allocated as.
    /// Only a check build keeps it.
    pub(crate) fn set_owner(&self, id: PortId, owner: TaggedValue) {
        if GC_CHECK {
            self.entries
                .with_mut(|entries| entries.owners[id.0 as usize] = owner);
        }
    }

    /// The port of `owner`, the object whose slot holds `id`.
    #[inline(always)]
    pub(crate) fn get(&self, id: PortId, owner: TaggedValue) -> Rc<Port> {
        self.entries.with(|entries| {
            match entries.ports.get(id.0 as usize).and_then(Option::as_ref) {
                Some(port) => {
                    if GC_CHECK {
                        entries.check(id, owner);
                    }
                    Rc::clone(port)
                }
                None => free_entry_read(id),
            }
        })
    }

    /// The port of the entry `id`, for a reader that has the object's
    /// payload but not its word (`debug_format`).
    pub(crate) fn port(&self, id: PortId) -> Rc<Port> {
        self.entries.with(|entries| {
            match entries.ports.get(id.0 as usize).and_then(Option::as_ref) {
                Some(port) => Rc::clone(port),
                None => free_entry_read(id),
            }
        })
    }

    /// Take out the entries `ids` names, whose objects the sweep just freed,
    /// and free them, except the `FinalRegistry`'s, which the drain takes
    /// out. Their ports are answered, for the caller to drop once the access
    /// has ended (`HeapTable`'s rule). In a check build the registry's
    /// entries among `ids` are answered too, for the sweep to check that the
    /// collection queued them.
    pub(crate) fn remove(&self, ids: &[PortId]) -> (Vec<Rc<Port>>, Vec<PortId>) {
        if ids.is_empty() {
            return (Vec::new(), Vec::new());
        }
        self.entries.with_mut(|entries| {
            let mut ports = Vec::with_capacity(ids.len());
            let mut finalized = Vec::new();
            for &id in ids {
                if entries.finalized[id.0 as usize] {
                    if GC_CHECK {
                        finalized.push(id);
                    }
                } else {
                    ports.push(entries.free(id));
                }
            }
            (ports, finalized)
        })
    }

    /// Take out the `FinalRegistry`'s entries `ids` names, which it queued,
    /// and free them: the drain's part. Their ports are answered, in the
    /// order of `ids`, for the caller to drop once the access has ended: a
    /// file whose `Rc` was the last flushes and closes as it drops.
    pub(crate) fn take_finalized(&self, ids: &[PortId]) -> Vec<Rc<Port>> {
        if ids.is_empty() {
            return Vec::new();
        }
        self.entries.with_mut(|entries| {
            ids.iter()
                .map(|&id| {
                    if !entries.finalized[id.0 as usize] {
                        not_finalized(id);
                    }
                    entries.finalized[id.0 as usize] = false;
                    entries.free(id)
                })
                .collect()
        })
    }

    /// The charges of the ports whose every `Rc` the entries `ids` hold:
    /// those the drain will close. A port can have more than one object,
    /// each with its entry, so the entries are counted per port. Classification
    /// takes the charges out of the ports, for the caller to drop after the
    /// access and before the sweep settles the account, so L counts no dead
    /// file port (#785). A port another owner keeps, a host or a live
    /// object's entry, keeps its charge, until its last `Rc` drops.
    pub(crate) fn take_last_charges(&self, ids: &[PortId]) -> Vec<FilePortCharge> {
        if ids.is_empty() {
            return Vec::new();
        }
        self.entries.with(|entries| {
            let mut ports: Vec<&Rc<Port>> = ids
                .iter()
                .map(|&id| match entries.ports.get(id.0 as usize) {
                    Some(Some(port)) => port,
                    _ => free_entry_read(id),
                })
                .collect();
            ports.sort_unstable_by_key(|port| Rc::as_ptr(port));
            ports
                .chunk_by(|a, b| Rc::ptr_eq(a, b))
                .filter(|entries| Rc::strong_count(entries[0]) == entries.len())
                .filter_map(|entries| entries[0].take_file_charge())
                .collect()
        })
    }

    /// The entries in use: `(gc-stats)`'s `ports`.
    pub(crate) fn len(&self) -> usize {
        self.entries
            .with(|entries| entries.ports.len() - entries.free.len())
    }

    /// The output ports of the `FinalRegistry`'s entries: every file output
    /// port that was open when its object was made, the dead ones a
    /// collection queued included, until the drain takes them out. Cloned
    /// within one access, for exit to write out after it (#786). A port with
    /// two objects is answered twice.
    fn output_files(&self) -> Vec<Rc<Port>> {
        self.entries.with(|entries| {
            entries
                .ports
                .iter()
                .zip(&entries.finalized)
                .filter_map(|(port, &finalized)| match port {
                    Some(port) if finalized && port.is_output() => Some(Rc::clone(port)),
                    _ => None,
                })
                .collect()
        })
    }
}

thread_local! {
    /// The `PortTable` of every heap made on this thread, held weakly, for
    /// exit to write out the file ports a program left open. See the
    /// [module docs](self#the-registry-of-tables).
    static PORT_TABLES: RefCell<Tables> = const {
        RefCell::new(Tables {
            tables: Vec::new(),
            prune_at: Tables::MIN_PRUNE_AT,
        })
    };
}

/// The heaps' tables, held weakly. A table whose heap has been dropped no
/// longer upgrades: its file ports were written out at teardown (#604).
struct Tables {
    tables: Vec<Weak<PortTable>>,
    /// How many entries to hold before dropping those of tables that are
    /// gone, so the list follows the heaps alive rather than every heap ever
    /// made, at an amortized constant cost per heap.
    prune_at: usize,
}

impl Tables {
    const MIN_PRUNE_AT: usize = 16;

    fn register(&mut self, table: &Rc<PortTable>) {
        if self.tables.len() >= self.prune_at {
            self.tables.retain(|table| table.strong_count() > 0);
            self.prune_at = (2 * self.tables.len()).max(Self::MIN_PRUNE_AT);
        }
        self.tables.push(Rc::downgrade(table));
    }
}

/// Register `table`, a new heap's, for exit: once per heap
/// (`Heap::with_capacity`).
pub(crate) fn register(table: &Rc<PortTable>) {
    PORT_TABLES.with(|tables| tables.borrow_mut().register(table));
}

/// The file output ports of every live heap on this thread, as
/// [`PortTable::output_files`] answers them, for exit to write out
/// (`crate::port::flush_open_output_files`). Each table is read in an access
/// of its own, after the list's borrow has ended.
pub(crate) fn open_output_files() -> Vec<Rc<Port>> {
    let tables: Vec<Rc<PortTable>> = PORT_TABLES.with(|tables| {
        tables
            .borrow()
            .tables
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    });
    tables
        .iter()
        .flat_map(|table| table.output_files())
        .collect()
}

/// The entries of this thread's registry of tables, the dead ones not yet
/// pruned among them: for the tests that hold the registry to the live
/// heaps (#786).
#[cfg(any(test, feature = "test-support"))]
pub fn registered_port_tables() -> usize {
    PORT_TABLES.with(|tables| tables.borrow().tables.len())
}

impl Entries {
    /// Take out the entry `id` and free it, answering its port.
    #[inline]
    fn free(&mut self, id: PortId) -> Rc<Port> {
        let port = match self.ports[id.0 as usize].take() {
            Some(port) => port,
            None => free_entry_read(id),
        };
        self.free.push(id.0);
        if GC_CHECK {
            self.owners[id.0 as usize] = TaggedValue::DEAD_SLOT;
        }
        port
    }

    /// A check build's comparison of `owner` with the word the entry's
    /// object was allocated as. Out of line, so the read inlines only the
    /// branch to it.
    #[cold]
    #[inline(never)]
    fn check(&self, id: PortId, owner: TaggedValue) {
        let recorded = self.owners.get(id.0 as usize).copied();
        if recorded != Some(owner) {
            panic!(
                "port id {} read through {owner:?}, but its entry belongs to {recorded:?}: an \
                 id that does not name its own object's entry (#785)",
                id.0
            );
        }
    }
}

#[cold]
#[inline(never)]
fn not_finalized(id: PortId) -> ! {
    panic!(
        "port id {} was taken out as the finalization registry's, but its entry is not (#785)",
        id.0
    )
}

#[cold]
#[inline(never)]
fn free_entry_read(id: PortId) -> ! {
    panic!(
        "port id {} names a free entry of its heap's port table: a port object outlived its \
         entry (#785)",
        id.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_answers_its_port_and_a_freed_one_is_reused() {
        let table = PortTable::new();
        let first = Port::new_output_string();
        let id = table.insert(Rc::clone(&first), false);
        let owner = TaggedValue::object(7);
        table.set_owner(id, owner);
        assert!(Rc::ptr_eq(&table.get(id, owner), &first));
        assert_eq!(table.len(), 1);

        let (ports, finalized) = table.remove(&[id]);
        assert!(finalized.is_empty());
        assert_eq!(ports.len(), 1);
        assert!(Rc::ptr_eq(&ports[0], &first));
        drop(ports);
        assert_eq!(table.len(), 0);
        assert_eq!(Rc::strong_count(&first), 1, "the table let go of the port");

        let second = Port::new_output_string();
        let again = table.insert(Rc::clone(&second), false);
        assert_eq!(again, id, "a freed entry is reused before the list grows");
        table.set_owner(again, TaggedValue::object(9));
        assert!(Rc::ptr_eq(&table.port(again), &second));
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "does not name its own object's entry")]
    fn a_check_build_refuses_an_id_read_through_another_object() {
        let table = PortTable::new();
        let id = table.insert(Port::new_output_string(), false);
        table.set_owner(id, TaggedValue::object(1));
        let _ = table.get(id, TaggedValue::object(2));
    }

    /// The heap's read of a port goes through the check: a second object
    /// naming the first one's entry, which nothing but a test can make
    /// (`alloc_object` is the heap's own), is refused where it is read.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "does not name its own object's entry")]
    fn a_check_build_refuses_a_port_object_that_names_another_s_entry() {
        use crate::heap::HeapObjectData;

        let mut heap = crate::Heap::new();
        let port = heap.alloc_port(Port::new_output_string());
        let id = match heap.get_object(port) {
            HeapObjectData::Port(id) => *id,
            _ => unreachable!("alloc_port answers a port object"),
        };
        let forged = heap.alloc_object(HeapObjectData::Port(id));
        let _ = heap.get_port(forged);
    }

    /// The sweep leaves the `FinalRegistry`'s entries for the drain, which
    /// takes them out in the order it names them.
    #[test]
    fn the_sweep_leaves_a_finalized_entry_to_the_drain() {
        let table = PortTable::new();
        let file = Port::new_output_string();
        let finalized = table.insert(Rc::clone(&file), true);
        let other = table.insert(Port::new_output_string(), true);
        let plain = table.insert(Port::new_output_string(), false);

        let (ports, skipped) = table.remove(&[finalized, plain]);
        assert_eq!(ports.len(), 1, "only the plain entry is the sweep's");
        if GC_CHECK {
            assert_eq!(skipped, vec![finalized]);
        }
        drop(ports);
        assert_eq!(table.len(), 2);

        let ports = table.take_finalized(&[other, finalized]);
        assert!(Rc::ptr_eq(&ports[1], &file), "in the order of the ids");
        drop(ports);
        assert_eq!(table.len(), 0);
        assert_eq!(Rc::strong_count(&file), 1);
        // A freed entry reused for another port is not the registry's.
        let again = table.insert(Port::new_output_string(), false);
        assert!(table.remove(&[again]).1.is_empty());
    }

    /// Only the ports whose every `Rc` the queued entries hold give up their charge
    /// at classification; a string port has none to give.
    #[test]
    fn a_port_another_owner_keeps_keeps_its_charge() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = crate::Heap::new();
        let kept = Port::open_output_file("/kept", &fs).unwrap();
        let alone = heap.alloc_port(Port::open_output_file("/alone", &fs).unwrap());
        let shared = heap.alloc_port(Rc::clone(&kept));
        let string = heap.alloc_port(Port::new_output_string());
        let ids: Vec<PortId> = [alone, shared, string]
            .into_iter()
            .map(|port| match heap.get_object(port) {
                crate::heap::HeapObjectData::Port(id) => *id,
                _ => unreachable!("alloc_port answers a port object"),
            })
            .collect();
        let charges = heap.handles.ports().take_last_charges(&ids);
        assert_eq!(charges.len(), 1, "only the port nothing else holds");
        assert_eq!(heap.open_file_ports(), 2);
        drop(charges);
        assert_eq!(heap.open_file_ports(), 1);
        assert_eq!(heap.external_bytes(), crate::heap::FILE_PORT_BYTES);
    }

    /// A port with two objects, whose two entries hold its every `Rc`, gives
    /// up its charge once when both are queued, and not when one is.
    #[test]
    fn a_port_whose_entries_hold_every_rc_gives_up_its_charge_once() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = crate::Heap::new();
        let port = Port::open_output_file("/twice", &fs).unwrap();
        let ids: Vec<PortId> = [heap.alloc_port(Rc::clone(&port)), heap.alloc_port(port)]
            .into_iter()
            .map(|object| match heap.get_object(object) {
                crate::heap::HeapObjectData::Port(id) => *id,
                _ => unreachable!("alloc_port answers a port object"),
            })
            .collect();
        assert!(heap.handles.ports().take_last_charges(&ids[..1]).is_empty());
        let charges = heap.handles.ports().take_last_charges(&[ids[1], ids[0]]);
        assert_eq!(charges.len(), 1);
        drop(charges);
        assert_eq!((heap.open_file_ports(), heap.external_bytes()), (0, 0));
    }

    /// The registry follows the live heaps (#786): 300 heaps, each made,
    /// given an open file output port and dropped, with at most two alive at
    /// a time, never leave it more than `MIN_PRUNE_AT` entries, and exit
    /// finds the live heaps' ports alone. On a thread of its own, whose
    /// registry starts empty.
    #[test]
    fn the_registry_prunes_dropped_heaps() {
        std::thread::spawn(|| {
            let fs = crate::vfs::MemoryFs::new();
            let mut previous: Option<crate::Heap> = None;
            for cycle in 0..300 {
                let mut heap = crate::Heap::new();
                let port = Port::open_output_file(&format!("/out-{cycle}"), &fs).unwrap();
                heap.alloc_port(Rc::clone(&port));
                heap.alloc_port(Port::new_output_string());
                let entries = registered_port_tables();
                assert!(
                    entries <= Tables::MIN_PRUNE_AT,
                    "cycle {cycle}: {entries} entries for at most two live heaps"
                );
                let open = open_output_files();
                assert_eq!(open.len(), 1 + usize::from(previous.is_some()));
                assert!(open.iter().any(|open| Rc::ptr_eq(open, &port)));
                drop(open);
                previous = Some(heap);
            }
            drop(previous);
            assert!(open_output_files().is_empty());
        })
        .join()
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "is not (#785)")]
    fn taking_out_a_plain_entry_as_finalized_panics() {
        let table = PortTable::new();
        let id = table.insert(Port::new_output_string(), false);
        drop(table.take_finalized(&[id]));
    }

    #[test]
    #[should_panic(expected = "names a free entry")]
    fn reading_a_freed_entry_panics() {
        let table = PortTable::new();
        let id = table.insert(Port::new_output_string(), false);
        table.set_owner(id, TaggedValue::object(1));
        drop(table.remove(&[id]).0);
        let _ = table.port(id);
    }
}
