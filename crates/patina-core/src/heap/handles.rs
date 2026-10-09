//! Handles: how a host keeps a value from one evaluation to the next (#605).
//!
//! A `TaggedValue` names a slot, and nothing roots it once it leaves the
//! interpreter: a collection during a later evaluation frees the slot, and
//! the kept value then reads whatever comes to occupy it. An [`Owned`]
//! handle roots its value until the handle is dropped. Each heap keeps one
//! [`HandleTable`], which every collection traces as a root, on both
//! backends: `GcVisitor::new` visits its values beside the intern tables.
//!
//! Each entry belongs to exactly one handle. [`Clone`] makes a new entry,
//! and a drop frees the handle's own, so no two handles share an index and a
//! handle needs no generation to tell its entry from a later one. A handle
//! that is forgotten (`mem::forget`) keeps its value for the heap's life, as
//! a forgotten `Rc` keeps its allocation.
//!
//! A handle holds its table weakly, so it keeps nothing alive. Teardown
//! (`Heap::teardown`, #604) drops the table, so a handle that outlives its
//! interpreter holds a dead reference: dropping it does nothing, and the heap
//! refuses to read it, whatever becomes of the heap afterwards.

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use super::gc::GcVisitor;
use crate::tagged_value::TaggedValue;

/// One heap's handles: the values its hosts keep, each rooted until its
/// handle is dropped.
pub struct HandleTable {
    /// The heap's number in the collector's log (`Telemetry::heap_id`), for
    /// the message when a handle meets another heap.
    heap_id: u64,
    entries: RefCell<Entries>,
}

struct Entries {
    /// Each entry's value; `None` for a free entry.
    values: Vec<Option<TaggedValue>>,
    /// The free entries, reused before the table grows.
    free: Vec<u32>,
}

impl HandleTable {
    /// An empty table for the heap numbered `heap_id`.
    pub(crate) fn new(heap_id: u64) -> Rc<Self> {
        Rc::new(HandleTable {
            heap_id,
            entries: RefCell::new(Entries {
                values: Vec::new(),
                free: Vec::new(),
            }),
        })
    }

    /// A new handle on `value`.
    pub(crate) fn hold(self: &Rc<Self>, value: TaggedValue) -> Owned {
        let mut entries = self.entries.borrow_mut();
        let index = match entries.free.pop() {
            Some(index) => {
                entries.values[index as usize] = Some(value);
                index
            }
            None => {
                let index = u32::try_from(entries.values.len())
                    .expect("fewer than 2^32 handles alive at once");
                entries.values.push(Some(value));
                index
            }
        };
        Owned {
            table: Rc::downgrade(self),
            index,
            heap_id: self.heap_id,
        }
    }

    /// The value in entry `index`, which a live handle owns.
    fn value(&self, index: u32) -> TaggedValue {
        self.entries.borrow().values[index as usize].expect("a live handle's entry holds its value")
    }

    /// Put `value` in entry `index`, which a live handle owns.
    fn set(&self, index: u32, value: TaggedValue) {
        let mut entries = self.entries.borrow_mut();
        let entry = &mut entries.values[index as usize];
        assert!(entry.is_some(), "a live handle's entry holds its value");
        *entry = Some(value);
    }

    /// Free entry `index`, whose handle is being dropped.
    fn release(&self, index: u32) {
        let mut entries = self.entries.borrow_mut();
        let freed = entries.values[index as usize].take();
        debug_assert!(freed.is_some(), "a handle's entry freed twice");
        entries.free.push(index);
    }

    /// How many handles are alive.
    pub fn len(&self) -> usize {
        let entries = self.entries.borrow();
        entries.values.len() - entries.free.len()
    }

    /// Whether no handle is alive.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Visit every value a handle holds: the table's part of the root set.
    /// Visited, not only marked, so that marking traces what each value
    /// reaches.
    pub(crate) fn trace_handles(&self, visitor: &mut GcVisitor<'_>) {
        let HandleTable {
            // A number, for a message; no value.
            heap_id: _,
            entries,
        } = self;
        let entries = entries.borrow();
        let Entries {
            values,
            // Indices of the entries with no value.
            free: _,
        } = &*entries;
        for &value in values.iter().flatten() {
            visitor.visit(value);
        }
    }

    /// Check that `handle` is one of this table's, and answer its entry.
    fn entry_of(self: &Rc<Self>, handle: &Owned) -> u32 {
        if !std::ptr::eq(handle.table.as_ptr(), Rc::as_ptr(self)) {
            if handle.heap_id == self.heap_id {
                panic!(
                    "a handle read after its interpreter was dropped: teardown freed the value \
                     it held (#604)"
                );
            }
            panic!(
                "a handle from heap {} used with heap {}: a handle is read only through the \
                 interpreter that made it",
                handle.heap_id, self.heap_id
            );
        }
        handle.index
    }

    /// The value `handle` holds. Panics if it is not one of this table's.
    pub(crate) fn get(self: &Rc<Self>, handle: &Owned) -> TaggedValue {
        let index = self.entry_of(handle);
        self.value(index)
    }

    /// Make `handle` hold `value`. Panics if it is not one of this table's.
    pub(crate) fn replace(self: &Rc<Self>, handle: &Owned, value: TaggedValue) {
        let index = self.entry_of(handle);
        self.set(index, value);
    }
}

impl fmt::Debug for HandleTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandleTable")
            .field("heap", &self.heap_id)
            .field("handles", &self.len())
            .finish()
    }
}

/// A value a host keeps: rooted until the handle is dropped (#605).
///
/// An interpreter's `eval_*_owned` methods answer one, and the interpreter
/// that made it reads it; it panics on a handle from another interpreter.
/// The handle keeps neither the heap nor its interpreter alive, and dropping
/// it after the interpreter does nothing. See `heap/handles.rs`.
pub struct Owned {
    table: Weak<HandleTable>,
    index: u32,
    heap_id: u64,
}

impl Clone for Owned {
    /// Another handle on the same value, with an entry of its own.
    fn clone(&self) -> Self {
        match self.table.upgrade() {
            Some(table) => {
                let value = table.value(self.index);
                table.hold(value)
            }
            // The table went with its interpreter: the copy is as dead.
            None => Owned {
                table: Weak::clone(&self.table),
                index: self.index,
                heap_id: self.heap_id,
            },
        }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(table) = self.table.upgrade() {
            table.release(self.index);
        }
    }
}

impl fmt::Debug for Owned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned")
            .field("heap", &self.heap_id)
            .field("entry", &self.index)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::super::sentinels::Sentinels;
    use super::super::trace_sentinels::collect_only;
    use super::*;
    use crate::heap::new_shared_heap;

    /// The table's sentinel test (#623): built by a literal, with a fresh
    /// value in each entry, a free entry between them, and no other root.
    #[test]
    fn handle_table_fields() {
        let shared = new_shared_heap();
        let mut heap = shared.borrow_mut();
        let mut s = Sentinels::new(&mut heap);
        let h = &mut *heap;
        let first = s.pair(h, "HandleTable.entries.values[0]");
        let second = s.vector(h, "HandleTable.entries.values[2]");
        let third = s.object(h, "HandleTable.entries.values[3]");
        heap.handles = Rc::new(HandleTable {
            heap_id: 0,
            entries: RefCell::new(Entries {
                values: vec![Some(first), None, Some(second), Some(third)],
                free: vec![1],
            }),
        });
        collect_only(&mut heap, |_| {});
        s.assert_survived(&heap);
    }

    #[test]
    fn a_handle_roots_its_value_until_it_is_dropped() {
        let shared = new_shared_heap();
        let mut heap = shared.borrow_mut();
        let value = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let handle = heap.hold(value);
        let copy = handle.clone();
        collect_only(&mut heap, |_| {});
        assert_eq!(heap.car(heap.held(&handle)), TaggedValue::fixnum(1));
        drop(handle);
        collect_only(&mut heap, |_| {});
        assert_eq!(heap.car(heap.held(&copy)), TaggedValue::fixnum(1));
        assert_eq!(heap.handles.len(), 1);
        drop(copy);
        assert!(heap.handles.is_empty());
        collect_only(&mut heap, |_| {});
        assert!(heap.free_pairs.contains(&value.heap_index()));
    }

    #[test]
    fn a_dropped_handle_frees_its_entry_for_the_next() {
        let shared = new_shared_heap();
        let heap = shared.borrow();
        let first = heap.hold(TaggedValue::fixnum(1));
        let second = heap.hold(TaggedValue::fixnum(2));
        drop(first);
        let third = heap.hold(TaggedValue::fixnum(3));
        assert_eq!(third.index, 0, "the freed entry is reused");
        assert_eq!(heap.held(&second), TaggedValue::fixnum(2));
        assert_eq!(heap.held(&third), TaggedValue::fixnum(3));
        heap.set_held(&third, TaggedValue::fixnum(4));
        assert_eq!(heap.held(&third), TaggedValue::fixnum(4));
    }

    #[test]
    #[should_panic(expected = "used with heap")]
    fn a_handle_from_another_heap_is_refused() {
        let first = new_shared_heap();
        let second = new_shared_heap();
        let handle = first.borrow().hold(TaggedValue::fixnum(1));
        second.borrow().held(&handle);
    }

    #[test]
    fn a_handle_outliving_its_heap_holds_nothing() {
        let shared = new_shared_heap();
        let handle = shared.borrow().hold(TaggedValue::fixnum(1));
        let copy = handle.clone();
        drop(shared);
        drop(handle);
        let again = copy.clone();
        drop(copy);
        drop(again);
    }

    #[test]
    fn teardown_lets_go_of_every_handle() {
        let shared = new_shared_heap();
        let handle = shared.borrow().hold(TaggedValue::fixnum(1));
        shared.borrow_mut().teardown();
        assert!(shared.borrow().handles.is_empty());
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shared.borrow().held(&handle);
        }));
        let payload = read.expect_err("a handle read after teardown");
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .unwrap_or_default();
        assert!(
            message.contains("after its interpreter was dropped"),
            "{message}"
        );
        drop(handle);
    }
}
