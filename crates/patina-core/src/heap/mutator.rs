//! The `Mutator`: the words one carrier of Scheme code owns, at the offsets
//! other tiers read (#699; `PRD/GC_PRD.md` §12 and §14, additions C1 and
//! C10).
//!
//! A JIT keeps the `Mutator` in a pinned register and reads its words at
//! fixed offsets, so its layout is an ABI. The offsets, the size and the
//! 128-byte alignment are asserted at compile time below, and they freeze
//! after stage 6's spike. Under M:1 a heap has one `Mutator`.
//!
//! **Live words.** Today only the event words are live:
//! - `reg_limit`, the poll word, which a post zeroes;
//! - `pending`, the events the owner posts;
//! - `event`, the events posted from anywhere else: signal handlers, timers,
//!   profilers, and later other carriers.
//!
//! **Rust's words.** Two words in the line's spare bytes are read by Rust
//! alone, never by generated code (#702):
//! - `no_gc_depth`, the open [`NoGcScope`](crate::NoGcScope)s: nested
//!   entries and holders, beneath which nothing collects;
//! - `driver_out`, whether the carrier's [`GcDriver`](crate::GcDriver), the
//!   capability to collect, is held.
//!
//! **Reserved words.** The rest stay zero until the stage that needs each:
//! - the allocation buffer, `ap` and `alloc_limit`, at 5a;
//! - the barrier's words, `meta_bias`, `remset_cur` and `remset_soft`, at 7;
//! - the register stack's `reg_top` at 4d;
//! - `ticks` with SRFI 18's deterministic scheduler;
//! - the words C1 and C10 reserve, so that adding them later breaks no ABI:
//!   `fenced_ap`, `quiesce_epoch` and `handshake`.
//!
//! **The protocol** (§12, OCaml's `domain.c`):
//! - **Owner posts** (allocation, `(gc)` where it cannot collect, finalizers,
//!   decommit): `pending |= bit`, then `reg_limit = 0`.
//! - **Remote posts:** `event.fetch_or(bit)`, then `reg_limit = 0`, both
//!   `SeqCst`.
//! - **Servicing.** A poll whose `reg_limit` reads 0 services what is
//!   posted, then restores the limit through [`Mutator::set_limit`]. Its
//!   fence makes a Dekker pair with a remote post, so no request is ever lost.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering, fence};

/// The events a poll services, one bit each (`PRD/GC_PRD.md` §12). Today
/// only [`GC_MAJOR`](event::GC_MAJOR) is posted; the others are named here
/// for the stages that post them.
pub mod event {
    /// A minor collection: generational heaps, stage 7.
    pub const GC_MINOR: u32 = 1 << 0;
    /// A full collection: what every collection of today's collector is.
    pub const GC_MAJOR: u32 = 1 << 1;
    /// A green-thread switch: SRFI 18's scheduler.
    pub const PREEMPT: u32 = 1 << 2;
    /// A signal a host or a handler posted: Ctrl-C (#701).
    pub const SIGNAL: u32 = 1 << 3;
    /// A debugger stop.
    pub const DEBUGGER: u32 = 1 << 4;
    /// A thread's termination: SRFI 18.
    pub const TERMINATE: u32 = 1 << 5;
    /// Reserved: a stop-the-world handshake under N carriers.
    pub const HANDSHAKE: u32 = 1 << 6;
    /// Finalizers queued by a collection, run after its pause (§9.6).
    pub const FINALIZERS_PENDING: u32 = 1 << 7;
    /// Empty memory to return, after a pause (§7).
    pub const DECOMMIT_PENDING: u32 = 1 << 8;
    /// An allocation dipped into the emergency reserve (§8, §17.3).
    pub const HEAP_EXHAUSTED: u32 = 1 << 9;
    /// A register stack to grow (§11.1).
    pub const STACK_GROW: u32 = 1 << 10;

    /// Either collection.
    pub const GC: u32 = GC_MINOR | GC_MAJOR;
}

/// One carrier's words, at the offsets of `PRD/GC_PRD.md` §14's table.
///
/// Its address is stable, since the heap owns it in an `Rc` that the
/// machines share, and an interrupt handle will point at it (#701). Every
/// word another thread may write is atomic: `reg_limit`, `event`,
/// `safepoint_state` and `handshake`. Relaxed loads and stores of those
/// compile to plain ones, so the atomics cost the owner nothing.
#[repr(C, align(128))]
#[derive(Debug)]
pub struct Mutator {
    /// 0x00. The allocation buffer's next free byte (5a).
    ap: Cell<usize>,
    /// 0x08. The allocation buffer's end (5a).
    alloc_limit: Cell<usize>,
    /// 0x10. The metadata table's bias, a per-heap constant the barrier's
    /// fast path loads once (7).
    meta_bias: usize,
    /// 0x18. The store buffer's next free entry (7).
    remset_cur: Cell<*mut usize>,
    /// 0x20. The store buffer's soft limit (7).
    remset_soft: Cell<*mut usize>,
    /// 0x28. The running green thread's register-stack top (4d).
    reg_top: Cell<*mut u64>,
    /// 0x30. The poll word: zero while an event is posted. A frame entry
    /// will compare the register-stack top against it (§12), so it is the
    /// stack's limit otherwise. Until stage 4d gives the VM a reserved stack
    /// it is [`Mutator::NO_LIMIT`]. Non-zero values are written only by the
    /// owner, through [`Mutator::set_limit`].
    reg_limit: AtomicUsize,
    /// 0x38. Events posted from outside the owner, by `fetch_or`.
    event: AtomicU32,
    /// 0x3C. Events the owner posted.
    pending: Cell<u32>,
    /// 0x40. Tick mode's poll count (SRFI 18's deterministic scheduler).
    ticks: Cell<i32>,
    /// 0x44. The barrier's mode, read by slow paths only (7).
    barrier_mode: u8,
    /// 0x45. `Running`, `AtSafepoint` or `InSafeRegion`, for the
    /// stop-the-world protocol under N carriers.
    safepoint_state: AtomicU8,
    /// 0x48. The running green thread (9).
    thread: Cell<*mut ()>,
    /// 0x50. The heap's shared part, for helpers (5a).
    heap: *const (),
    /// 0x58. A `Leaf` helper's error, and the `Oom` status (§8).
    status: Cell<u32>,
    /// 0x60. Reserved (C1): rule 4's standalone-fence variant, if stage 6
    /// chooses it (§18.2).
    fenced_ap: usize,
    /// 0x68. Reserved (C10): deferred code-memory reuse under N carriers.
    quiesce_epoch: u64,
    /// 0x70. Reserved (C10): the stop-the-world handshake under N carriers.
    handshake: AtomicU32,
    /// 0x74. The open [`NoGcScope`](crate::NoGcScope)s. A collection runs
    /// only at depth 0 (`PRD/GC_PRD.md` §11.3). Rust's alone.
    no_gc_depth: Cell<u32>,
    /// 0x78. Whether the carrier's [`GcDriver`](crate::GcDriver) is held:
    /// one per carrier (C2). Rust's alone.
    driver_out: Cell<bool>,
}

// The ABI (§14): each offset, the size and the alignment, checked as the
// crate compiles, so moving a word fails the build.
const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    assert!(offset_of!(Mutator, ap) == 0x00);
    assert!(offset_of!(Mutator, alloc_limit) == 0x08);
    assert!(offset_of!(Mutator, meta_bias) == 0x10);
    assert!(offset_of!(Mutator, remset_cur) == 0x18);
    assert!(offset_of!(Mutator, remset_soft) == 0x20);
    assert!(offset_of!(Mutator, reg_top) == 0x28);
    assert!(offset_of!(Mutator, reg_limit) == 0x30);
    assert!(offset_of!(Mutator, event) == 0x38);
    assert!(offset_of!(Mutator, pending) == 0x3C);
    assert!(offset_of!(Mutator, ticks) == 0x40);
    assert!(offset_of!(Mutator, barrier_mode) == 0x44);
    assert!(offset_of!(Mutator, safepoint_state) == 0x45);
    assert!(offset_of!(Mutator, thread) == 0x48);
    assert!(offset_of!(Mutator, heap) == 0x50);
    assert!(offset_of!(Mutator, status) == 0x58);
    assert!(offset_of!(Mutator, fenced_ap) == 0x60);
    assert!(offset_of!(Mutator, quiesce_epoch) == 0x68);
    assert!(offset_of!(Mutator, handshake) == 0x70);
    assert!(offset_of!(Mutator, no_gc_depth) == 0x74);
    assert!(offset_of!(Mutator, driver_out) == 0x78);
    assert!(size_of::<Mutator>() == 0x80);
    assert!(align_of::<Mutator>() == 128);
};

impl Mutator {
    /// The offset of `reg_limit`, which a poll reads (`GcAttrs::poll`).
    pub const REG_LIMIT_OFFSET: u16 = std::mem::offset_of!(Mutator, reg_limit) as u16;

    /// `reg_limit` while nothing is posted and the VM has no stack limit:
    /// the largest value, so that a frame entry's `top > reg_limit` never
    /// fires on it.
    pub const NO_LIMIT: usize = usize::MAX;

    /// A carrier with nothing posted and every reserved word zero.
    pub fn new() -> Self {
        Self {
            ap: Cell::new(0),
            alloc_limit: Cell::new(0),
            meta_bias: 0,
            remset_cur: Cell::new(std::ptr::null_mut()),
            remset_soft: Cell::new(std::ptr::null_mut()),
            reg_top: Cell::new(std::ptr::null_mut()),
            reg_limit: AtomicUsize::new(Self::NO_LIMIT),
            event: AtomicU32::new(0),
            pending: Cell::new(0),
            ticks: Cell::new(0),
            barrier_mode: 0,
            safepoint_state: AtomicU8::new(0),
            thread: Cell::new(std::ptr::null_mut()),
            heap: std::ptr::null(),
            status: Cell::new(0),
            fenced_ap: 0,
            quiesce_epoch: 0,
            handshake: AtomicU32::new(0),
            no_gc_depth: Cell::new(0),
            driver_out: Cell::new(false),
        }
    }

    /// Whether a poll has work: `reg_limit` reads zero, so something is
    /// posted. The one load a poll's fast path makes.
    #[inline]
    pub fn poll_due(&self) -> bool {
        self.reg_limit.load(Ordering::Relaxed) == 0
    }

    /// Post `bits` from the owner: an allocation that crossed its threshold,
    /// a collection asked for where it cannot run.
    #[inline]
    pub fn post(&self, bits: u32) {
        self.pending.set(self.pending.get() | bits);
        self.reg_limit.store(0, Ordering::Relaxed);
    }

    /// Post `bits` from anywhere: a signal handler, a timer, a profiler.
    /// Lock-free, allocation-free and panic-free, so a signal handler may
    /// call it.
    pub fn post_remote(&self, bits: u32) {
        self.event.fetch_or(bits, Ordering::SeqCst);
        self.reg_limit.store(0, Ordering::SeqCst);
    }

    /// Restore the poll word to `limit` once a poll has serviced what it
    /// could, unless something is still posted. The fence orders the store
    /// before the re-read, against a remote post's two stores, so a post
    /// that lands in between is never lost (§12).
    pub fn set_limit(&self, limit: usize) {
        self.reg_limit.store(limit, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        if self.pending.get() != 0 || self.event.load(Ordering::Relaxed) != 0 {
            self.reg_limit.store(0, Ordering::Relaxed);
        }
    }

    /// Whether a collection is posted.
    #[inline]
    pub fn gc_posted(&self) -> bool {
        self.pending.get() & event::GC != 0
    }

    /// A collection ran: take its posts, and restore the poll word unless
    /// something else is still posted.
    pub(crate) fn collected(&self) {
        self.pending.set(self.pending.get() & !event::GC);
        self.set_limit(Self::NO_LIMIT);
    }

    /// The open [`NoGcScope`](crate::NoGcScope)s: zero where a collection
    /// may run.
    #[inline]
    pub fn no_gc_depth(&self) -> u32 {
        self.no_gc_depth.get()
    }

    /// A `NoGcScope` opened: the depth before it.
    pub(crate) fn enter_no_gc(&self) -> u32 {
        let depth = self.no_gc_depth.get();
        self.no_gc_depth.set(depth + 1);
        depth
    }

    /// A `NoGcScope` closed: the depth after it. Checked in every build, not
    /// only in check builds: it runs once per scope, off every hot path, and
    /// a release build that underflowed here would wrap to `u32::MAX` and
    /// never collect again, with no report.
    pub(crate) fn exit_no_gc(&self) -> u32 {
        let depth = self.no_gc_depth.get();
        assert!(
            depth > 0,
            "unbalanced GC defer: exit without a matching enter"
        );
        self.no_gc_depth.set(depth - 1);
        depth - 1
    }

    /// Take the carrier's [`GcDriver`](crate::GcDriver): whether it was free.
    pub(crate) fn take_driver(&self) -> bool {
        !self.driver_out.replace(true)
    }

    /// Give the carrier's driver back.
    pub(crate) fn return_driver(&self) {
        self.driver_out.set(false);
    }
}

impl Default for Mutator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Mutator, event};

    #[test]
    fn a_post_makes_the_poll_due_until_it_is_serviced() {
        let m = Mutator::new();
        assert!(!m.poll_due() && !m.gc_posted());
        m.post(event::GC_MAJOR);
        assert!(m.poll_due() && m.gc_posted());
        m.collected();
        assert!(!m.poll_due() && !m.gc_posted());
    }

    #[test]
    fn servicing_one_event_leaves_the_poll_due_for_another() {
        let m = Mutator::new();
        m.post(event::GC_MAJOR | event::FINALIZERS_PENDING);
        m.collected();
        assert!(!m.gc_posted());
        assert!(m.poll_due(), "the finalizers are still posted");
    }

    #[test]
    fn a_remote_post_survives_the_limit_being_restored() {
        let m = Mutator::new();
        m.post_remote(event::SIGNAL);
        m.set_limit(Mutator::NO_LIMIT);
        assert!(
            m.poll_due(),
            "set_limit re-reads the events after storing the limit"
        );
    }
}
