//! Stale-reference checks (#621): in a check build
//! ([`GC_CHECK`](super::GC_CHECK)), a heap reference that outlived its slot
//! panics where it is used, instead of reading as a legal value.
//!
//! Each arena keeps one word per slot. Its low 16 bits are the slot's
//! allocation generation, and [`FREED`] is set while the slot is on the free
//! list:
//! - sweep sets `FREED` and bumps the generation as it frees a slot
//!   ([`SlotChecks::free`]);
//! - the reuse branch of the arena's `alloc_*` clears `FREED`
//!   ([`SlotChecks::reuse`]);
//! - every reference built from an index carries the slot's generation in
//!   `TaggedValue` payload bits 40–55, which `heap_index` does not read.
//!
//! So a reference matches its slot's word exactly while the slot still holds
//! the object it was made for, and one comparison tells an accessor all it
//! needs: a mismatch with `FREED` set is a use after free, and a mismatch
//! without it is a reference to a slot that has since been freed and reused,
//! which no tombstone or poison can see because the slot holds a legal value
//! again. The marker checks the same word in `GcVisitor::visit`.
//!
//! The word is a freed-slot bitset and a generation table in one `Vec<u32>`,
//! so a check is one plain index and one compare. That matters in an
//! unoptimized build, where every accessor pays it.
//!
//! The stamp is a property of today's encoding: `PRD/GC_PRD.md` replaces it
//! at stage 5 with the poisoning sweep, the quarantine and the heap verifier.
//! In a build without the checks [`SlotChecks`] is zero-sized and every
//! method is an empty inline function, so a plain release build compiles none
//! of this and its values carry no stamp.
//!
//! **Where references are built from an index.** A reference that carried the
//! wrong stamp would be a false positive here, and would break `eq?`, which
//! compares raw bits. An audit of the workspace for every way to build a heap
//! `TaggedValue` from an index or raw bits found these, all in `heap/`:
//! - the four `alloc_*` returns, stamped by [`SlotChecks::reuse`] (a fresh
//!   slot's generation is 0, the unstamped value);
//! - the symbol and core-syntax lookups, stamped by [`SlotChecks::stamp`];
//! - the four freed-bits reports in sweep, stamped by [`SlotChecks::free`]
//!   with the generation the slot had *before* it was freed, so that a
//!   `SourceMap` key (the raw bits of the value as it was made) still
//!   matches the report that prunes it;
//! - `GcVisitor::visit_object_index`, which rebuilds the reference from an
//!   [`ObjectIndex`](crate::tagged_value::ObjectIndex) that kept the stamp,
//!   and the `syntax_sources` prune, which reads only the index.
//!
//! `TaggedValue::from_raw` has no other caller, and it and the index
//! constructors (`TaggedValue::pair` and the rest) are crate-private, so no
//! other crate can mint a reference. Every copy of a value carries the same stamp, so the
//! consumers of raw bits — `eq?`/`eqv?`/`equal?`'s fast paths, identity
//! hashing (which hashes the index), the datum writer's labels, the
//! desugarer's and parser's `seen` sets, the VM's primitive table and the
//! `SourceMap` — see one key per object, as before.

#[cfg(any(debug_assertions, feature = "gc-check"))]
pub(crate) use checked::SlotChecks;
#[cfg(not(any(debug_assertions, feature = "gc-check")))]
pub(crate) use unchecked::SlotChecks;

/// Set in a slot's word while the slot is on its arena's free list.
#[cfg(any(debug_assertions, feature = "gc-check"))]
const FREED: u32 = 1 << 16;

#[cfg(any(debug_assertions, feature = "gc-check"))]
mod checked {
    use super::FREED;
    use crate::tagged_value::{ObjectIndex, TaggedValue};

    /// One word per slot of an arena: the slot's generation, and `FREED`.
    #[derive(Debug)]
    pub(crate) struct SlotChecks {
        slots: Vec<u32>,
    }

    impl SlotChecks {
        /// The state of an empty arena.
        pub(crate) const fn new() -> Self {
            Self { slots: Vec::new() }
        }

        /// A slot was appended to the arena. Its generation is 0, so the
        /// reference `alloc_*` returns for it needs no stamp.
        #[inline(always)]
        pub(crate) fn push(&mut self) {
            self.slots.push(0);
        }

        /// The reuse branch of `alloc_*` took `fresh`'s slot off the free
        /// list. Returns `fresh` stamped with the slot's current generation.
        #[inline(always)]
        pub(crate) fn reuse(&mut self, fresh: TaggedValue) -> TaggedValue {
            let slot = &mut self.slots[fresh.heap_index() as usize];
            *slot &= !FREED;
            fresh.with_generation(*slot as u16)
        }

        /// `tv`, built from the index of a live slot, stamped with that
        /// slot's generation.
        #[inline(always)]
        pub(crate) fn stamp(&self, tv: TaggedValue) -> TaggedValue {
            tv.with_generation(self.slots[tv.heap_index() as usize] as u16)
        }

        /// Sweep frees `dead`'s slot: bump its generation and set `FREED`.
        /// Returns `dead` stamped with the generation the slot had until now,
        /// which is the stamp every reference to the dead object carries.
        #[inline]
        pub(crate) fn free(&mut self, dead: TaggedValue) -> TaggedValue {
            let slot = &mut self.slots[dead.heap_index() as usize];
            let generation = *slot as u16;
            *slot = u32::from(generation.wrapping_add(1)) | FREED;
            dead.with_generation(generation)
        }

        /// The accessors' check: panic unless `tv` names a live slot of its
        /// own generation.
        #[inline(always)]
        pub(crate) fn check(&self, arena: &'static str, tv: TaggedValue) {
            let slot = self.slots[tv.heap_index() as usize];
            if slot != u32::from(tv.generation()) {
                stale_reference(arena, tv, slot);
            }
        }

        /// [`Self::check`] for an accessor that answers `None` for an index
        /// past the arena rather than panicking, so that it still does.
        #[inline]
        pub(crate) fn check_if_present(&self, arena: &'static str, tv: TaggedValue) {
            if let Some(&slot) = self.slots.get(tv.heap_index() as usize)
                && slot != u32::from(tv.generation())
            {
                stale_reference(arena, tv, slot);
            }
        }

        /// [`Self::check`] for a bare object index (`CallFrame.closure`).
        #[inline(always)]
        pub(crate) fn check_index(&self, index: ObjectIndex) {
            self.check("object", index.value());
        }

        /// The marker's check (`GcVisitor::visit`): panic if a root or a
        /// traced edge names a slot that was freed and reused since the
        /// reference was made, which sweep cannot see because the slot is
        /// live again. A slot that is free now is left to sweep's pre-mark
        /// check, which reports it with its own message at the end of this
        /// collection, so that each check has a control of its own.
        #[inline(always)]
        pub(crate) fn check_reached(&self, arena: &'static str, tv: TaggedValue) {
            let slot = self.slots[tv.heap_index() as usize];
            if slot != u32::from(tv.generation()) && slot & FREED == 0 {
                reused_slot_reached_by_marking(arena, tv, slot);
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn stale_reference(arena: &str, tv: TaggedValue, slot: u32) -> ! {
        let index = tv.heap_index();
        if slot & FREED != 0 {
            panic!("use-after-free: {arena} slot {index} was reclaimed by the GC");
        }
        panic!(
            "stale {arena} reference, slot {index} generation {} (now {}): \
             freed and reused since this value was made",
            tv.generation(),
            slot as u16
        );
    }

    #[cold]
    #[inline(never)]
    fn reused_slot_reached_by_marking(arena: &str, tv: TaggedValue, slot: u32) -> ! {
        panic!(
            "dangling reference: {arena} slot {} generation {} (now {}) was freed and reused, \
             but marking reached it",
            tv.heap_index(),
            tv.generation(),
            slot as u16
        );
    }
}

/// The same interface with nothing behind it: what a plain release build
/// compiles. Zero-sized, so `Heap` keeps its layout, and every method is an
/// empty inline function, so no accessor gains a load or a branch.
#[cfg(not(any(debug_assertions, feature = "gc-check")))]
mod unchecked {
    use crate::tagged_value::{ObjectIndex, TaggedValue};

    #[derive(Debug)]
    pub(crate) struct SlotChecks;

    impl SlotChecks {
        pub(crate) const fn new() -> Self {
            Self
        }

        #[inline(always)]
        pub(crate) fn push(&mut self) {}

        #[inline(always)]
        pub(crate) fn reuse(&mut self, fresh: TaggedValue) -> TaggedValue {
            fresh
        }

        #[inline(always)]
        pub(crate) fn stamp(&self, tv: TaggedValue) -> TaggedValue {
            tv
        }

        #[inline(always)]
        pub(crate) fn free(&mut self, dead: TaggedValue) -> TaggedValue {
            dead
        }

        #[inline(always)]
        pub(crate) fn check(&self, _arena: &'static str, _tv: TaggedValue) {}

        #[inline(always)]
        pub(crate) fn check_if_present(&self, _arena: &'static str, _tv: TaggedValue) {}

        #[inline(always)]
        pub(crate) fn check_index(&self, _index: ObjectIndex) {}

        #[inline(always)]
        pub(crate) fn check_reached(&self, _arena: &'static str, _tv: TaggedValue) {}
    }
}

// ============================================================================
// Positive controls
// ============================================================================
//
// One test per check, each expecting its check's message. They run in every
// check build — every debug `cargo test`, and a release one with `gc-check` —
// and a build without the checks reports them ignored rather than compiling
// them out, so a lane that meant to run the checks and did not shows it. The
// controls for sweep's pre-mark check (a root naming a free slot) are in
// `gc.rs`, beside the commit that introduced it.

#[cfg(test)]
mod tests {
    use crate::error::ExceptionKind;
    use crate::heap::{Heap, PromiseState};
    use crate::tagged_value::{ObjectIndex, TaggedValue};
    use crate::{Collector, Environment, GcRoots, GcVisitor, MarkSweepCollector};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A control: a test that must panic with `$expected` in a check build.
    macro_rules! control {
        ($(#[$doc:meta])* $name:ident, $expected:literal, $body:block) => {
            $(#[$doc])*
            #[test]
            #[cfg_attr(
                not(any(debug_assertions, feature = "gc-check")),
                ignore = "needs a check build"
            )]
            #[should_panic(expected = $expected)]
            fn $name() $body
        };
    }

    struct Roots(Vec<TaggedValue>);

    impl GcRoots for Roots {
        fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
            visitor.visit_slice(&self.0);
        }
    }

    fn collect(heap: &mut Heap, roots: &[TaggedValue]) {
        MarkSweepCollector::new().collect(heap, &[&Roots(roots.to_vec())]);
    }

    /// A fresh heap holding only the object `make` allocates, collected with
    /// an empty root set: the returned reference names a free slot.
    fn freed(make: impl FnOnce(&mut Heap) -> TaggedValue) -> (Heap, TaggedValue) {
        let mut heap = Heap::new();
        let dead = make(&mut heap);
        collect(&mut heap, &[]);
        (heap, dead)
    }

    /// A reference to a slot freed and reused since. As [`freed`], then
    /// allocate again with `make`, which takes the one free slot, and keep
    /// that tenant's reference; collect with an empty root set and allocate
    /// once more. The returned reference was made at generation 1, and the
    /// slot is now at generation 2 with another tenant in it.
    ///
    /// The non-zero stamp is the point: a reference that lost its stamp on
    /// the way — an `ObjectIndex` that dropped it, a reuse that did not
    /// stamp — reads as generation 0, which is still refused but with a
    /// message the control does not expect, so the control fails.
    fn reused(make: impl Fn(&mut Heap) -> TaggedValue) -> (Heap, TaggedValue) {
        let (mut heap, first) = freed(&make);
        let dead = make(&mut heap);
        assert_eq!(dead.heap_index(), first.heap_index(), "the slot is reused");
        collect(&mut heap, &[]);
        let tenant = make(&mut heap);
        assert_eq!(tenant.heap_index(), dead.heap_index(), "and reused again");
        (heap, dead)
    }

    fn pair(heap: &mut Heap) -> TaggedValue {
        heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2))
    }

    fn vector(heap: &mut Heap) -> TaggedValue {
        heap.alloc_vector(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)])
    }

    fn string(heap: &mut Heap) -> TaggedValue {
        heap.alloc_str("ab")
    }

    fn bytevector(heap: &mut Heap) -> TaggedValue {
        heap.alloc_bytevector(vec![1, 2, 3])
    }

    fn vm_closure(heap: &mut Heap) -> TaggedValue {
        heap.alloc_vm_closure(0, vec![TaggedValue::fixnum(1)], Rc::new(Environment::new()))
    }

    // ------------------------------------------------------------------------
    // A reference to a freed slot, at each accessor (item 2 of #621)
    // ------------------------------------------------------------------------

    control!(
        get_pair_of_a_freed_pair,
        "use-after-free: pair slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(pair);
            heap.get_pair(dead);
        }
    );

    control!(
        set_car_of_a_freed_pair,
        "use-after-free: pair slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(pair);
            heap.set_car(dead, TaggedValue::NULL);
        }
    );

    control!(
        set_cdr_of_a_freed_pair,
        "use-after-free: pair slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(pair);
            heap.set_cdr(dead, TaggedValue::NULL);
        }
    );

    control!(
        /// The vector and string arenas had no detection before #621: their
        /// tombstone, an empty `Vec`, is a legal value, so a swept vector
        /// read back as `#()`.
        vector_len_of_a_freed_vector,
        "use-after-free: vector slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(vector);
            heap.vector_len(dead);
        }
    );

    control!(
        vector_ref_of_a_freed_vector,
        "use-after-free: vector slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(vector);
            heap.vector_ref(dead, 0);
        }
    );

    control!(
        vector_set_of_a_freed_vector,
        "use-after-free: vector slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(vector);
            heap.vector_set(dead, 0, TaggedValue::NULL);
        }
    );

    control!(
        vector_slice_of_a_freed_vector,
        "use-after-free: vector slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(vector);
            heap.vector_slice(dead);
        }
    );

    control!(
        vector_slice_mut_of_a_freed_vector,
        "use-after-free: vector slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(vector);
            heap.vector_slice_mut(dead);
        }
    );

    control!(
        /// Read back as `""` before #621.
        get_string_chars_of_a_freed_string,
        "use-after-free: string slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(string);
            heap.get_string_chars(dead);
        }
    );

    control!(
        get_string_chars_mut_of_a_freed_string,
        "use-after-free: string slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(string);
            heap.get_string_chars_mut(dead);
        }
    );

    control!(
        string_set_char_of_a_freed_string,
        "use-after-free: string slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(string);
            heap.string_set_char(dead, 0, 'x');
        }
    );

    control!(
        get_object_of_a_freed_object,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(bytevector);
            heap.get_object(dead);
        }
    );

    // The nine accessors that index the object arena directly, and answered
    // `None` or `false` for a freed slot before #621.

    control!(
        get_exception_of_a_freed_exception,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (heap, dead) =
                freed(|heap| heap.alloc_exception(ExceptionKind::Error, "e".into(), vec![]));
            heap.get_exception(dead);
        }
    );

    control!(
        /// `inner` is read through `get_promise` before anything is written,
        /// so this is `get_object`'s check; without it a freed `inner` reads
        /// as no promise at all and the update silently does nothing.
        promise_update_of_a_freed_promise,
        "use-after-free: object slot 1 was reclaimed by the GC",
        {
            let mut heap = Heap::new();
            let mut promise = || {
                heap.alloc_promise(Rc::new(RefCell::new(PromiseState::Forced(TaggedValue::NULL))))
            };
            let (outer, inner) = (promise(), promise());
            collect(&mut heap, &[outer]);
            heap.promise_update(outer, inner);
        }
    );

    control!(
        /// The write into `inner`'s slot. With that slot reused by another
        /// promise, an unchecked `get_promise(inner)` would hand back the
        /// tenant's box and the write would re-point the tenant. The write
        /// has no check of its own: `get_object`'s, which `get_promise(inner)`
        /// runs on the same value before anything is written, refuses it.
        promise_update_of_a_stale_promise,
        "stale object reference, slot 1 generation 0 (now 1): freed and reused since this value was made",
        {
            let promise = |heap: &mut Heap| {
                heap.alloc_promise(Rc::new(RefCell::new(PromiseState::Forced(TaggedValue::NULL))))
            };
            let mut heap = Heap::new();
            let (outer, inner) = (promise(&mut heap), promise(&mut heap));
            collect(&mut heap, &[outer]);
            let tenant = promise(&mut heap);
            assert_eq!(tenant.heap_index(), inner.heap_index());
            heap.promise_update(outer, inner);
        }
    );

    control!(
        retire_vm_closure_of_a_freed_closure,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(vm_closure);
            heap.retire_vm_closure(dead);
        }
    );

    control!(
        /// Behind `frame_globals`, which falls back to the global environment
        /// when this answers `None` — the way it answered a freed slot.
        get_vm_closure_globals_of_a_freed_closure,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(vm_closure);
            heap.get_vm_closure_globals(ObjectIndex::of(dead).unwrap());
        }
    );

    control!(
        get_vm_closure_free_var_of_a_freed_closure,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (heap, dead) = freed(vm_closure);
            heap.get_vm_closure_free_var(ObjectIndex::of(dead).unwrap(), 0);
        }
    );

    control!(
        set_vm_closure_free_var_of_a_freed_closure,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(vm_closure);
            heap.set_vm_closure_free_var(ObjectIndex::of(dead).unwrap(), 0, TaggedValue::NULL);
        }
    );

    control!(
        bytevector_u8_set_of_a_freed_bytevector,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(bytevector);
            heap.bytevector_u8_set(dead, 0, 9);
        }
    );

    control!(
        get_bytevector_mut_of_a_freed_bytevector,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(bytevector);
            heap.get_bytevector_mut(dead);
        }
    );

    control!(
        bytevector_copy_into_a_freed_bytevector,
        "use-after-free: object slot 0 was reclaimed by the GC",
        {
            let (mut heap, dead) = freed(bytevector);
            heap.bytevector_copy_into(dead, 0, &[9]);
        }
    );

    // ------------------------------------------------------------------------
    // A reference to a slot freed and reused since, read (item 3)
    // ------------------------------------------------------------------------

    control!(
        /// Before #621 this read the new tenant: `(1 . 2)` from a reference
        /// to a dead pair, with no report in any build.
        stale_pair_read_after_its_slot_was_reused,
        "stale pair reference, slot 0 generation 1 (now 2): freed and reused since this value was made",
        {
            let (heap, dead) = reused(pair);
            heap.get_pair(dead);
        }
    );

    control!(
        stale_vector_read_after_its_slot_was_reused,
        "stale vector reference, slot 0 generation 1 (now 2): freed and reused since this value was made",
        {
            let (heap, dead) = reused(vector);
            heap.vector_ref(dead, 0);
        }
    );

    control!(
        stale_string_read_after_its_slot_was_reused,
        "stale string reference, slot 0 generation 1 (now 2): freed and reused since this value was made",
        {
            let (heap, dead) = reused(string);
            heap.get_string_chars(dead);
        }
    );

    control!(
        stale_object_read_after_its_slot_was_reused,
        "stale object reference, slot 0 generation 1 (now 2): freed and reused since this value was made",
        {
            let (heap, dead) = reused(bytevector);
            heap.get_object(dead);
        }
    );

    // ------------------------------------------------------------------------
    // A root naming a slot freed and reused since, reached by marking (item 3)
    // ------------------------------------------------------------------------

    control!(
        marking_a_stale_pair_whose_slot_was_reused,
        "dangling reference: pair slot 0 generation 1 (now 2) was freed and reused, but marking reached it",
        {
            let (mut heap, dead) = reused(pair);
            collect(&mut heap, &[dead]);
        }
    );

    control!(
        marking_a_stale_vector_whose_slot_was_reused,
        "dangling reference: vector slot 0 generation 1 (now 2) was freed and reused, but marking reached it",
        {
            let (mut heap, dead) = reused(vector);
            collect(&mut heap, &[dead]);
        }
    );

    control!(
        marking_a_stale_string_whose_slot_was_reused,
        "dangling reference: string slot 0 generation 1 (now 2) was freed and reused, but marking reached it",
        {
            let (mut heap, dead) = reused(string);
            collect(&mut heap, &[dead]);
        }
    );

    control!(
        marking_a_stale_object_whose_slot_was_reused,
        "dangling reference: object slot 0 generation 1 (now 2) was freed and reused, but marking reached it",
        {
            let (mut heap, dead) = reused(bytevector);
            collect(&mut heap, &[dead]);
        }
    );

    // ------------------------------------------------------------------------
    // A `CallFrame.closure` whose slot was freed and reused (item 3)
    // ------------------------------------------------------------------------

    control!(
        /// What a VM frame holds, through the accessor `LoadClosure` uses.
        frame_closure_read_after_its_slot_was_reused,
        "stale object reference, slot 0 generation 1 (now 2): freed and reused since this value was made",
        {
            let (heap, dead) = reused(vm_closure);
            heap.get_vm_closure_free_var(ObjectIndex::of(dead).unwrap(), 0);
        }
    );

    control!(
        /// What the VM's root provider does with each frame's closure.
        marking_a_frame_closure_whose_slot_was_reused,
        "dangling reference: object slot 0 generation 1 (now 2) was freed and reused, but marking reached it",
        {
            struct Frame(ObjectIndex);
            impl GcRoots for Frame {
                fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
                    visitor.visit_object_index(self.0);
                }
            }
            let (mut heap, dead) = reused(vm_closure);
            let frame = Frame(ObjectIndex::of(dead).unwrap());
            MarkSweepCollector::new().collect(&mut heap, &[&frame]);
        }
    );

    // ------------------------------------------------------------------------
    // And the checks stay quiet where the references are good
    // ------------------------------------------------------------------------

    #[test]
    fn live_and_reused_references_pass_every_check() {
        let mut heap = Heap::new();
        let live = pair(&mut heap);
        let dead = pair(&mut heap);
        collect(&mut heap, &[live]);
        let tenant = pair(&mut heap);
        assert_eq!(tenant.heap_index(), dead.heap_index());
        assert_eq!(
            heap.get_pair(tenant),
            (TaggedValue::fixnum(1), TaggedValue::fixnum(2))
        );
        // A collection marking the live pair and the slot's new tenant.
        collect(&mut heap, &[live, tenant]);
        assert_eq!(heap.car(live), TaggedValue::fixnum(1));
        assert_eq!(heap.cdr(tenant), TaggedValue::fixnum(2));
        // The symbol table re-mints its references: they must carry the
        // stamp of the slot they name, or `eq?` on symbols would break.
        let filler = bytevector(&mut heap);
        collect(&mut heap, &[]);
        let _ = filler;
        let symbol = heap.intern_symbol("s");
        assert_eq!(heap.intern_symbol("s"), symbol);
        collect(&mut heap, &[]);
        assert_eq!(heap.intern_symbol("s"), symbol);
        heap.get_object(symbol);
    }

    #[test]
    fn freed_bits_carry_the_stamp_of_the_dead_reference() {
        // `SourceMap` keys are a value's raw bits as it was made; the report
        // that prunes them must use the same stamp, not the slot's next one.
        let mut heap = Heap::new();
        heap.enable_gc_freed_tracking();
        let first = pair(&mut heap);
        collect(&mut heap, &[]);
        let second = pair(&mut heap);
        assert_eq!(second.heap_index(), first.heap_index());
        collect(&mut heap, &[]);
        let crate::heap::GcFreedBits::Exact(bits) = heap.take_gc_freed_bits() else {
            panic!("two frees cannot overflow the cap");
        };
        assert_eq!(bits, vec![first.raw_bits(), second.raw_bits()]);
    }
}
