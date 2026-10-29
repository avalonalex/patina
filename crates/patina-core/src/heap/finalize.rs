//! The `FinalRegistry`: the objects whose death the heap acts on in Rust,
//! after the collection that finds them dead (#785, GC stage 4a;
//! `PRD/GC_PRD.md` §9.6, §9.9 steps 7 and 11, §14).
//!
//! One kind today, a file port: its drop flushes and closes a file, and a
//! `FileSystem` other than the operating system's may supply the writer
//! that does it. So a dead file port is not dropped inside the collection,
//! under the heap's mutable borrow, but registered when its object is made
//! (`Heap::alloc_port`), queued by the collection that finds the object
//! dead, and dropped by the drain, once the collection has returned.
//!
//! - **Classification** ([`Heap::queue_dead_finals`]) runs after marking,
//!   the ephemeron fixpoint and its break included, and before the sweep
//!   reuses the mark bits: an entry whose object is unmarked moves from the
//!   registered list to the queue, so it is never classified again once its
//!   slot is reused. A queued port the table holds the last `Rc` of gives up
//!   its charge there, so the L the sweep settles counts no dead file port,
//!   as when the sweep dropped it.
//! - **The drain** ([`finalize_after_collection`]) runs in both collection
//!   entries, `GcController::safe_point` and `collect_at_call`, once the
//!   collection has returned and the heap's borrow with it: before Scheme
//!   resumes, and before a call retried after `Step::Collect` opens again.
//!   It takes the queued entries out in id order and drops their ports
//!   with no heap borrow and no table access open, under F4's checks
//!   ([`finalizing`]). The machine's run is still on the stack, so a
//!   writer must not call back into the interpreter.
//! - **Teardown** ([`teardown`]) queues every entry under the heap's borrow,
//!   so that no registered index names a slot it frees, and drains them
//!   once the borrow is released (F5).
//!
//! Neither this table nor the `PortTable` is accessed inside an access to
//! the other (#687). The registry is on the `HandleTable`, by `Rc`, beside
//! the `PortTable`, and teardown's new handle table takes both over. 4f adds
//! `FinalKind::HostPayload`; the young and old lists come with 5a, and
//! `queue` with its first caller, a terminated thread (stage 9).

use std::rc::Rc;
use std::time::Instant;

use super::gc::{AssertNoGc, MarkBits};
use super::table::HeapTable;
use super::{GC_CHECK, Heap, HeapObjectData, PortId, SharedHeap};
use crate::tagged_value::HeapIndex;

/// What an entry finalizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FinalKind {
    /// A file port, whose `PortTable` entry the id names.
    Port,
}

/// A registered object: its slot in the object arena, which classification
/// reads the mark bit of, and what to finalize when it dies. By index, not
/// by word, so the registry holds no reference (`heap/check.rs`).
#[derive(Debug, Clone, Copy)]
struct Entry {
    object: HeapIndex,
    kind: FinalKind,
    id: u32,
}

/// One heap's objects to finalize. See the [module docs](self).
pub(crate) struct FinalRegistry {
    lists: HeapTable<Lists>,
}

#[derive(Default)]
struct Lists {
    /// The entries whose objects the last collection found live, and those
    /// registered since.
    registered: Vec<Entry>,
    /// The entries whose objects died, for the next drain.
    queued: Vec<(FinalKind, u32)>,
}

impl FinalRegistry {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            lists: HeapTable::new(Lists::default()),
        })
    }

    /// Register the object in slot `object`, to be finalized as `kind`'s
    /// `id` once a collection finds it dead. Only a constructor calls it.
    pub(crate) fn register(&self, object: HeapIndex, kind: FinalKind, id: u32) {
        self.lists
            .with_mut(|lists| lists.registered.push(Entry { object, kind, id }));
    }

    /// Classification: queue every entry whose object `live` says is dead,
    /// and answer those it queued.
    fn queue_dead(
        &self,
        mut live: impl FnMut(HeapIndex, FinalKind, u32) -> bool,
    ) -> Vec<(FinalKind, u32)> {
        self.lists.with_mut(|lists| {
            let Lists { registered, queued } = lists;
            let start = queued.len();
            registered.retain(|entry| {
                let keep = live(entry.object, entry.kind, entry.id);
                if !keep {
                    queued.push((entry.kind, entry.id));
                }
                keep
            });
            queued[start..].to_vec()
        })
    }

    /// Teardown's classification: queue every entry.
    fn queue_all(&self) {
        self.lists.with_mut(|lists| {
            let Lists { registered, queued } = lists;
            queued.extend(registered.drain(..).map(|entry| (entry.kind, entry.id)));
        });
    }

    /// The queued entries, taken out in id order.
    fn drain(&self) -> Vec<(FinalKind, u32)> {
        let mut queued = self
            .lists
            .with_mut(|lists| std::mem::take(&mut lists.queued));
        queued.sort_unstable_by_key(|&(kind, id)| (id, kind));
        queued
    }

    /// A check build's part of the sweep: each dead port of `ids` that is
    /// the registry's was queued by this collection's classification, so a
    /// registration a constructor missed panics rather than leaking its
    /// descriptor until teardown.
    fn assert_queued(&self, ids: &[PortId]) {
        if ids.is_empty() {
            return;
        }
        self.lists.with(|lists| {
            for id in ids {
                if !lists.queued.contains(&(FinalKind::Port, id.number())) {
                    not_queued(*id);
                }
            }
        });
    }

    /// The entries registered and the entries queued, for tests.
    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        self.lists
            .with(|lists| (lists.registered.len(), lists.queued.len()))
    }
}

impl Heap {
    /// Classification (§9.9 step 7): queue each registered object that the
    /// mark bits, complete and not yet reused by the sweep, leave unmarked.
    /// Each queued port whose every `Rc` the queued entries hold gives up its
    /// charge, dropped after the table's access and before the sweep settles
    /// the account: so L, the next trigger and the record's external bytes
    /// count no dead file port. A port another owner keeps keeps its charge,
    /// until its last `Rc` drops; nothing runs between here and the drain
    /// that could change the count.
    pub(crate) fn queue_dead_finals(&mut self, marks: &MarkBits) {
        let objects = &self.objects;
        let queued = self.handles.finals().queue_dead(|object, kind, id| {
            let live = marks.objects.get(object as usize);
            if GC_CHECK && !live {
                check_final_slot(objects, object, kind, id);
            }
            live
        });
        if queued.is_empty() {
            return;
        }
        let ids: Vec<PortId> = queued
            .iter()
            .map(|&(FinalKind::Port, id)| PortId::from_number(id))
            .collect();
        let charges = self.handles.ports().take_last_charges(&ids);
        drop(charges);
    }

    /// The sweep's part, once the object arena is swept: `finalized`, the
    /// dead ports the sweep left to the drain, were queued (a check build's
    /// assertion; empty elsewhere).
    pub(crate) fn assert_finals_queued(&self, finalized: &[PortId]) {
        if GC_CHECK {
            self.handles.finals().assert_queued(finalized);
        }
    }

    /// Teardown's part under the borrow: queue every registered entry, so
    /// that none names a slot the teardown frees.
    pub(crate) fn queue_every_final(&self) {
        self.handles.finals().queue_all();
    }

    /// Finalize the queued entries of a heap no one else can reach, for core's
    /// tests, which collect through a `&mut Heap`.
    #[cfg(test)]
    pub(crate) fn finalize_queued(&mut self) {
        finalize(&self.handles);
    }
}

/// A check build's check of a dead registered object: its slot still holds
/// what it was registered as, since a slot is freed only by a sweep, and the
/// sweep frees it only after this classification.
#[cold]
#[inline(never)]
fn check_final_slot(objects: &[HeapObjectData], object: HeapIndex, kind: FinalKind, id: u32) {
    let FinalKind::Port = kind;
    match objects.get(object as usize) {
        Some(HeapObjectData::Port(port)) if port.number() == id => {}
        other => panic!(
            "finalization entry for port {id} names object slot {object}, which holds {:?}: \
             a registered object's slot was freed or reused without its entry being queued (#785)",
            other.map(std::mem::discriminant)
        ),
    }
}

#[cold]
#[inline(never)]
fn not_queued(id: PortId) -> ! {
    panic!(
        "the sweep freed a port object whose entry {} is the finalization registry's, but the \
         collection did not queue it: its object was never registered (#785)",
        id.number()
    )
}

/// The drain (§9.9 step 11): finalize what the collection just run queued,
/// with the heap's borrow released, and add its time to the collection's
/// record, as `after`'s. Called by both collection entries once a collection
/// ran. Out of line and not generic, so each machine's entry grows by a call.
#[inline(never)]
pub(crate) fn finalize_after_collection(heap: &SharedHeap) {
    let started = Instant::now();
    run_finalizers(heap);
    heap.borrow_mut().note_finalized(started.elapsed());
}

/// Free every slot of `heap` and finalize every entry, as its owner does
/// when it is dropped (`Heap::teardown`, #604, F5): every entry is queued
/// under the borrow, which teardown's sweep needs, and finalized once the
/// borrow is released, so that a file port's writer flushes with no heap
/// borrow open. A heap someone still holds a borrow of is left as it is,
/// leaking rather than freeing what the borrow may read. A second teardown
/// frees and finalizes nothing.
pub fn teardown(heap: &SharedHeap) {
    let Ok(mut torn) = heap.try_borrow_mut() else {
        return;
    };
    torn.teardown();
    drop(torn);
    run_finalizers(heap);
}

/// Finalize the queued entries of `heap`, which no one borrows, under F4's
/// checks.
fn run_finalizers(heap: &SharedHeap) {
    let handles = Rc::clone(&heap.borrow().handles);
    finalizing(heap, || finalize(&handles));
}

/// Take the queued entries out of the registry, then their ports out of the
/// `PortTable`, never one access inside the other, and drop the ports with
/// no access open: a file whose `Rc` was the last flushes and closes, a
/// flush error ignored, and a `MemoryFs` writer commits. A drop, not
/// `Port::close`, which would close a port another owner still holds: only
/// the last `Rc`'s drop closes the file.
fn finalize(handles: &super::handles::HandleTable) {
    let queued = handles.finals().drain();
    if queued.is_empty() {
        return;
    }
    let ids: Vec<PortId> = queued
        .iter()
        .map(|&(FinalKind::Port, id)| PortId::from_number(id))
        .collect();
    let ports = handles.ports().take_finalized(&ids);
    drop(ports);
}

/// F4, asserted (`PRD/GC_PRD.md` §9.6): `run`, a finalization, reaches no
/// poll, under an [`AssertNoGc`], and in a check build allocates nothing
/// on the heap, which is not borrowed while it runs.
pub(crate) fn finalizing(heap: &SharedHeap, run: impl FnOnce()) {
    let allocated = GC_CHECK.then(|| allocations(heap));
    let window = AssertNoGc::new(heap);
    run();
    drop(window);
    if let Some(before) = allocated {
        let after = allocations(heap);
        if after != before {
            allocated_while_finalizing(before, after);
        }
    }
}

/// What the heap has allocated: the allocations since the last collection,
/// and the bytes, which external charges count too.
fn allocations(heap: &SharedHeap) -> (usize, u64) {
    let heap = heap.borrow();
    (heap.allocs_since_gc, heap.bytes_allocated())
}

#[cold]
#[inline(never)]
fn allocated_while_finalizing(before: (usize, u64), after: (usize, u64)) -> ! {
    panic!(
        "a finalizer allocated: {before:?} allocations and bytes before it, {after:?} after; \
         finalization allocates nothing (F4, #785)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::gc::{MarkSweepCollector, RootProvider, SlotVisitor};
    use crate::port::Port;
    use crate::tagged_value::TaggedValue;
    use std::cell::RefCell;
    use std::path::Path;

    #[derive(Default)]
    struct Roots(Vec<TaggedValue>);

    impl RootProvider for Roots {
        fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
            for &value in &self.0 {
                visitor.pinned(value);
            }
        }
    }

    fn collect(heap: &mut Heap, roots: &[TaggedValue]) {
        crate::heap::gc::Collector::collect(
            &mut MarkSweepCollector::new(),
            heap,
            &[&Roots(roots.to_vec())],
        );
    }

    fn text(fs: &crate::vfs::MemoryFs, path: &str) -> Option<String> {
        fs.get_text_file(Path::new(path))
    }

    /// A file port nothing reaches is queued by the collection that finds it
    /// dead, and a rooted one never is; only file ports register.
    #[test]
    fn a_dead_file_port_is_queued_and_a_rooted_one_is_not() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = Heap::new();
        let kept = heap.alloc_port(Port::open_output_file("/kept", &fs).unwrap());
        let dead = Port::open_output_file("/dead", &fs).unwrap();
        dead.write_string("dead").unwrap();
        heap.alloc_port(dead);
        heap.alloc_port(Port::new_output_string());
        assert_eq!(
            heap.handles.finals().counts(),
            (2, 0),
            "the string port is not registered"
        );

        heap.get_port(kept).unwrap().write_string("kept").unwrap();
        collect(&mut heap, &[kept]);
        assert_eq!(heap.handles.finals().counts(), (1, 1));
        assert_eq!(
            heap.port_entries(),
            2,
            "the dead file port's entry waits for the drain"
        );
        heap.finalize_queued();
        assert_eq!(heap.handles.finals().counts(), (1, 0));
        assert_eq!(heap.port_entries(), 1);
        assert_eq!(text(&fs, "/dead").as_deref(), Some("dead"));

        collect(&mut heap, &[kept]);
        assert_eq!(
            heap.handles.finals().counts(),
            (1, 0),
            "a live port is never queued"
        );
        assert_eq!(text(&fs, "/kept"), None, "not written out while it lives");
    }

    /// A port held only as an ephemeron's datum is kept while the key lives,
    /// and queued by the collection that finds the key dead.
    #[test]
    fn a_port_an_ephemeron_holds_is_queued_once_its_key_dies() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = Heap::new();
        let datum = Port::open_output_file("/datum", &fs).unwrap();
        datum.write_string("datum").unwrap();
        let port = heap.alloc_port(datum);
        let key = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let ephemeron = heap.alloc_ephemeron(key, port);

        collect(&mut heap, &[ephemeron, key]);
        assert_eq!(
            heap.handles.finals().counts(),
            (1, 0),
            "kept while the key lives"
        );
        collect(&mut heap, &[ephemeron]);
        assert_eq!(
            heap.handles.finals().counts(),
            (0, 1),
            "queued once the key died"
        );
        assert_eq!(text(&fs, "/datum"), None);
        heap.finalize_queued();
        assert_eq!(text(&fs, "/datum").as_deref(), Some("datum"));
    }

    /// After a collection that queues N dead file ports, and before the
    /// drain, L and the external bytes leave out their N buffers: the ports
    /// are still open, but nothing counts them.
    #[test]
    fn a_queued_port_leaves_l_before_the_drain() {
        use crate::heap::FILE_PORT_BYTES;

        let fs = crate::vfs::MemoryFs::new();
        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        let mut controller = crate::GcController::new(crate::GcMode::On);
        let kept = shared
            .borrow_mut()
            .alloc_port(Port::open_output_file("/kept", &fs).unwrap());
        for i in 0..4 {
            let port = Port::open_output_file(&format!("/dead-{i}"), &fs).unwrap();
            port.write_string("dead").unwrap();
            shared.borrow_mut().alloc_port(port);
        }
        assert_eq!(shared.borrow().external_bytes(), 5 * FILE_PORT_BYTES);
        controller.collect(&mut shared.borrow_mut(), &[&Roots(vec![kept])], false);
        {
            let heap = shared.borrow();
            assert_eq!(heap.external_bytes(), FILE_PORT_BYTES);
            assert!(
                heap.live_bytes() < 2 * FILE_PORT_BYTES,
                "{}",
                heap.live_bytes()
            );
            assert_eq!(heap.open_file_ports(), 1);
            assert_eq!(heap.handles.finals().counts(), (1, 4));
        }
        assert_eq!(
            text(&fs, "/dead-0"),
            None,
            "not written out inside the collection"
        );
        finalize_after_collection(&shared);
        for i in 0..4 {
            assert_eq!(text(&fs, &format!("/dead-{i}")).as_deref(), Some("dead"));
        }
        assert_eq!(shared.borrow().external_bytes(), FILE_PORT_BYTES);
        assert_eq!(shared.borrow().port_entries(), 1);
    }

    /// A dead port another owner keeps is queued and drained like any other,
    /// but keeps its charge, and stays open, until its last `Rc` drops.
    #[test]
    fn a_dead_port_a_host_keeps_stays_open_and_charged() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = Heap::new();
        let host = Port::open_output_file("/host", &fs).unwrap();
        heap.alloc_port(Rc::clone(&host));
        collect(&mut heap, &[]);
        heap.finalize_queued();
        assert_eq!((heap.open_file_ports(), heap.port_entries()), (1, 0));
        assert!(host.is_open());
        host.write_string("host").unwrap();
        drop(host);
        assert_eq!(text(&fs, "/host").as_deref(), Some("host"));
        assert_eq!(heap.open_file_ports(), 0);
    }

    /// A port a host gave two objects, whose entries hold its every `Rc`
    /// once the host lets go, leaves L with the collection that finds both
    /// dead, and is written out once, by the drain.
    #[test]
    fn a_port_with_two_dead_objects_leaves_l_with_their_collection() {
        let fs = crate::vfs::MemoryFs::new();
        let mut heap = Heap::new();
        let port = Port::open_output_file("/twice", &fs).unwrap();
        port.write_string("twice").unwrap();
        heap.alloc_port(Rc::clone(&port));
        heap.alloc_port(port);
        collect(&mut heap, &[]);
        assert_eq!((heap.external_bytes(), heap.open_file_ports()), (0, 0));
        assert!(heap.live_bytes() < crate::heap::FILE_PORT_BYTES);
        assert_eq!(text(&fs, "/twice"), None);
        heap.finalize_queued();
        assert_eq!(text(&fs, "/twice").as_deref(), Some("twice"));
        assert_eq!(heap.port_entries(), 0);
    }

    /// The drain answers the queued entries in id order, whatever order they
    /// were queued in.
    #[test]
    fn the_drain_answers_in_id_order() {
        let registry = FinalRegistry::new();
        for (object, id) in [(10, 3), (11, 1), (12, 2), (13, 0)] {
            registry.register(object, FinalKind::Port, id);
        }
        let queued = registry.queue_dead(|object, _, _| object == 13);
        assert_eq!(queued.len(), 3);
        let ids: Vec<u32> = registry.drain().into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(registry.drain().is_empty());
        registry.queue_all();
        assert_eq!(registry.drain(), vec![(FinalKind::Port, 0)]);
    }

    /// Teardown finalizes every entry, live or queued, with the heap's borrow
    /// released; a second teardown finalizes nothing (F5).
    #[test]
    fn teardown_finalizes_every_entry_once() {
        let fs = crate::vfs::MemoryFs::new();
        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        let kept = {
            let mut heap = shared.borrow_mut();
            let port = Port::open_output_file("/live", &fs).unwrap();
            port.write_string("live").unwrap();
            let kept = heap.alloc_port(port);
            let dead = Port::open_output_file("/queued", &fs).unwrap();
            dead.write_string("queued").unwrap();
            heap.alloc_port(dead);
            kept
        };
        collect(&mut shared.borrow_mut(), &[kept]);
        assert_eq!(shared.borrow().handles.finals().counts(), (1, 1));

        teardown(&shared);
        assert_eq!(text(&fs, "/live").as_deref(), Some("live"));
        assert_eq!(text(&fs, "/queued").as_deref(), Some("queued"));
        let heap = shared.borrow();
        assert_eq!(heap.handles.finals().counts(), (0, 0));
        assert_eq!((heap.port_entries(), heap.open_file_ports()), (0, 0));
        drop(heap);
        teardown(&shared);
        assert_eq!(shared.borrow().handles.finals().counts(), (0, 0));
    }

    /// A heap someone holds a borrow of is not torn down, nor finalized.
    #[test]
    fn teardown_leaves_a_borrowed_heap_alone() {
        let fs = crate::vfs::MemoryFs::new();
        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        shared
            .borrow_mut()
            .alloc_port(Port::open_output_file("/out", &fs).unwrap());
        let held = shared.borrow();
        teardown(&shared);
        assert_eq!(held.handles.finals().counts(), (1, 0));
        assert_eq!(text(&fs, "/out"), None);
    }

    /// A check build refuses a classification whose dead entry's slot no
    /// longer holds its port: a slot freed, or reused, without its entry
    /// being queued.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "without its entry being queued")]
    fn a_check_build_refuses_an_entry_whose_slot_was_freed() {
        let mut heap = Heap::new();
        // A string port is not registered: its slot is freed with no entry.
        let freed = heap.alloc_port(Port::new_output_string());
        collect(&mut heap, &[]);
        heap.handles
            .finals()
            .register(freed.heap_index(), FinalKind::Port, 0);
        collect(&mut heap, &[]);
    }

    /// A check build refuses a sweep that frees a port object whose entry is
    /// the registry's but was never registered, so a constructor that forgot
    /// to register panics rather than leaking its descriptor.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "never registered")]
    fn a_check_build_refuses_a_finalized_port_that_was_never_registered() {
        let mut heap = Heap::new();
        let id = heap.handles.ports().insert(Port::new_output_string(), true);
        let object = heap.alloc_object(HeapObjectData::Port(id));
        heap.handles.ports().set_owner(id, object);
        collect(&mut heap, &[]);
    }
}
