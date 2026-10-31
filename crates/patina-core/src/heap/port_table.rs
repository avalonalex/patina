//! The `PortTable`: the ports a heap's port objects stand for (#785, GC
//! stage 4a; `PRD/GC_PRD.md` §6).
//!
//! A port object's slot holds a [`PortId`], a number, and the heap's table
//! holds the port itself, an `Rc<Port>` per entry. So a slot holds no `Rc`,
//! and what changes about a port, its buffers, its position, its file, lives
//! in Rust beside the heap, not in a payload stage 5 would have to take
//! apart (`SHARED_PAYLOADS`, #711). The table is behind [`HeapTable`] (C3),
//! and every read clones the `Rc` within one access, so a port operation
//! does its I/O after the access has ended. The read, on every port
//! operation, does not mark the table borrowed ([`HeapTable::with_unguarded`],
//! #818), since a clone reaches nothing.
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
//! does no I/O, and the sweep takes its entry out as it starts, while the
//! mark bits are complete: each entry keeps the word of the object that
//! names it, and an entry whose object is unmarked is freed
//! ([`PortTable::take_dead_plain`]), its port dropped once the access has
//! ended. The arena's sweep visits every dead object, and testing each for
//! a port cost 8–9 instructions an object (#818); the pass costs about 13
//! instructions an entry of the table instead. A check build still tests
//! them, to check that the pass took out every dead plain port's entry and
//! that the collection queued every dead file port's.
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
//! entries by an amortized rule ([`WeakList::push`]), so the list follows
//! the live heaps.
//!
//! A heap's table lets go of an open file port only at the drain, after a
//! collection or at the teardown every backend runs as it drops its heap
//! (#604), and the port then closes unless another owner holds it: a host
//! that kept the `Rc` past the port's object, or past its interpreter. Exit
//! cannot reach such a port through a table, and the host cannot write it
//! out either when the program it runs calls `exit`, which ends the process
//! from inside the evaluation. So the drain notes it in the registry too,
//! weakly ([`note_released`]), and exit writes it out with the rest. A port
//! a host opens and never gives an object belongs to no heap, and exit does
//! not write it out; nor one a host keeps past a heap dropped without its
//! teardown, which no backend does.
//!
//! The registry is per thread, so exit writes out the exiting thread's heaps
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
    /// The word each entry's object was allocated as: the sweep's pass
    /// finds a dead plain port's entry by its object's mark bit (#818), and
    /// a check build's read compares it with the word it was given, as the
    /// arenas' generation stamps are compared (#621), so an id that does not
    /// name its own object's entry panics where it is read. `DEAD_SLOT` for
    /// a free entry. Compared by its bits, never traced: a trace through it
    /// would keep every port object alive. A collector that moves objects
    /// or verifies every heap word must account for these.
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
                    entries.owners.push(TaggedValue::DEAD_SLOT);
                    id
                }
            };
            PortId(id)
        })
    }

    /// Record `owner`, the word the object naming `id` was allocated as.
    pub(crate) fn set_owner(&self, id: PortId, owner: TaggedValue) {
        self.entries
            .with_mut(|entries| entries.owners[id.0 as usize] = owner);
    }

    /// The port of `owner`, the object whose slot holds `id`.
    #[inline(always)]
    pub(crate) fn get(&self, id: PortId, owner: TaggedValue) -> Rc<Port> {
        // SAFETY: the closure clones an `Rc` out of the table, and a check
        // build compares two words or panics: none of it can reach the
        // table.
        unsafe {
            self.entries.with_unguarded(|entries| {
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

    /// The sweep's pass, as it starts, while the mark bits are complete:
    /// take out and free every entry that is not the `FinalRegistry`'s and
    /// whose object `live` says is dead, `live` asked the object's slot
    /// index. Their ports are answered, for the caller to drop once the
    /// access has ended (`HeapTable`'s rule). The drain takes out the
    /// registry's.
    pub(crate) fn take_dead_plain(&self, mut live: impl FnMut(u32) -> bool) -> Vec<Rc<Port>> {
        self.entries.with_mut(|entries| {
            let Entries {
                ports,
                free,
                finalized,
                owners,
            } = entries;
            let mut dead = Vec::new();
            // One step an entry, the free ones and the registry's included,
            // with no bounds check: a pass costs the table's length.
            for (index, ((port, &finalized), owner)) in ports
                .iter_mut()
                .zip(finalized.iter())
                .zip(owners.iter_mut())
                .enumerate()
            {
                if !finalized && port.is_some() && !live(owner.heap_index()) {
                    dead.extend(port.take());
                    free.push(index as u32);
                    *owner = TaggedValue::DEAD_SLOT;
                }
            }
            dead
        })
    }

    /// A check build's part of the sweep: of `ids`, the entries of the port
    /// objects it freed, a plain port's entry is already free, which the
    /// pass that starts the sweep took out, and the registry's entries are
    /// answered, for the sweep to check that the collection queued them.
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check", test)),
        expect(
            dead_code,
            reason = "a check build's part of the sweep, and its control's"
        )
    )]
    pub(crate) fn check_swept(&self, ids: &[PortId]) -> Vec<PortId> {
        self.entries.with(|entries| {
            let mut finalized = Vec::new();
            for &id in ids {
                if entries.finalized[id.0 as usize] {
                    finalized.push(id);
                } else if entries.ports[id.0 as usize].is_some() {
                    dead_entry_kept(id);
                }
            }
            finalized
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
    /// The `PortTable` of every heap made on this thread, and the file
    /// output ports a table let go of while another owner kept them, each
    /// held weakly, for exit to write out the file ports left open. See the
    /// [module docs](self#the-registry-of-tables).
    static PORT_TABLES: RefCell<Tables> = const {
        RefCell::new(Tables {
            tables: WeakList::new(),
            released: WeakList::new(),
        })
    };
}

/// The heaps' tables, and the ports released from them, held weakly. A
/// table whose heap has been dropped no longer upgrades: its file ports were
/// written out at teardown (#604).
struct Tables {
    tables: WeakList<PortTable>,
    /// The open file output ports whose entries the drain took out, at a
    /// collection or at teardown, while another owner, a host, still held
    /// them ([`note_released`]): no table reaches them any more, and exit
    /// still writes them out.
    released: WeakList<Port>,
}

/// A list of weak references that drops those that no longer upgrade, and
/// the repeats of one that does, once it reaches `prune_at` entries, and
/// then holds twice the entries left, so it follows what is alive rather
/// than everything ever added, at an amortized constant cost per entry. A
/// port is pushed once for each drain that releases it while a host holds
/// it, so a host that gives one port to many heaps pushes it many times.
struct WeakList<T> {
    items: Vec<Weak<T>>,
    prune_at: usize,
}

impl<T> WeakList<T> {
    const MIN_PRUNE_AT: usize = 16;

    const fn new() -> Self {
        Self {
            items: Vec::new(),
            prune_at: Self::MIN_PRUNE_AT,
        }
    }

    fn push(&mut self, item: &Rc<T>) {
        if self.items.len() >= self.prune_at {
            self.items.retain(|item| item.strong_count() > 0);
            self.items.sort_unstable_by_key(Weak::as_ptr);
            self.items.dedup_by(|a, b| Weak::ptr_eq(a, b));
            self.prune_at = (2 * self.items.len()).max(Self::MIN_PRUNE_AT);
        }
        self.items.push(Rc::downgrade(item));
    }

    fn upgrade(&self) -> Vec<Rc<T>> {
        self.items.iter().filter_map(Weak::upgrade).collect()
    }
}

/// Register `table`, a new heap's, for exit: once per heap
/// (`Heap::with_capacity`).
pub(crate) fn register(table: &Rc<PortTable>) {
    PORT_TABLES.with(|tables| tables.borrow_mut().tables.push(table));
}

/// Note each output port of `ports`, which the drain just took out of a
/// table, that an owner besides `ports` still holds: a host's `Rc`, kept
/// past the port object it was given, or past its interpreter's teardown.
/// No table reaches such a port, so exit finds it here. A port can have two
/// objects, each with its entry, so the `Rc`s are counted per port, as
/// [`PortTable::take_last_charges`] counts them.
pub(crate) fn note_released(ports: &[Rc<Port>]) {
    let mut ports: Vec<&Rc<Port>> = ports.iter().filter(|port| port.is_output()).collect();
    if ports.is_empty() {
        return;
    }
    ports.sort_unstable_by_key(|port| Rc::as_ptr(port));
    let held: Vec<&Rc<Port>> = ports
        .chunk_by(|a, b| Rc::ptr_eq(a, b))
        .filter(|entries| Rc::strong_count(entries[0]) > entries.len())
        .map(|entries| entries[0])
        .collect();
    if held.is_empty() {
        return;
    }
    // A heap torn down from a thread-local's destructor, at the thread's
    // exit, may find the registry destroyed already, and a panic there
    // would abort the process. No exit can write the port out then, and it
    // flushes when its last owner drops it.
    let _ = PORT_TABLES.try_with(|tables| {
        let mut tables = tables.borrow_mut();
        held.into_iter().for_each(|port| tables.released.push(port));
    });
}

/// The file output ports of every live heap on this thread, as
/// [`PortTable::output_files`] answers them, and those the heaps released
/// that a host still holds, for exit to write out
/// (`crate::port::flush_open_output_files`). Each table is read in an access
/// of its own, after the list's borrow has ended.
pub(crate) fn open_output_files() -> Vec<Rc<Port>> {
    let (tables, mut released) = PORT_TABLES.with(|tables| {
        let tables = tables.borrow();
        (tables.tables.upgrade(), tables.released.upgrade())
    });
    let mut ports: Vec<Rc<Port>> = tables
        .iter()
        .flat_map(|table| table.output_files())
        .collect();
    ports.append(&mut released);
    ports
}

/// The entries of this thread's registry of tables, the dead ones not yet
/// pruned among them: for the tests that hold the registry to the live
/// heaps (#786).
#[cfg(any(test, feature = "test-support"))]
pub fn registered_port_tables() -> usize {
    PORT_TABLES.with(|tables| tables.borrow().tables.items.len())
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
        self.owners[id.0 as usize] = TaggedValue::DEAD_SLOT;
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

/// A check build's failure: the sweep freed a plain port's object whose
/// entry the pass that starts the sweep left in the table.
#[cfg_attr(
    not(any(debug_assertions, feature = "gc-check", test)),
    expect(
        dead_code,
        reason = "a check build's part of the sweep, and its control's"
    )
)]
#[cold]
#[inline(never)]
fn dead_entry_kept(id: PortId) -> ! {
    panic!(
        "port id {}: its object was swept, but its entry is still in the table: the sweep's \
         pass missed a dead port (#818)",
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

        let ports = table.take_dead_plain(|_| false);
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
        for (id, object) in [(finalized, 1), (other, 2), (plain, 3)] {
            table.set_owner(id, TaggedValue::object(object));
        }

        let ports = table.take_dead_plain(|_| false);
        assert_eq!(ports.len(), 1, "only the plain entry is the sweep's");
        assert_eq!(table.check_swept(&[finalized, plain]), vec![finalized]);
        drop(ports);
        assert_eq!(table.len(), 2);

        let ports = table.take_finalized(&[other, finalized]);
        assert!(Rc::ptr_eq(&ports[1], &file), "in the order of the ids");
        drop(ports);
        assert_eq!(table.len(), 0);
        assert_eq!(Rc::strong_count(&file), 1);
        // A freed entry reused for another port is not the registry's.
        let again = table.insert(Port::new_output_string(), false);
        table.set_owner(again, TaggedValue::object(4));
        assert_eq!(table.take_dead_plain(|_| false).len(), 1);
    }

    /// The sweep's pass takes out the plain entries whose objects are dead,
    /// asked by each object's slot index, and leaves the live ones and the
    /// registry's (#818).
    #[test]
    fn the_sweep_s_pass_takes_out_only_the_dead_plain_entries() {
        let table = PortTable::new();
        let ids: Vec<(PortId, u32, bool)> = [(5, false), (6, false), (7, false), (8, true)]
            .into_iter()
            .map(|(object, finalized)| {
                let id = table.insert(Port::new_output_string(), finalized);
                table.set_owner(id, TaggedValue::object(object));
                (id, object, finalized)
            })
            .collect();
        let mut asked = Vec::new();
        let dead = table.take_dead_plain(|object| {
            asked.push(object);
            object == 6
        });
        assert_eq!(dead.len(), 2, "the entries of objects 5 and 7");
        assert_eq!(
            asked,
            vec![5, 6, 7],
            "the registry's entry is not asked about"
        );
        drop(dead);
        assert_eq!(table.len(), 2);
        assert!(table.port(ids[1].0).is_output(), "object 6's entry stays");
        assert!(table.take_dead_plain(|object| object == 6).is_empty());
    }

    /// A check build's sweep refuses a dead plain port whose entry the pass
    /// left in the table, which only a pass that missed it would leave.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "the sweep's pass missed a dead port")]
    fn a_check_build_refuses_a_dead_port_the_pass_left() {
        let table = PortTable::new();
        let id = table.insert(Port::new_output_string(), false);
        table.set_owner(id, TaggedValue::object(1));
        let _ = table.check_swept(&[id]);
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
                    entries <= WeakList::<PortTable>::MIN_PRUNE_AT,
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

    /// The ports the drain notes for exit follow the live ports (#786): a
    /// host that keeps one open file port and gives it to 300 heaps in turn,
    /// each torn down, has it noted 300 times, and the list still never
    /// holds more than `MIN_PRUNE_AT` entries, the repeats pruned with the
    /// dead. On a thread of its own, whose registry starts empty.
    #[test]
    fn the_released_ports_are_pruned_of_repeats() {
        std::thread::spawn(|| {
            let fs = crate::vfs::MemoryFs::new();
            let kept = Port::open_output_file("/kept", &fs).unwrap();
            let released = || PORT_TABLES.with(|tables| tables.borrow().released.items.len());
            for cycle in 0..300 {
                let heap = crate::heap::new_shared_heap();
                heap.borrow_mut().alloc_port(Rc::clone(&kept));
                crate::heap::teardown(&heap);
                let entries = released();
                assert!(
                    (1..=WeakList::<Port>::MIN_PRUNE_AT).contains(&entries),
                    "cycle {cycle}: {entries} entries for one kept port"
                );
            }
            assert!(
                open_output_files()
                    .iter()
                    .all(|port| Rc::ptr_eq(port, &kept))
            );
            drop(kept);
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
        drop(table.take_dead_plain(|_| false));
        let _ = table.port(id);
    }
}
