//! Handles: how a host keeps a value or an environment from one evaluation
//! to the next (#605, #620).
//!
//! A `TaggedValue` names a slot, and nothing roots it once it leaves the
//! interpreter: a collection during a later evaluation frees the slot, and
//! the kept value then reads whatever comes to occupy it. An [`Owned`]
//! handle roots its value until the handle is dropped. Each heap keeps one
//! [`HandleTable`], which every collection traces as a root, on both
//! backends: `GcVisitor::new` visits its entries beside the intern tables.
//!
//! An environment a host builds is reachable from no root either, so its
//! bindings went the same way (#620). An [`OwnedEnvironment`] handle roots
//! one: the table holds the environment, and a collection traces it with
//! `GcVisitor::visit_env`, its bindings, its parent chain and its alias
//! targets. A host that holds an environment by its own `Rc` instead, as the
//! legacy pipeline's callers do, can have the table trace it for as long as
//! anything holds it ([`HandleTable::track`]); the table keeps it weakly.
//!
//! Each entry belongs to exactly one handle. [`Clone`] makes a new entry,
//! and a drop frees the handle's own, so no two handles share an index and a
//! handle needs no generation to tell its entry from a later one. A handle
//! that is forgotten (`mem::forget`) keeps its entry for the heap's life, as
//! a forgotten `Rc` keeps its allocation.
//!
//! A handle holds its table weakly, so it keeps nothing alive. Teardown
//! (`Heap::teardown`, #604) drops the table, so a handle that outlives its
//! interpreter holds a dead reference: dropping it does nothing, and the heap
//! refuses to read it, whatever becomes of the heap afterwards. Dropping the
//! table also lets go of the environments it holds, each of which holds the
//! heap: heap, table, environment and heap again are a cycle, and teardown
//! is what breaks it.

use std::fmt;
use std::rc::{Rc, Weak};

use super::gc::RootSet;
use super::table::HeapTable;
use crate::Trace;
use crate::environment::Environment;
use crate::instance::CurrentPort;
use crate::tagged_value::TaggedValue;

/// One heap's handles: the values and environments its hosts keep, each
/// rooted until its handle is dropped, and the environments it traces for
/// as long as their hosts hold them. With them, the heap's three standard
/// port parameters (#608), which the heap keeps for the interpreter.
///
/// Its trace (derived, #686) is the table's part of the root set, which
/// `GcVisitor::new` visits beside the intern tables on either backend: every
/// value a handle holds, visited rather than only marked so that marking
/// traces what each reaches, and every tracked environment something still
/// holds. Pinned by the sentinel test `handle_table_fields`.
#[derive(Trace)]
pub struct HandleTable {
    /// The heap's number in the collector's log (`Telemetry::heap_id`), for
    /// the message when a handle meets another heap.
    #[trace(skip, reason = "a number, for a message")]
    heap_id: u64,
    entries: HeapTable<Entries>,
    /// The root providers registered with the heap (`Heap::root_set`, #682),
    /// kept with the other roots hosts keep. Not a field of `Heap`: one more
    /// there moved the allocator's hot fields, and cost the VM's
    /// continuation benchmarks 4–5% in cycles with the same instructions.
    #[trace(
        skip,
        reason = "providers, which the collector traces itself, each through its own trace (#682)"
    )]
    root_set: Rc<RootSet>,
    /// The parameter objects `current-input-port`, `current-output-port` and
    /// `current-error-port` are bound to, indexed by [`CurrentPort`] (#608):
    /// made once per heap, on `(patina internal io)`'s first build
    /// (`Heap::standard_port_parameters`), and rooted here for the heap's
    /// life, so a second build binds the same three. `None` on a heap no
    /// backend set up. Here and not on `Heap`, for `root_set`'s reason.
    standard_ports: HeapTable<Option<[TaggedValue; 3]>>,
}

#[derive(Trace)]
struct Entries {
    /// Each handle's entry, and the free ones between them.
    list: Vec<Entry>,
    /// The free entries, reused before the list grows.
    #[trace(skip, reason = "indices of the free entries, which hold nothing")]
    free: Vec<u32>,
    /// Environments hosts hold by their own `Rc`, traced while anything
    /// holds them ([`HandleTable::track`]), at most once each.
    tracked: Vec<Weak<Environment>>,
}

/// What an entry of the table holds.
#[derive(Trace)]
enum Entry {
    /// Nothing: the entry is on the free list.
    Free,
    /// An [`Owned`] handle's value.
    Value(TaggedValue),
    /// An [`OwnedEnvironment`] handle's environment.
    Environment(Rc<Environment>),
}

impl HandleTable {
    /// An empty table for the heap numbered `heap_id`.
    pub(crate) fn new(heap_id: u64) -> Rc<Self> {
        Rc::new(HandleTable {
            heap_id,
            entries: HeapTable::new(Entries {
                list: Vec::new(),
                free: Vec::new(),
                tracked: Vec::new(),
            }),
            root_set: Rc::default(),
            standard_ports: HeapTable::new(None),
        })
    }

    /// The root providers registered with the heap.
    pub(crate) fn root_set(&self) -> &Rc<RootSet> {
        &self.root_set
    }

    /// The heap's standard port parameters, once made.
    pub(crate) fn standard_ports(&self) -> Option<[TaggedValue; 3]> {
        self.standard_ports.with(|ports| *ports)
    }

    /// Record the heap's standard port parameters, made once.
    pub(crate) fn set_standard_ports(&self, parameters: [TaggedValue; 3]) {
        self.standard_ports.with_mut(|ports| {
            debug_assert!(ports.is_none(), "the standard port parameters made twice");
            *ports = Some(parameters);
        });
    }

    /// Which standard port parameter `word` is, if it is one. Compared by
    /// the whole word, generation stamp and all: the three are rooted for
    /// the heap's life, so no other object ever takes their slots.
    #[inline]
    pub(crate) fn standard_port_parameter(&self, word: TaggedValue) -> Option<CurrentPort> {
        self.standard_ports.with(|ports| {
            let ports = ports.as_ref()?;
            [CurrentPort::Input, CurrentPort::Output, CurrentPort::Error]
                .into_iter()
                .find(|&which| ports[which as usize] == word)
        })
    }

    /// A claim on a new entry holding `entry`.
    fn claim(self: &Rc<Self>, entry: Entry) -> Claim {
        let index = self.entries.with_mut(|entries| match entries.free.pop() {
            Some(index) => {
                entries.list[index as usize] = entry;
                index
            }
            None => {
                let index = u32::try_from(entries.list.len())
                    .expect("fewer than 2^32 handles alive at once");
                entries.list.push(entry);
                index
            }
        });
        Claim {
            table: Rc::downgrade(self),
            index,
            heap_id: self.heap_id,
        }
    }

    /// A new handle on `value`.
    pub(crate) fn hold(self: &Rc<Self>, value: TaggedValue) -> Owned {
        Owned(self.claim(Entry::Value(value)))
    }

    /// A new handle on `env`, an environment of this table's heap.
    pub(crate) fn hold_environment(self: &Rc<Self>, env: Rc<Environment>) -> OwnedEnvironment {
        OwnedEnvironment(self.claim(Entry::Environment(env)))
    }

    /// A new claim on what `claim`'s entry holds, with an entry of its own.
    fn duplicate(self: &Rc<Self>, claim: &Claim) -> Claim {
        let entry = self
            .entries
            .with(|entries| match &entries.list[claim.index as usize] {
                Entry::Value(value) => Entry::Value(*value),
                Entry::Environment(env) => Entry::Environment(Rc::clone(env)),
                Entry::Free => unreachable!("a live handle's entry is not free"),
            });
        self.claim(entry)
    }

    /// Free entry `index`, whose handle is being dropped.
    fn release(&self, index: u32) {
        let freed = self.entries.with_mut(|entries| {
            let freed = std::mem::replace(&mut entries.list[index as usize], Entry::Free);
            debug_assert!(
                !matches!(freed, Entry::Free),
                "a handle's entry freed twice"
            );
            entries.free.push(index);
            freed
        });
        // An environment may go with its entry, and let go of the heap, so it
        // is dropped after the table's access ends.
        drop(freed);
    }

    /// Trace `env`, an environment of this table's heap, at every
    /// collection for as long as anything else holds it.
    pub(crate) fn track(&self, env: &Rc<Environment>) {
        let address = Rc::as_ptr(env);
        self.entries.with_mut(|entries| {
            let tracked = &mut entries.tracked;
            if !tracked
                .iter()
                .any(|held| std::ptr::eq(held.as_ptr(), address))
            {
                tracked.push(Rc::downgrade(env));
            }
        });
    }

    /// How many handles are alive.
    pub fn len(&self) -> usize {
        self.entries
            .with(|entries| entries.list.len() - entries.free.len())
    }

    /// Whether no handle is alive.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many environments the table tracks, as of the last collection,
    /// which forgets the ones nothing holds any more.
    pub fn tracked(&self) -> usize {
        self.entries.with(|entries| entries.tracked.len())
    }

    /// Forget each tracked environment nothing holds any more, as a
    /// collection begins: what the table tracks for a host is traced for as
    /// long as the host holds it, and no longer.
    pub(crate) fn forget_dropped_environments(&self) {
        self.entries
            .with_mut(|entries| entries.tracked.retain(|held| held.strong_count() > 0));
    }

    /// Check that `claim` is one of this table's, and answer its entry.
    fn entry_of(self: &Rc<Self>, claim: &Claim) -> u32 {
        if !std::ptr::eq(claim.table.as_ptr(), Rc::as_ptr(self)) {
            if claim.heap_id == self.heap_id {
                panic!(
                    "a handle read after its interpreter was dropped: teardown freed what it \
                     held (#604)"
                );
            }
            panic!(
                "a handle from heap {} used with heap {}: a handle is read only through the \
                 interpreter that made it",
                claim.heap_id, self.heap_id
            );
        }
        claim.index
    }

    /// The value `handle` holds. Panics if it is not one of this table's.
    pub(crate) fn get(self: &Rc<Self>, handle: &Owned) -> TaggedValue {
        let index = self.entry_of(&handle.0);
        self.entries
            .with(|entries| match &entries.list[index as usize] {
                Entry::Value(value) => *value,
                Entry::Environment(_) | Entry::Free => {
                    unreachable!("an `Owned` handle's entry holds its value")
                }
            })
    }

    /// Make `handle` hold `value`. Panics if it is not one of this table's.
    pub(crate) fn replace(self: &Rc<Self>, handle: &Owned, value: TaggedValue) {
        let index = self.entry_of(&handle.0);
        self.entries.with_mut(|entries| {
            let entry = &mut entries.list[index as usize];
            assert!(
                matches!(entry, Entry::Value(_)),
                "an `Owned` handle's entry holds its value"
            );
            *entry = Entry::Value(value);
        });
    }

    /// The environment `handle` holds. Panics if it is not one of this
    /// table's.
    pub(crate) fn get_environment(self: &Rc<Self>, handle: &OwnedEnvironment) -> Rc<Environment> {
        let index = self.entry_of(&handle.0);
        self.entries
            .with(|entries| match &entries.list[index as usize] {
                Entry::Environment(env) => Rc::clone(env),
                Entry::Value(_) | Entry::Free => {
                    unreachable!("an `OwnedEnvironment` handle's entry holds its environment")
                }
            })
    }
}

impl fmt::Debug for HandleTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandleTable")
            .field("heap", &self.heap_id)
            .field("handles", &self.len())
            .field("tracked", &self.tracked())
            .finish()
    }
}

/// A handle's claim on its entry of its heap's table, which it frees when
/// dropped: what [`Owned`] and [`OwnedEnvironment`] are each made of.
struct Claim {
    table: Weak<HandleTable>,
    index: u32,
    heap_id: u64,
}

impl Clone for Claim {
    /// A claim on the same value or environment, with an entry of its own.
    fn clone(&self) -> Self {
        match self.table.upgrade() {
            Some(table) => table.duplicate(self),
            // The table went with its interpreter: the copy is as dead.
            None => Claim {
                table: Weak::clone(&self.table),
                index: self.index,
                heap_id: self.heap_id,
            },
        }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(table) = self.table.upgrade() {
            table.release(self.index);
        }
    }
}

/// A value a host keeps: rooted until the handle is dropped (#605).
///
/// An interpreter's `eval_*_owned` methods answer one, and the interpreter
/// that made it reads it; it panics on a handle from another interpreter.
/// The handle keeps neither the heap nor its interpreter alive, and dropping
/// it after the interpreter does nothing. See `heap/handles.rs`.
#[derive(Clone)]
pub struct Owned(Claim);

impl fmt::Debug for Owned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned")
            .field("heap", &self.0.heap_id)
            .field("entry", &self.0.index)
            .finish()
    }
}

/// An environment a host keeps: rooted, with everything bound in it, until
/// the handle is dropped (#620).
///
/// An interpreter's `new_environment` answers one, and the interpreter that
/// made it uses it; it panics on a handle from another interpreter. Like
/// [`Owned`], the handle keeps neither the heap nor its interpreter alive,
/// and dropping it after the interpreter does nothing.
#[derive(Clone)]
pub struct OwnedEnvironment(Claim);

impl fmt::Debug for OwnedEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedEnvironment")
            .field("heap", &self.0.heap_id)
            .field("entry", &self.0.index)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::super::sentinels::Sentinels;
    use super::super::trace_sentinels::{collect_only, env_holding};
    use super::*;
    use crate::heap::new_shared_heap;

    /// The table's sentinel test (#623): built by a literal, with a fresh
    /// value in each kind of entry, a free entry between them, a tracked
    /// environment, and no other root.
    #[test]
    fn handle_table_fields() {
        let shared = new_shared_heap();
        let mut heap = shared.borrow_mut();
        let mut s = Sentinels::new(&mut heap);
        let h = &mut *heap;
        let value = s.pair(h, "HandleTable.entries.list[0]: Entry::Value");
        let bound = s.vector(h, "HandleTable.entries.list[2]: Entry::Environment");
        let tracked_value = s.object(h, "HandleTable.entries.tracked[0]");
        let held = env_holding_on(h, bound);
        let tracked = env_holding_on(h, tracked_value);
        // Parameter objects, so that their values survive only if the field
        // is visited, not merely marked.
        let standard = [
            "HandleTable.standard_ports[Input]",
            "HandleTable.standard_ports[Output]",
            "HandleTable.standard_ports[Error]",
        ]
        .map(|field| {
            let value = s.pair(h, field);
            h.alloc_parameter(value, None)
        });
        heap.handles = Rc::new(HandleTable {
            heap_id: 0,
            entries: HeapTable::new(Entries {
                list: vec![Entry::Value(value), Entry::Free, Entry::Environment(held)],
                free: vec![1],
                tracked: vec![Rc::downgrade(&tracked)],
            }),
            root_set: Rc::default(),
            standard_ports: HeapTable::new(Some(standard)),
        });
        collect_only(&mut heap, |_| {});
        s.assert_survived(&heap);
        drop(tracked);
    }

    /// An environment on `heap`, held borrowed, whose one binding holds
    /// `value`.
    fn env_holding_on(heap: &super::super::Heap, value: TaggedValue) -> Rc<Environment> {
        let env = Rc::new(Environment::namespace(heap));
        env.define("held", value);
        env
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
    fn an_environment_handle_roots_what_is_bound_in_it_until_it_is_dropped() {
        let shared = new_shared_heap();
        let value = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
        let env = env_holding(&shared, value);
        let handle = shared.borrow().hold_environment(env);
        let copy = handle.clone();
        let mut heap = shared.borrow_mut();
        collect_only(&mut heap, |_| {});
        let env = heap.held_environment(&copy);
        assert_eq!(heap.car(env.get("held").unwrap()), TaggedValue::fixnum(2));
        drop(env);
        drop(handle);
        drop(copy);
        assert!(heap.handles.is_empty());
        collect_only(&mut heap, |_| {});
        assert!(heap.free_pairs.contains(&value.heap_index()));
    }

    #[test]
    fn a_tracked_environment_is_traced_while_anything_holds_it() {
        let shared = new_shared_heap();
        let value = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL);
        let env = env_holding(&shared, value);
        shared.borrow().track_environment(&env);
        shared.borrow().track_environment(&env);
        assert_eq!(shared.borrow().handles.tracked(), 1, "tracked once");
        let mut heap = shared.borrow_mut();
        collect_only(&mut heap, |_| {});
        assert_eq!(heap.car(env.get("held").unwrap()), TaggedValue::fixnum(3));
        drop(env);
        collect_only(&mut heap, |_| {});
        assert_eq!(heap.handles.tracked(), 0, "forgotten once nothing holds it");
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
        assert_eq!(third.0.index, 0, "the freed entry is reused");
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
    #[should_panic(expected = "used with heap")]
    fn an_environment_handle_from_another_heap_is_refused() {
        let first = new_shared_heap();
        let second = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&first));
        let handle = first.borrow().hold_environment(env);
        second.borrow().held_environment(&handle);
    }

    #[test]
    #[should_panic(expected = "an environment of another heap")]
    fn an_environment_of_another_heap_is_not_held() {
        let first = new_shared_heap();
        let second = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&first));
        let _handle = second.borrow().hold_environment(env);
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

    /// An environment handle closes no cycle through the heap: the table
    /// holds the environment, which holds no handle to the heap (#704). So
    /// the heap goes when its last holder lets go, with no teardown, and the
    /// handle still drops to nothing after. Before #704 the environment held
    /// the heap, and only teardown broke the cycle.
    #[test]
    fn a_held_environment_does_not_keep_its_heap_alive() {
        let shared = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&shared));
        let handle = shared.borrow().hold_environment(env);
        let weak = Rc::downgrade(&shared);
        drop(shared);
        assert!(
            weak.upgrade().is_none(),
            "the held environment kept its heap alive"
        );
        drop(handle);
    }
}
