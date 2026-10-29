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
//! The sweep takes a dead object's entry out, after the object arena's
//! sweep, and drops the ports once the access has ended: where the slot's
//! tombstone dropped them before the table existed, so a dead port is still
//! written out and closed by the collection that finds it.
//!
//! The heap keeps one table for its life, on its `HandleTable`, by `Rc`, so
//! that #786's registry of open-file tables can hold it weakly; teardown
//! carries it over to the handle table it starts.
//!
//! [`Cx::get_port`]: crate::Cx::get_port

use std::rc::Rc;

use super::GC_CHECK;
use super::table::HeapTable;
use crate::port::Port;
use crate::tagged_value::TaggedValue;

/// What a port object holds: the number of its entry in its heap's
/// `PortTable`. Opaque: outside core nothing can make one or read its
/// number, so no id names an entry it was not given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortId(u32);

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

    /// An entry for `port`, which the object about to be allocated names.
    pub(crate) fn insert(&self, port: Rc<Port>) -> PortId {
        self.entries.with_mut(|entries| {
            let id = match entries.free.pop() {
                Some(id) => {
                    entries.ports[id as usize] = Some(port);
                    id
                }
                None => {
                    let id = u32::try_from(entries.ports.len()).expect("more than 2^32 ports");
                    entries.ports.push(Some(port));
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
    #[inline]
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
    /// and free them. Their ports are answered, for the caller to drop once
    /// the access has ended (`HeapTable`'s rule): a file port whose `Rc` was
    /// the last flushes and closes as it drops.
    pub(crate) fn remove(&self, ids: &[PortId]) -> Vec<Rc<Port>> {
        if ids.is_empty() {
            return Vec::new();
        }
        self.entries.with_mut(|entries| {
            let mut ports = Vec::with_capacity(ids.len());
            for &PortId(id) in ids {
                match entries.ports[id as usize].take() {
                    Some(port) => ports.push(port),
                    None => free_entry_read(PortId(id)),
                }
                entries.free.push(id);
                if GC_CHECK {
                    entries.owners[id as usize] = TaggedValue::DEAD_SLOT;
                }
            }
            ports
        })
    }

    /// The entries in use: `(gc-stats)`'s `ports`.
    pub(crate) fn len(&self) -> usize {
        self.entries
            .with(|entries| entries.ports.len() - entries.free.len())
    }
}

impl Entries {
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
        let id = table.insert(Rc::clone(&first));
        let owner = TaggedValue::object(7);
        table.set_owner(id, owner);
        assert!(Rc::ptr_eq(&table.get(id, owner), &first));
        assert_eq!(table.len(), 1);

        let ports = table.remove(&[id]);
        assert_eq!(ports.len(), 1);
        assert!(Rc::ptr_eq(&ports[0], &first));
        drop(ports);
        assert_eq!(table.len(), 0);
        assert_eq!(Rc::strong_count(&first), 1, "the table let go of the port");

        let second = Port::new_output_string();
        let again = table.insert(Rc::clone(&second));
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
        let id = table.insert(Port::new_output_string());
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

    #[test]
    #[should_panic(expected = "names a free entry")]
    fn reading_a_freed_entry_panics() {
        let table = PortTable::new();
        let id = table.insert(Port::new_output_string());
        table.set_owner(id, TaggedValue::object(1));
        drop(table.remove(&[id]));
        let _ = table.port(id);
    }
}
