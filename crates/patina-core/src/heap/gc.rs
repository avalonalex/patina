//! Garbage collection infrastructure (`docs/GC_DESIGN.md`).
//!
//! Non-moving stop-the-world mark-and-sweep over the typed arenas. Mark state
//! lives in side bit-vectors (no `TaggedValue` or object-header changes);
//! sweep pushes dead slots onto the existing free lists that `alloc_*` already
//! drains, and tombstones each slot so `Rc` payloads drop at sweep time —
//! that eager drop is what breaks closure ↔ environment cycles (design §8).
//!
//! Pluggability is split across two seams:
//! - [`GcRoots`] — implemented by anything that owns live values (backend
//!   state, registries, transient loop state). Backends provide roots and
//!   safe points; they never implement collection.
//! - `Collector` — the swappable algorithm. `MarkSweepCollector` is the
//!   v1 implementation shared by both backends. Implementations must be
//!   non-moving: live slots may never be relocated. The whole mark phase —
//!   root tracing, the weak-table fixpoint, weak-entry pruning — is
//!   `run_mark_phase`; a collector composes around it
//!   (`run_mark_phase` → `Heap::sweep`) and cannot mis-order its interior.
//!
//! Collection never happens on its own: a backend drives it by calling
//! [`GcController::safe_point`] at a point where every live value is
//! reachable from the roots it supplies. Whether that collects is decided
//! *ahead of time*, where the answer changes: `Heap::note_alloc` raises a
//! shared pending flag when allocation crosses the [`GcMode`]-derived
//! threshold — in bytes for the adaptive default, in allocations for stress,
//! never for `Off` (`heap/account.rs`, #606) — `Heap::request_gc` raises it
//! for a request posted from elsewhere, and the safe point itself is a single
//! flag load (design §6.1).
//!
//! The other way a collection starts is a primitive asking for one at its
//! call (`(gc)`, through `patina_primitives::Step::Collect`, #639): the
//! machine suspends the caller at the call's return pc and runs
//! [`GcController::collect_at_call`] before the caller's next instruction,
//! or posts the request where collection is deferred.
//!
//! `safe_point` and `collect_at_call` are the only ways in from outside this
//! crate. The collector, the mark phase, `Heap::sweep` and
//! `GcController::collect` are crate-private (#624), so no backend or host
//! can collect without the deferral rule both enforce. The one exception is
//! `collect_for_tests`, for tests that drive the collector against
//! hand-built state, and it exists only under the `test-support` feature,
//! which only `patina-vm`'s tests enable.
//!
//! The deferral protocol is asserted, not only described (#624, design §7):
//! a collection runs only while the collecting loop's own [`GcDeferGuard`]
//! is the only one alive; a holder's guard ([`GcDeferGuard::holding`])
//! panics on drop if a collection ran inside its extent; and every poll site
//! panics inside an [`AssertNoGc`] scope, a window whose soundness rests on
//! reaching no safe point. These three are compiled into check builds
//! ([`GC_CHECK`]) only; the defer balance
//! (`Heap::exit_gc_defer`) is checked in every build.

use std::rc::Rc;
use std::time::Instant;

use rustc_hash::FxHashSet;

use std::cell::{Cell, RefCell};

use super::account::{
    self, GcThreshold, OBJECT_SLOT_BYTES, PAIR_SLOT_BYTES, STRING_SLOT_BYTES, VECTOR_SLOT_BYTES,
};
use super::check::SlotChecks;
use super::{GC_CHECK, Heap, HeapObjectData, PromiseState, SharedHeap};
use crate::compiled_macro::{CompiledMacro, CompiledRule};
use crate::cont_value::{ContEnv, ContValue, ExceptionHandler, PromptFrame};
use crate::continuation::{CpsContinuation, DynamicWindRecord, WindRecord};
use crate::environment::{Environment, GcEdge};
use crate::library::Library;
use crate::procedure::Procedure;
use crate::tagged_value::{HeapIndex, ObjectIndex, TaggedValue};

// ============================================================================
// Bit-vectors
// ============================================================================

/// A fixed-size bit set, one bit per arena slot.
#[derive(Debug)]
pub(crate) struct BitSet {
    words: Vec<u64>,
    len: usize,
}

impl BitSet {
    fn new(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(64)],
            len,
        }
    }

    /// Set bit `i`. Returns `true` if the bit was previously clear.
    #[inline]
    fn set(&mut self, i: usize) -> bool {
        debug_assert!(i < self.len, "bit index {i} out of range {}", self.len);
        let word = &mut self.words[i / 64];
        let mask = 1u64 << (i % 64);
        let was_clear = *word & mask == 0;
        *word |= mask;
        was_clear
    }

    #[inline]
    fn get(&self, i: usize) -> bool {
        debug_assert!(i < self.len, "bit index {i} out of range {}", self.len);
        self.words[i / 64] & (1u64 << (i % 64)) != 0
    }

    fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// Mark bits for all four arenas, sized to arena lengths at collection start,
/// and the payload bytes of the objects marked. Produced by
/// [`GcVisitor::finish`], consumed by `Heap::sweep`.
#[derive(Debug)]
pub struct MarkBits {
    pub(crate) pairs: BitSet,
    pub(crate) vectors: BitSet,
    pub(crate) strings: BitSet,
    pub(crate) objects: BitSet,
    /// The payloads of the marked objects (`heap/account.rs`), counted as
    /// marking reaches each one: a vector's elements when it is traced, a
    /// string's characters when it is marked, an object's payload when it
    /// is traced, an interned symbol's name when the visitor roots it.
    pub(crate) live_payload: usize,
    /// The part of `live_payload` that is tree-walker closures'
    /// (`account::cps_lambda_payload`), from which sweep credits the dead
    /// closures' in one sum.
    pub(crate) live_closures: usize,
}

impl MarkBits {
    /// Whether `tv`'s cell is marked, or `None` if no arena tracks it.
    ///
    /// The one place the tag → arena mapping is written for *reading*.
    /// `GcVisitor::visit` writes the same mapping but its arms differ per
    /// arena, so the two are not one function; what this buys is that a reader
    /// cannot disagree with a writer about which arena a tag belongs to, or
    /// about what the fall-through means.
    fn is_marked(&self, tv: TaggedValue) -> Option<bool> {
        if tv.is_pair() {
            Some(self.pairs.get(tv.heap_index() as usize))
        } else if tv.is_vector() {
            Some(self.vectors.get(tv.heap_index() as usize))
        } else if tv.is_string() {
            Some(self.strings.get(tv.heap_index() as usize))
        } else if tv.is_object() {
            Some(self.objects.get(tv.heap_index() as usize))
        } else {
            None
        }
    }

    fn for_heap(heap: &Heap) -> Self {
        Self {
            pairs: BitSet::new(heap.pairs.len()),
            vectors: BitSet::new(heap.vectors.len()),
            strings: BitSet::new(heap.strings.len()),
            objects: BitSet::new(heap.objects.len()),
            live_payload: 0,
            live_closures: 0,
        }
    }

    /// The bytes marking found live: each marked slot, and the payloads of
    /// the marked objects. Read it before `Heap::sweep`, as
    /// [`MarkBits::marked`].
    pub fn live_bytes(&self) -> usize {
        let marked = self.marked();
        marked.pairs * PAIR_SLOT_BYTES
            + marked.vectors * VECTOR_SLOT_BYTES
            + marked.strings * STRING_SLOT_BYTES
            + marked.objects * OBJECT_SLOT_BYTES
            + self.live_payload
    }

    /// Live slots per arena. Read this before `Heap::sweep`, which reuses the
    /// mark bits as scratch space.
    pub fn marked(&self) -> ArenaCounts {
        ArenaCounts {
            pairs: self.pairs.count_ones(),
            vectors: self.vectors.count_ones(),
            strings: self.strings.count_ones(),
            objects: self.objects.count_ones(),
        }
    }
}

// ============================================================================
// Stats
// ============================================================================

/// Per-arena slot counts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArenaCounts {
    pub pairs: usize,
    pub vectors: usize,
    pub strings: usize,
    pub objects: usize,
}

impl ArenaCounts {
    pub fn total(&self) -> usize {
        self.pairs + self.vectors + self.strings + self.objects
    }
}

/// Cumulative collector statistics plus a snapshot of the last collection.
#[derive(Debug, Clone, Copy, Default)]
pub struct GcStats {
    /// Number of collections performed.
    pub collections: u64,
    /// Slots found live in the last collection.
    pub last_marked: ArenaCounts,
    /// Slots reclaimed in the last collection.
    pub last_swept: ArenaCounts,
    /// Duration of the last collection in microseconds.
    pub last_pause_micros: u128,
}

// ============================================================================
// Traits
// ============================================================================

/// Implemented by anything that owns live values: backend state, shared
/// registries, and transient roots (e.g. the tree-walker's in-flight
/// `StepResult`, passed per collection).
pub trait GcRoots {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>);

    /// One round of the weak-table fixpoint (design §9.5), driven by
    /// `run_mark_phase`: `ids` are continuation ids whose ref objects were
    /// just proven live by marking — trace the payloads this provider keys
    /// by them (ids it does not own are simply skipped). A payload may mark
    /// further ref objects; the driver re-drains and broadcasts the next
    /// batch until none remain. Ids are heap-unique (`Heap` mints them), so
    /// every provider may receive every batch.
    ///
    /// The default is a no-op for providers with no weak tables.
    fn trace_weak_ids(&self, _ids: &[u64], _visitor: &mut GcVisitor<'_>) {}

    /// Called once per collection after marking reaches its weak fixpoint:
    /// drop weak-table entries whose keys did not survive
    /// (`GcVisitor::weak_continuation_id_is_live`). Runs before
    /// `Heap::sweep`; pruning must not touch the heap — it may only drop
    /// side-table payloads that live outside the arenas.
    fn sweep_weak(&self, _visitor: &GcVisitor<'_>) {}
}

/// A pluggable collection algorithm. Implementations must be **non-moving**:
/// live slots may never be relocated, because raw arena indices escape
/// `TaggedValue` (symbol table, `SourceMap` keys, `eq?` semantics, VM
/// constants — design §3.4).
///
/// A collector expresses its automatic policy as a threshold
/// ([`GcThreshold`]: bytes for the adaptive default,
/// `MarkSweepCollector::auto_threshold`; allocations for stress), which
/// [`GcController`] installs into the heap — the trigger *decision* happens
/// in `Heap::note_alloc`, not by querying the collector (design §6.1).
///
/// Crate-private, with every other way to run a collection (#624): a
/// collection outside [`GcController::safe_point`] skips the deferral rule,
/// which is the one thing that makes a collection sound.
pub(crate) trait Collector {
    /// Run a full collection. The caller must be at a safe point: every live
    /// value reachable from `roots`, and no outstanding heap borrow other
    /// than the one behind `heap`.
    fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats;
}

// ============================================================================
// Deferral
// ============================================================================

/// RAII guard marking a scope that holds live values no root provider can
/// see — a nested trampoline, or a Rust loop holding unevaluated forms across
/// an evaluation call. Backends refuse to collect while an *outer* guard is
/// alive (design §7).
///
/// Drop takes a mutable heap borrow, so a guard must not be dropped while a
/// heap borrow is outstanding.
///
/// Owns its `SharedHeap` handle rather than borrowing one, so a guard can be
/// stored in a struct whose lifetime *is* the deferral extent — see
/// `ParsedLibrary`, which holds unevaluated forms and therefore defers for as
/// long as it exists. The `Rc` clone is paid once per guard, on paths taken
/// per dispatch-loop entry rather than per step.
///
/// Two kinds, by what the guard protects (#624):
/// - a **loop's** own guard ([`GcDeferGuard::new`]), which every dispatch
///   loop and trampoline takes for its extent. The outermost one is the only
///   guard that may be alive when a collection runs: `safe_point` asserts
///   that in check builds, and `collect_at_call` collects only then.
/// - a **holder's** guard ([`GcDeferGuard::holding`]), taken by a Rust scope
///   or value that holds heap values across an evaluation call. No collection
///   may run inside its extent at all, and in check builds its drop asserts
///   that none did. Today that is guaranteed by the first kind: every loop
///   entered under a holder is nested. A nested loop that is later allowed
///   to collect (GC_PRD stage 4e) then fails at the holder that needed the
///   deferral, rather than freeing what it holds.
pub struct GcDeferGuard {
    heap: SharedHeap,
    /// Defer depth observed on entry. Zero means nothing outer is deferring.
    outer_depth: u32,
    /// Whether this is a holder's guard ([`GcDeferGuard::holding`]), which
    /// the heap counts apart from the depth ([`Heap::gc_defer_is_one_loop`]).
    holder: bool,
    /// A holder's guard, in a check build: the heap's collection count when
    /// it was taken, which must not have moved when it drops.
    collections_at_entry: Option<u64>,
}

impl GcDeferGuard {
    /// A dispatch loop's guard, for the loop's own extent.
    pub fn new(heap: &SharedHeap) -> Self {
        Self::enter(heap, false)
    }

    /// A holder's guard: for a scope or value that keeps heap values no root
    /// provider sees across a call that can evaluate — a library's
    /// unevaluated body (`ParsedLibrary`), a form being expanded while its
    /// imports load (`desugar_with_imports`), the global environment a swap
    /// set aside (`VmState::with_globals`).
    ///
    /// Defers like [`GcDeferGuard::new`]. In a check build it also records
    /// how many collections the heap has run, and its drop panics if that
    /// number has changed: a collection inside the extent could have freed
    /// what the holder holds.
    ///
    /// On a machine it does not reach the values a primitive holds across
    /// `ApplyContext::apply_proc`, which takes no guard of its own there: the
    /// nested loop's guard is the only deferral. The tree-walker's detached
    /// `ApplyContext for Evaluator`, which no loop runs above, takes one of
    /// these in each of its methods instead (#622).
    pub fn holding(heap: &SharedHeap) -> Self {
        Self::enter(heap, true)
    }

    fn enter(heap: &SharedHeap, holder: bool) -> Self {
        let (outer_depth, collections) = {
            let mut h = heap.borrow_mut();
            let depth = h.gc_defer_depth();
            h.enter_gc_defer(holder);
            (depth, h.gc_collections())
        };
        Self {
            heap: heap.clone(),
            outer_depth,
            holder,
            collections_at_entry: (holder && GC_CHECK).then_some(collections),
        }
    }

    /// Whether this guard is the outermost one — i.e. nothing else was
    /// deferring when it was taken, so a safe point inside it may collect.
    ///
    /// Both backends guard every loop they run — the VM each
    /// `run_loop_until_outcome`, the tree-walker each `run_trampoline` — so
    /// the outermost loop runs at defer depth 1, under its own guard alone.
    /// A loop asks this once, at entry, and hoists the answer out of its
    /// safe point, so it is not the whole rule: a guard a callee took later
    /// and kept past its instruction would not change it. When a collection
    /// runs, the depth is checked as well. `safe_point` asserts it is 1 in a
    /// check build (#624); `collect_at_call`, which runs inside an
    /// instruction or a step and is handed no guard, requires it, with no
    /// holder's guard among the one alive ([`Heap::gc_defer_is_one_loop`]).
    pub fn is_outermost(&self) -> bool {
        self.outer_depth == 0
    }
}

impl Drop for GcDeferGuard {
    fn drop(&mut self) {
        let collections = {
            let mut h = self.heap.borrow_mut();
            h.exit_gc_defer(self.holder);
            h.gc_collections()
        };
        // Not while unwinding: a second panic would abort the process and
        // lose the first one's message.
        if GC_CHECK
            && let Some(at_entry) = self.collections_at_entry
            && collections != at_entry
            && !std::thread::panicking()
        {
            collected_inside_a_holder(collections - at_entry);
        }
    }
}

/// The panic of a holder's guard, out of line: the drop runs on every
/// library load and every expanded form.
#[cold]
#[inline(never)]
fn collected_inside_a_holder(collections: u64) -> ! {
    panic!(
        "GC deferral violated: {collections} collection(s) ran inside a \
         GcDeferGuard::holding extent, whose holder keeps heap values no root \
         provider sees (docs/GC_DESIGN.md §7)"
    )
}

/// The panic of `safe_point`'s depth check.
#[cold]
#[inline(never)]
fn collected_under_another_guard(depth: u32) -> ! {
    panic!(
        "GC deferral violated: a collection ran with {depth} defer guard(s) \
         alive; only the collecting loop's own may be (docs/GC_DESIGN.md §7)"
    )
}

// ============================================================================
// No-collection windows
// ============================================================================

/// RAII scope over a window that must not reach a GC poll: a stretch of code
/// whose soundness a comment argues from where the safe points are, such as
/// a value held only in a Rust local until the next write, or a weak-table
/// entry and its handle not yet both reachable (#624, design §7). Every poll
/// site asserts that no scope is open ([`NoGcScopes::assert_none_open`]), so a
/// poll added inside a window, or a dispatch loop entered from one, panics
/// at once, even while it is nested and cannot collect, instead of freeing a
/// live value on the day it can.
///
/// The rule it carries: a comment that argues "no safe point here" comes
/// with an `AssertNoGc` over the same lines (AGENTS.md).
///
/// A depth counter on the heap, maintained in check builds
/// ([`GC_CHECK`]) only: a plain release build keeps no count
/// and checks nothing. The scope holds its own handle on the counter, so
/// opening it takes one shared heap borrow and closing it takes none; it may
/// be dropped while the heap is borrowed.
///
/// To end a window partway through a function, pass the scope on by value and
/// drop it where the window closes, as the VM's stub pushers do once they
/// have written a transfer's operands back.
#[must_use = "an AssertNoGc covers the extent of the binding that holds it"]
pub struct AssertNoGc {
    open: Option<Rc<Cell<u32>>>,
}

impl AssertNoGc {
    /// Open a window. Must not be called while the heap is mutably borrowed.
    #[inline]
    pub fn new(heap: &SharedHeap) -> Self {
        let open = GC_CHECK.then(|| {
            let open = heap.borrow().no_gc_scopes.clone();
            open.set(open.get() + 1);
            open
        });
        Self { open }
    }
}

impl Drop for AssertNoGc {
    #[inline]
    fn drop(&mut self) {
        if GC_CHECK && let Some(open) = &self.open {
            open.set(open.get() - 1);
        }
    }
}

/// The poll-site half of [`AssertNoGc`]: the heap's count of open scopes,
/// taken once at loop entry like the pending flag, so a poll's check is one
/// load and one compare in a check build and nothing in a plain release.
pub struct NoGcScopes {
    open: Option<Rc<Cell<u32>>>,
}

impl NoGcScopes {
    pub fn of(heap: &SharedHeap) -> Self {
        Self {
            open: GC_CHECK.then(|| heap.borrow().no_gc_scopes.clone()),
        }
    }

    /// Panic if an [`AssertNoGc`] scope is open. Call at every poll site,
    /// before its safe point, on every iteration: not inside
    /// [`GcController::safe_point`], which returns before it would get there
    /// unless a collection is pending and the loop is outermost, so a check
    /// there would almost never run.
    #[inline(always)]
    pub fn assert_none_open(&self) {
        if GC_CHECK && let Some(open) = &self.open {
            let n = open.get();
            if n != 0 {
                polled_inside_a_no_gc_scope(n);
            }
        }
    }
}

#[cold]
#[inline(never)]
fn polled_inside_a_no_gc_scope(open: u32) -> ! {
    panic!(
        "GC poll inside an AssertNoGc scope ({open} open): a safe point was \
         reached in a window whose soundness assumes none (docs/GC_DESIGN.md §7)"
    )
}

// ============================================================================
// Policy
// ============================================================================

/// What kind of collection a primitive asks for at its call
/// (`patina_primitives::Step::Collect`, #639). Today's collector has one:
/// every collection is a full one. GC_PRD's generational heaps add a minor;
/// a match on this type is where each backend then learns of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectKind {
    /// A full collection: everything unreachable is reclaimed, and every
    /// ephemeron whose key is unreachable is broken. What `(gc)` asks for.
    Major,
}

/// When automatic collection fires. Shared by both backends: `PATINA_GC` and
/// `PATINA_GC_STRESS` are process-global, so the mode table lives here rather
/// than being re-derived per backend (design §6).
///
/// A backend reads it from the environment when it is made
/// ([`GcMode::from_env`]). A test that compares collecting with not
/// collecting names its mode instead, through the backends' `with_gc_mode`
/// constructors, so that a stress lane's variable does not turn its
/// not-collecting side into a collecting one (#626).
#[derive(Clone, Copy, Debug)]
pub enum GcMode {
    /// Collect only when `(gc)` has been called. **Testing lanes only**
    /// (`PATINA_GC=0`): the differential suite needs a no-collection
    /// reference run to diff the collecting modes against. Not a supported
    /// user configuration — Patina always runs with GC.
    Off,
    /// Collect on the collector's adaptive threshold, in bytes: after
    /// `max(8 MiB, 2·L)` of allocation (#606). The default since stage 4c:
    /// the §6.1 trigger redesign made the standing cost of an enabled
    /// collector indistinguishable from off, so enabling costs only the
    /// pauses themselves.
    On,
    /// Collect once `n` allocations have happened since the last collection,
    /// whatever their size, ignoring the adaptive floor. The
    /// differential-testing lane.
    Stress(usize),
    /// Collect at every outermost safe point, allocation or not
    /// (`PATINA_GC_ZEAL=entry`, #625). Its threshold is 0, so the pending
    /// flag is raised again as soon as a collection re-installs it. Stress
    /// cannot express this: it counts allocations, so a stretch of
    /// instructions that allocates nothing never collects under it, even at
    /// `PATINA_GC_STRESS=1`. A torture lane, about 7x slower than stress 1:
    /// it runs on a subset of the suite (`scripts/run_gc_zeal.sh`).
    Zeal,
}

impl GcMode {
    /// Read the mode from the environment. Adaptive collection is always on;
    /// the variables exist for the differential test lanes
    /// (`docs/GC_DESIGN.md` §11): `PATINA_GC=0` produces the no-collection
    /// reference run, `PATINA_GC_STRESS` takes an optional allocation
    /// count (`=1`, the default, collects at nearly every safe point that
    /// follows an allocation; larger values trade coverage for runtime), and
    /// `PATINA_GC_ZEAL=entry` collects at every outermost safe point.
    ///
    /// Zeal wins over stress, and stress over `PATINA_GC=0`. `entry` is the
    /// one zeal mode today's collector has, spelled as GC_PRD §14 spells it;
    /// the PRD's others (`major`, `minor`, `alternate`, `move-all`) come with
    /// the redesign. Any other value panics rather than run a torture lane
    /// that is quietly not torturing anything.
    pub fn from_env() -> Self {
        fn flag(name: &str) -> Option<String> {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty() && v != "0")
        }

        if let Some(v) = flag("PATINA_GC_ZEAL") {
            match v.as_str() {
                "entry" => GcMode::Zeal,
                other => panic!(
                    "PATINA_GC_ZEAL={other}: today's collector has one zeal mode, \
                     `entry` (collect at every outermost safe point); GC_PRD §14's \
                     others come with the redesign"
                ),
            }
        } else if let Some(v) = flag("PATINA_GC_STRESS") {
            GcMode::Stress(v.parse().unwrap_or(1).max(1))
        } else if matches!(std::env::var("PATINA_GC").as_deref(), Ok("0")) {
            GcMode::Off
        } else {
            GcMode::On
        }
    }
}

/// The variable that sets each mode, spelled as a lane sets it: what the
/// collection-count record (`PATINA_GC_COUNT_DIR`) says the process ran under.
impl std::fmt::Display for GcMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GcMode::Off => f.write_str("PATINA_GC=0"),
            GcMode::On => f.write_str("adaptive"),
            GcMode::Stress(n) => write!(f, "PATINA_GC_STRESS={n}"),
            GcMode::Zeal => f.write_str("PATINA_GC_ZEAL=entry"),
        }
    }
}

/// A GC mode paired with the collector instance a backend owns.
///
/// Shared by both backends so the mode table, the `(gc)`-request rule, and the
/// env-var grammar have one implementation rather than one per backend.
pub struct GcController {
    mode: GcMode,
    collector: MarkSweepCollector,
}

impl GcController {
    /// A controller in the mode the environment selects: what every backend
    /// uses unless a test names a mode.
    pub fn from_env() -> Self {
        Self::new(GcMode::from_env())
    }

    /// A controller in `mode`, whatever the environment says: for the
    /// backends' `with_gc_mode` constructors, which a test that compares
    /// collecting with not collecting uses (#626). Not an interface: an
    /// embedder's interpreter collects in the mode the environment selects.
    #[doc(hidden)]
    pub fn new(mode: GcMode) -> Self {
        count_log::open_if_asked();
        Self {
            mode,
            collector: MarkSweepCollector::new(),
        }
    }

    /// Name `backend` in the collection-count record (`PATINA_GC_COUNT_DIR`)
    /// as one this process made, so that the nightly stress lane can check
    /// that its tree-walker job ran the tree-walker (#626). Each backend
    /// calls it when it makes its controller. Not an interface.
    #[doc(hidden)]
    pub fn note_backend(backend: &'static str) {
        count_log::note_backend(backend);
    }

    /// The threshold at which `Heap::note_alloc` should raise the
    /// collection-pending flag — the mode made concrete, and the single owner
    /// of that mapping. A backend installs this into its heap when the pair
    /// is wired up (`Heap::set_gc_threshold`); `GcController::collect`
    /// re-installs it after each collection, the only point the adaptive
    /// term changes. A heap with no controller attached keeps its inert
    /// [`GcThreshold::NEVER`] default, where only `(gc)` raises the flag.
    ///
    /// The adaptive default counts bytes (#606); stress counts allocations,
    /// as its lanes' pinned collection counts assume. Every mode that
    /// collects on its own counts descriptor pressure too (#607,
    /// `heap/account.rs`); `PATINA_GC=0` does not, so its reference run
    /// collects only where `(gc)` or an open that ran out of descriptors
    /// asks to.
    pub fn current_threshold(&self) -> GcThreshold {
        let threshold = match self.mode {
            GcMode::Off => return GcThreshold::NEVER,
            GcMode::On => GcThreshold::bytes(self.collector.auto_threshold()),
            GcMode::Stress(n) => GcThreshold::allocations(n),
            // Already crossed: installing it raises the pending flag, and
            // `collect` re-installs it after every sweep.
            GcMode::Zeal => GcThreshold::allocations(0),
        };
        threshold.with_descriptors(account::descriptor_pressure_threshold())
    }

    /// Collect now, with no check of the deferral rule. Crate-private (#624):
    /// a backend collects through [`GcController::safe_point`].
    pub(crate) fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
        let stats = self.collector.collect(heap, roots);
        count_log::note_collection();
        // Sweep lowered the pending flag; re-arm the threshold that raises it.
        // This is what lets `note_alloc` compare against a stored number
        // instead of safe points re-deriving the policy per instruction.
        heap.set_gc_threshold(self.current_threshold());
        stats
    }

    /// Run a backend safe point.
    ///
    /// This owns the *rules* every backend must not restate: `(gc)` is honored
    /// even when automatic collection is off; only the outermost guard may
    /// collect; one `borrow_mut` spans the whole collection. Backends supply
    /// only what is genuinely theirs — their root set.
    ///
    /// `with_roots` receives a `collect` callback and decides whether to call
    /// it. That inversion lets a backend assemble roots that borrow from its
    /// own frame (a `Ref<LibraryRegistry>`, a transient step provider) and
    /// lets it *abort* — returning without calling `collect` — when a root is
    /// unavailable, which must never degrade into tracing a partial root set.
    /// The pending flag stays raised across an abort, so the next safe point
    /// retries.
    ///
    /// `pending` (the handle from `Heap::gc_pending_handle`) and
    /// `is_outermost` are loop invariants the caller hoists out of its
    /// dispatch loop; the fast path is one load and one branch — no `RefCell`
    /// borrow, no mode dispatch. The collection *decision* was already made
    /// where it becomes true, in `Heap::note_alloc` / `Heap::request_gc`
    /// (design §6.1).
    ///
    /// Returns whether a collection ran, for a backend with work to do after
    /// one (the VM's code release, #338). It cannot tell from the pending
    /// flag: under [`GcMode::Zeal`] the flag is raised again before the
    /// collection returns.
    #[inline]
    pub fn safe_point(
        gc: &RefCell<Self>,
        heap: &SharedHeap,
        pending: &Cell<bool>,
        is_outermost: bool,
        with_roots: impl FnOnce(&mut dyn FnMut(&[&dyn GcRoots])),
    ) -> bool {
        // A nested loop never collects — its caller holds live values in Rust
        // locals no root provider can see. Its flag check would be dead code,
        // but `is_outermost` is a hoisted constant, so the branch predicts.
        if !is_outermost || !pending.get() {
            return false;
        }
        Self::safe_point_cold(gc, heap, with_roots)
    }

    /// The rare branch, out of line so only the fast path inlines into the
    /// caller's dispatch loop.
    #[inline(never)]
    fn safe_point_cold(
        gc: &RefCell<Self>,
        heap: &SharedHeap,
        with_roots: impl FnOnce(&mut dyn FnMut(&[&dyn GcRoots])),
    ) -> bool {
        // A test's switch, absent from a build without `test-support`
        // (`Heap::set_skip_safe_points`, #639).
        #[cfg(feature = "test-support")]
        if heap.borrow().skips_safe_points() {
            return false;
        }
        let mut collected = false;
        with_roots(&mut |roots| {
            let mut h = heap.borrow_mut();
            // The outermost loop's own guard, and no other. `is_outermost`
            // is read once, at loop entry, so a guard that a callee took and
            // kept past its instruction would not stop the running loop from
            // collecting under it; this does (#624).
            if GC_CHECK {
                let depth = h.gc_defer_depth();
                if depth != 1 {
                    drop(h);
                    collected_under_another_guard(depth);
                }
            }
            gc.borrow_mut().collect(&mut h, roots);
            collected = true;
        });
        collected
    }

    /// Run the collection a primitive asked for at its call
    /// (`patina_primitives::Step::Collect`, #639): `(gc)`'s, and the
    /// collect-and-retry of an open that ran out of descriptors (#607). The
    /// machine calls it at a poll of its own, with the caller suspended at
    /// the call's return pc and every live value where `with_roots` finds
    /// it: the VM in `resume_stub`'s frame (`CollectAtCall`), the tree-walker
    /// at the top of its trampoline (`StepResult::CollectAtCall`). Returns
    /// whether a collection ran.
    ///
    /// It collects where a safe point may (#624): with one guard alive, and
    /// that one a loop's ([`Heap::gc_defer_is_one_loop`]: the defer depth at
    /// 1 and no holder's guard). Every loop takes a guard at entry, so that
    /// is the running loop's own, the outermost, which is what
    /// [`GcDeferGuard::is_outermost`] answers at a safe point; it is asked
    /// of the heap here because the poll runs inside an instruction or a
    /// step, not at the top of the loop. A holder's guard alone is no
    /// loop's: a host that holds values under one and calls this outside any
    /// loop gets the collection posted, not run inside the holder's extent.
    /// And with no [`AssertNoGc`] scope open, which it asserts in a check
    /// build, as every poll site does. It collects in every mode,
    /// `PATINA_GC=0` included, as `(gc)` always has.
    ///
    /// Where collection is deferred — a nested loop, a library body being
    /// loaded, a holder's extent — it posts the collection for the next safe
    /// point that may collect, as `(gc)` did everywhere before #639, and
    /// counts it ([`Heap::defer_collection`], `(gc-stats)`'s
    /// `deferred-collections`). So does a collection whose roots the
    /// backend cannot supply: `with_roots` returns without calling
    /// `collect`, as at a safe point, while a library load holds the
    /// registry.
    pub fn collect_at_call(
        gc: &RefCell<Self>,
        heap: &SharedHeap,
        kind: CollectKind,
        with_roots: impl FnOnce(&mut dyn FnMut(&[&dyn GcRoots])),
    ) -> bool {
        NoGcScopes::of(heap).assert_none_open();
        // The one kind there is: a full collection.
        let CollectKind::Major = kind;
        let mut collected = false;
        if heap.borrow().gc_defer_is_one_loop() {
            with_roots(&mut |roots| {
                let mut h = heap.borrow_mut();
                gc.borrow_mut().collect(&mut h, roots);
                collected = true;
            });
        }
        if !collected {
            heap.borrow_mut().defer_collection();
        }
        collected
    }
}

/// `PATINA_GC_COUNT_DIR`: a record of how many collections the process ran,
/// for a stress lane to assert that its run collected (#626).
///
/// A lane that passes with no collections has tested nothing, and four have
/// (#5, #164, #200, #201): the variable that sets the mode did not reach the
/// process, or the program allocated too little to cross the interval. Each
/// is invisible in the program's output. A test binary has no hook at its
/// exit and the CLI leaves through `process::exit`, so the record is
/// rewritten at every collection rather than written once at the end.
///
/// When the variable names a directory, the first collector the process
/// makes creates `gc-count.<pid>` there, holding one line,
/// `pid=<pid> exe=<executable> env=<mode> backends=<kinds> collections=<n>`,
/// `n` zero-padded to 20 digits. The record is written at once with a count
/// of zero, so a process that never collects leaves a zero rather than no
/// file, and a directory with no record means the variable never reached a
/// process. `env` is the mode the environment selects ([`GcMode`]'s
/// `Display`): a backend made with an explicit mode still counts, so a lane
/// pins a minimum well above what those reach. `backends` is the kinds of
/// backend the process has made, sorted and comma-separated (`vm`,
/// `tree-walker`), or `none`, so that a lane can check that it ran the
/// backend it meant to: the tree-walker's Larceny job would otherwise pass on
/// the VM, whose tallies are nearly all the same.
///
/// The count is of the whole process, every backend in it: a test binary
/// running tests on parallel threads writes one record. The cost when the
/// variable is set is a seek and a write per collection, small beside the
/// collection; unset, a `OnceLock` read.
mod count_log {
    use super::GcMode;
    use std::collections::BTreeSet;
    use std::io::{Seek, SeekFrom, Write};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    struct Log {
        file: std::fs::File,
        /// The record up to the backends, fixed for the process.
        prefix: String,
        backends: BTreeSet<&'static str>,
        collections: u64,
    }

    impl Log {
        fn write(&mut self) {
            let backends = if self.backends.is_empty() {
                "none".to_string()
            } else {
                self.backends.iter().copied().collect::<Vec<_>>().join(",")
            };
            let record = format!(
                "{}backends={backends} collections={:020}\n",
                self.prefix, self.collections
            );
            // A diagnostic: a failed write leaves an older record, which only
            // a lane reads, and it reads it as fewer collections. The length
            // is set after the write, since naming the first backend makes
            // the record shorter than the `none` before it.
            let _ = self
                .file
                .seek(SeekFrom::Start(0))
                .and_then(|_| self.file.write_all(record.as_bytes()))
                .and_then(|_| self.file.set_len(record.len() as u64));
        }
    }

    fn log() -> Option<&'static Mutex<Log>> {
        static LOG: OnceLock<Option<Mutex<Log>>> = OnceLock::new();
        LOG.get_or_init(open).as_ref()
    }

    fn open() -> Option<Mutex<Log>> {
        // An empty value is what a shell script makes of an unset variable;
        // the same guard `GcMode::from_env` uses.
        let dir = std::env::var("PATINA_GC_COUNT_DIR")
            .ok()
            .filter(|v| !v.is_empty() && v != "0")?;
        let pid = std::process::id();
        // A new file, never an old one: a pid reused within one directory
        // gets a suffix rather than adding to another process's count.
        let mut suffix = String::new();
        let mut attempt = 0;
        let file = loop {
            let path = std::path::Path::new(&dir).join(format!("gc-count.{pid}{suffix}"));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => break file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                    attempt += 1;
                    suffix = format!(".{attempt}");
                }
                Err(e) => {
                    // Loud: a lane reads a missing record as a process that
                    // was never watched, and says so, but not why.
                    eprintln!("patina: PATINA_GC_COUNT_DIR={dir}: {}: {e}", path.display());
                    return None;
                }
            }
        };
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "unknown".to_string());
        let mut log = Log {
            file,
            prefix: format!("pid={pid} exe={exe} env={} ", GcMode::from_env()),
            backends: BTreeSet::new(),
            collections: 0,
        };
        log.write();
        Some(Mutex::new(log))
    }

    /// The record, if the variable asks for one. Never poison-panics: a
    /// record has no invariant a panic elsewhere can have broken.
    fn locked() -> Option<MutexGuard<'static, Log>> {
        log().map(|log| log.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Create the record, with no collections, if the variable asks for one.
    pub(super) fn open_if_asked() {
        log();
    }

    /// Count a collection, if the variable asks for a record.
    pub(super) fn note_collection() {
        if let Some(mut log) = locked() {
            log.collections += 1;
            log.write();
        }
    }

    /// Name a backend the process made, if the variable asks for a record.
    pub(super) fn note_backend(backend: &'static str) {
        if let Some(mut log) = locked()
            && log.backends.insert(backend)
        {
            log.write();
        }
    }
}

// ============================================================================
// Marking
// ============================================================================

/// Marking front-end handed to root providers.
///
/// Owns the mark bits and worklists; borrows the heap read-only for child
/// enumeration. Environments and continuations live outside the arenas as
/// `Rc` structs, so they get dedup sets keyed by pointer identity instead of
/// mark bits.
pub struct GcVisitor<'h> {
    heap: &'h Heap,
    marks: MarkBits,
    worklist: Vec<TaggedValue>,
    cont_worklist: Vec<Rc<CpsContinuation>>,
    cont_env_worklist: Vec<ContEnv>,
    /// The environments `visit_env` has been shown and not yet walked. Kept
    /// here between calls, always empty, so a walk allocates nothing.
    env_worklist: Vec<Rc<Environment>>,
    seen_envs: FxHashSet<usize>,
    seen_conts: FxHashSet<usize>,
    seen_exprs: FxHashSet<usize>,
    /// Dedup set for `Rc`-shared structures owned by root providers
    /// (see [`GcVisitor::visit_once`]).
    seen_shared: FxHashSet<usize>,
    /// Weak-key discovery for the VM's continuation side tables (design
    /// §9.5): every id whose `VmContinuationRef` /
    /// `VmDelimitedContinuationRef` heap object has been marked. One
    /// namespace for both kinds — `Heap` mints their ids from one counter.
    /// The queue holds the subset [`run_mark_phase`] has not yet broadcast
    /// to [`GcRoots::trace_weak_ids`].
    live_weak_ids: FxHashSet<u64>,
    new_weak_ids: Vec<u64>,
    /// Ephemerons reached during marking, awaiting the fixpoint in
    /// [`run_mark_phase`]. An entry leaves when its key turns out to be
    /// reachable; whatever is left at the end has a dead key and is broken.
    pending_ephemerons: Vec<TaggedValue>,
}

impl<'h> GcVisitor<'h> {
    pub fn new(heap: &'h Heap) -> Self {
        let mut marks = MarkBits::for_heap(heap);
        // Interned symbols are roots in v1 (design §9.2). Rooted here rather
        // than by the collector because a dangling intern-table index would
        // break any collector — it is a heap invariant, not policy. Mark-only:
        // symbol_table entries are Symbol leaves by construction, so the
        // worklist round-trip would be pure overhead. Never traced, so their
        // payload, the name, is counted here: the key is the same name.
        for (name, &idx) in &heap.symbol_table {
            marks.objects.set(idx as usize);
            marks.live_payload += name.len();
        }
        // Syntactic-keyword markers are roots on the same terms: a marker is
        // the identity of a form, so collecting one would let the next intern
        // mint a different object for the same keyword. Leaves too.
        for &idx in heap.core_syntax_table.values() {
            marks.objects.set(idx as usize);
        }
        Self {
            heap,
            marks,
            worklist: Vec::new(),
            cont_worklist: Vec::new(),
            cont_env_worklist: Vec::new(),
            env_worklist: Vec::new(),
            seen_envs: FxHashSet::default(),
            seen_conts: FxHashSet::default(),
            seen_exprs: FxHashSet::default(),
            seen_shared: FxHashSet::default(),
            live_weak_ids: FxHashSet::default(),
            new_weak_ids: Vec::new(),
            pending_ephemerons: Vec::new(),
        }
    }

    /// The normal edge: mark and enqueue a heap reference; no-op for
    /// immediates (fixnum, special, char). Strings are leaves, so they are
    /// marked without a worklist round-trip.
    ///
    /// In a check build a reference whose slot was freed and reused since it
    /// was made panics here (`heap/check.rs`): a root or a traced edge kept a
    /// dead object's reference, and marking would otherwise retain the slot's
    /// new tenant in its name. A reference to a slot that is free now is
    /// reported by sweep's pre-mark check instead.
    #[inline]
    pub fn visit(&mut self, tv: TaggedValue) {
        let heap = self.heap;
        let newly_marked = if tv.is_pair() {
            heap.pair_checks.check_reached("pair", tv);
            self.marks.pairs.set(tv.heap_index() as usize)
        } else if tv.is_vector() {
            heap.vector_checks.check_reached("vector", tv);
            self.marks.vectors.set(tv.heap_index() as usize)
        } else if tv.is_string() {
            heap.string_checks.check_reached("string", tv);
            let index = tv.heap_index() as usize;
            if self.marks.strings.set(index) {
                self.marks.live_payload += account::string_payload(&heap.strings[index]);
            }
            false
        } else if tv.is_object() {
            heap.object_checks.check_reached("object", tv);
            self.marks.objects.set(tv.heap_index() as usize)
        } else {
            false
        };
        if newly_marked {
            self.worklist.push(tv);
        }
    }

    /// Convenience for register files / buffers.
    pub fn visit_slice(&mut self, values: &[TaggedValue]) {
        for &tv in values {
            self.visit(tv);
        }
    }

    /// For bare object-arena indices that are not stored as `TaggedValue`
    /// (e.g. the VM's `CallFrame.closure`). Checked as [`Self::visit`]
    /// checks a value, against the generation the index kept.
    pub fn visit_object_index(&mut self, index: ObjectIndex) {
        let value = index.value();
        self.heap.object_checks.check_reached("object", value);
        if self.marks.objects.set(index.index() as usize) {
            self.worklist.push(value);
        }
    }

    /// Whether `tv` is reachable independently of the ephemeron holding it:
    /// either it is not a heap cell at all, or marking has already reached it.
    ///
    /// Private: only the ephemeron fixpoint in `run_mark_phase` may ask, and
    /// only there is the answer meaningful. A root provider calling it from
    /// `trace_roots` would get "not yet marked" for most live objects, because
    /// the worklist has barely drained — and skipping a trace on that basis is
    /// a use-after-free. The fixpoint must also not *cause* the key to be
    /// marked, which is the whole difference between a weak key and a strong
    /// one.
    fn value_is_live(&self, tv: TaggedValue) -> bool {
        // No arena tracks an immediate, and none tracks a closure either, so
        // both land on the default — conservative in the safe direction: the
        // pair survives a collection it might have been broken by, and never
        // the reverse.
        self.marks.is_marked(tv).unwrap_or(true)
    }

    /// Trace an environment and every environment it keeps live: its parent
    /// chain, its alias targets and the owners of its imports, and theirs.
    /// Deduped by environment identity, so the global environment is walked
    /// once no matter how many closures point at it. Iterative: what one
    /// environment reaches is queued, not recursed into.
    ///
    /// What an environment reaches is `Environment::for_each_gc_edge`'s
    /// answer, which names every field (#623).
    pub fn visit_env(&mut self, env: &Environment) {
        let mut pending = std::mem::take(&mut self.env_worklist);
        self.visit_env_chain(env, &mut pending);
        while let Some(next) = pending.pop() {
            self.visit_env_chain(&next, &mut pending);
        }
        self.env_worklist = pending;
    }

    /// Mark the values of `env` and of its parents, and queue the other
    /// environments they reach. The chain stops at the first environment
    /// this collection has walked already: its parents were walked with it.
    fn visit_env_chain(&mut self, env: &Environment, pending: &mut Vec<Rc<Environment>>) {
        let mut current = Some(env);
        while let Some(env) = current {
            if !self.seen_envs.insert(env.gc_identity()) {
                break;
            }
            let parent = env.for_each_gc_edge(&mut |edge| match edge {
                GcEdge::Value(tv) => self.visit(tv),
                GcEdge::Env(next) => {
                    if !self.seen_envs.contains(&next.gc_identity()) {
                        pending.push(Rc::clone(next));
                    }
                }
            });
            current = parent.map(|parent| parent.as_ref());
        }
    }

    /// Trace a continuation held outside the heap. Root providers need this
    /// for continuations that never became heap objects (e.g. the
    /// tree-walker's `PENDING_ESCAPE` and in-flight continuation chain).
    pub fn visit_continuation(&mut self, k: &Rc<CpsContinuation>) {
        if self.seen_conts.insert(Rc::as_ptr(k) as usize) {
            self.cont_worklist.push(k.clone());
        }
    }

    /// Record `identity` (an `Rc::as_ptr` address) as visited for this
    /// collection; returns `false` if it was already seen.
    ///
    /// Root providers need this for their own `Rc`-shared structures. Without
    /// it, tracing a persistent linked list whose nodes each capture the tail
    /// below them is **exponential**: the tree-walker's `ContEnv` chains cost
    /// `2ⁿ − 1` node visits, which measured 6.8 s for a single collection at
    /// nesting depth 26. Every other `Rc`-shared structure the visitor walks
    /// (`visit_env`, `visit_continuation`, `visit_expr_literals`) has its own
    /// dedup set for exactly this reason.
    pub fn visit_once(&mut self, identity: usize) -> bool {
        self.seen_shared.insert(identity)
    }

    /// Trace a promise's payload, forced or not.
    pub fn visit_promise(&mut self, promise: &RefCell<PromiseState>) {
        match *promise.borrow() {
            PromiseState::Delayed(tv) | PromiseState::Forced(tv) => self.visit(tv),
        }
    }

    /// Trace a `dynamic-wind` record: its before/after thunks and the handler
    /// stack it will run them under. A handler reachable only from a record
    /// is live for as long as the record can still run a thunk.
    pub fn visit_wind(&mut self, wind: &DynamicWindRecord) {
        self.visit_wind_with(wind, trace_exception_handler);
    }

    /// Trace a backend's wind record, delegating only its handler payloads.
    /// The callback must visit every GC root carried by a handler.
    pub fn visit_wind_with<H>(
        &mut self,
        wind: &WindRecord<H>,
        mut trace_handler: impl FnMut(&H, &mut Self),
    ) {
        let WindRecord {
            // A number minted per `dynamic-wind` call.
            id: _,
            before,
            after,
            handlers,
        } = wind;
        self.visit(*before);
        self.visit(*after);
        for handler in handlers.iter() {
            trace_handler(handler, self);
        }
    }

    /// Trace a stack of tree-walker wind records.
    pub fn visit_winds(&mut self, winds: &[DynamicWindRecord]) {
        self.visit_winds_with(winds, trace_exception_handler);
    }

    /// Trace a backend's wind stack with its handler-root visitor.
    pub fn visit_winds_with<H>(
        &mut self,
        winds: &[WindRecord<H>],
        mut trace_handler: impl FnMut(&H, &mut Self),
    ) {
        for wind in winds {
            self.visit_wind_with(wind, &mut trace_handler);
        }
    }

    /// Trace a library's two root sets: its exports and its environment
    /// (`Library::for_each_gc_edge`, which names every field).
    pub fn visit_library(&mut self, library: &Library) {
        library.for_each_gc_edge(&mut |edge| match edge {
            GcEdge::Value(tv) => self.visit(tv),
            GcEdge::Env(env) => self.visit_env(env),
        });
    }

    /// Count a live tree-walker closure's payload (`heap/account.rs`): into
    /// the live bytes, and into the closures' part of them, from which sweep
    /// credits the dead closures'. Out of line, so that the marking loop,
    /// which every collection on either backend runs, carries a call in the
    /// closure's arm rather than the measuring.
    #[inline(never)]
    fn count_closure_payload(
        &mut self,
        params: &Vec<crate::core_expr::ScopedParam>,
        variadic: &Option<crate::core_expr::ScopedParam>,
    ) {
        let payload = account::cps_lambda_payload(params, variadic);
        self.marks.live_payload += payload;
        self.marks.live_closures += payload;
    }

    /// Trace literals embedded in live code (`CpsExprKind::Literal` /
    /// `Quasiquote`). Root providers need this for expression trees reachable
    /// outside the heap (e.g. a suspended `StepResult`'s current expression).
    /// Memoized per collection by node address — bodies are shared via `Rc`
    /// across closures. Depth is bounded by program size, not data size, so
    /// recursion is acceptable here (data tracing stays iterative).
    pub fn visit_expr_literals(&mut self, expr: &crate::cps_expr::CpsExpr) {
        let mut seen = std::mem::take(&mut self.seen_exprs);
        expr.for_each_literal(&mut seen, &mut |tv| self.visit(tv));
        self.seen_exprs = seen;
    }

    /// Did marking reach a `VmContinuationRef` / `VmDelimitedContinuationRef`
    /// object with this id? For [`GcRoots::sweep_weak`] — an entry whose id
    /// was never reached is dead.
    pub fn weak_continuation_id_is_live(&self, id: u64) -> bool {
        self.live_weak_ids.contains(&id)
    }

    /// Finish marking: drain the worklists to a fixed point and return the
    /// mark bits, ready for `Heap::sweep`.
    pub fn finish(mut self) -> MarkBits {
        self.drain();
        self.marks
    }

    /// Process the worklists to a fixed point.
    fn drain(&mut self) {
        loop {
            if let Some(tv) = self.worklist.pop() {
                self.trace_children(tv);
            } else if let Some(k) = self.cont_worklist.pop() {
                self.trace_continuation_children(&k);
            } else if let Some(env) = self.cont_env_worklist.pop() {
                // Each entry is a continuation variable's name and its value.
                for (_, value) in env.iter() {
                    trace_cont_value(value, self);
                }
            } else {
                break;
            }
        }
    }

    fn trace_children(&mut self, tv: TaggedValue) {
        let heap = self.heap;
        let idx = tv.heap_index() as usize;
        if tv.is_pair() {
            let (car, cdr) = heap.pairs[idx];
            self.visit(car);
            self.visit(cdr);
        } else if tv.is_vector() {
            let elements = &heap.vectors[idx];
            self.marks.live_payload += account::vector_payload(elements);
            for &element in elements {
                self.visit(element);
            }
        } else if tv.is_object() {
            self.trace_object_children(&heap.objects[idx], tv);
        }
    }

    /// Trace one object's children.
    ///
    /// Every variant is matched, and every variant with named fields is taken
    /// apart by name (#623): a new variant fails to compile here until it has
    /// an arm, and a new field until the arm names it — traced, or written
    /// `field: _` with the reason it holds no value. The sentinel tests in
    /// `heap/trace_sentinels.rs` put a fresh value in each traced field and
    /// read it back after a collection; deleting a field's trace fails its
    /// test.
    fn trace_object_children(&mut self, data: &'h HeapObjectData, tv: TaggedValue) {
        // The byte account (`heap/account.rs`): a VM continuation's snapshot
        // is its handle's payload, so it is counted live here with the
        // handle, which is when the weak-id fixpoint traces it.
        self.marks.live_payload += data.payload_bytes();
        match data {
            // Leaves: no embedded heap references. Filing a variant here that
            // holds a value is a use-after-free, not a compile error
            // (`HeapObjectData`'s doc comment), so each payload says what it
            // holds instead.
            //
            // An arbitrary-precision integer.
            HeapObjectData::BigInt(_)
            // A quotient of two of them.
            | HeapObjectData::Rational(_)
            // An `f64`.
            | HeapObjectData::Real(_)
            // The symbol's name.
            | HeapObjectData::Symbol(_)
            // Bytes.
            | HeapObjectData::Bytevector(_)
            // Buffers, a file handle, a position and flags: `port.rs` holds
            // no `TaggedValue`.
            | HeapObjectData::Port(_)
            // An id, a name and field names.
            | HeapObjectData::RecordType(_)
            | HeapObjectData::Identifier {
                // A spelling.
                name: _,
                // Scope ids.
                scopes: _,
                // A flag.
                written: _,
            }
            // A name and an id.
            | HeapObjectData::PromptTag(_)
            // A datum label's number.
            | HeapObjectData::LabelPlaceholder(_)
            // Which syntactic keyword.
            | HeapObjectData::CoreSyntax(_)
            // A swept slot.
            | HeapObjectData::Free => {}

            // Weak key, SRFI 124: neither field is traced here. The datum
            // must survive only as long as the *key* does, so tracing either
            // one now would be wrong — tracing the key would make the pair
            // strong, and tracing the datum would let a dead key keep it
            // alive. Both wait for the ephemeron fixpoint below, which traces
            // a pair only once it knows the key is reachable some other way.
            HeapObjectData::Ephemeron(_) => self.pending_ephemerons.push(tv),

            // Weak keys: the payload lives in VmState's side tables and is
            // traced only if the ref object itself is live — record the id
            // for the trace_weak_ids fixpoint (design §9.5). The set-guard
            // keeps the queue duplicate-free so each id is broadcast once.
            HeapObjectData::VmContinuationRef {
                id,
                // A size, for the byte account, counted above.
                bytes: _,
            }
            | HeapObjectData::VmDelimitedContinuationRef {
                id,
                // A size, as above.
                bytes: _,
            } => {
                if self.live_weak_ids.insert(*id) {
                    self.new_weak_ids.push(*id);
                }
            }

            HeapObjectData::Complex { real, imag } => {
                self.visit(*real);
                self.visit(*imag);
            }
            HeapObjectData::Exception {
                // Which kind of condition; `Custom` carries a string.
                kind: _,
                // A string.
                message: _,
                irritants,
            } => {
                for &irritant in irritants {
                    self.visit(irritant);
                }
            }
            HeapObjectData::Procedure(p) => match p.as_ref() {
                Procedure::Primitive {
                    // A string.
                    name: _,
                    // Argument counts.
                    arity: _,
                    // A string.
                    qualified_name: _,
                    // An index into the primitive registry.
                    registry_index: _,
                } => {}
                Procedure::CpsLambda {
                    // Names and scope ids: measured below, not traced.
                    params,
                    // A name and scope ids, as above.
                    variadic,
                    // A name.
                    cont_param: _,
                    body,
                    env,
                    // Scope ids.
                    binding_scopes: _,
                } => {
                    // The byte account: a tree-walker closure's payload is
                    // counted here, not by `payload_bytes` above, which keeps
                    // the allocation path's match a leaf (`heap/account.rs`).
                    self.count_closure_payload(params, variadic);
                    self.visit_expr_literals(body);
                    self.visit_env(env);
                }
            },
            HeapObjectData::Macro(m) => self.trace_compiled_macro(m),
            HeapObjectData::Record {
                // An `Rc`'d descriptor: an id, a name and field names.
                record_type: _,
                fields,
            } => {
                for &field in fields.borrow().iter() {
                    self.visit(field);
                }
            }
            HeapObjectData::Parameter { values, converter } => {
                for &value in values.borrow().iter() {
                    self.visit(value);
                }
                if let Some(converter) = converter {
                    self.visit(*converter);
                }
            }
            HeapObjectData::Promise(state) => self.visit_promise(state),
            HeapObjectData::Library(lib) => self.visit_library(lib),
            HeapObjectData::Values(values) => {
                for &value in values {
                    self.visit(value);
                }
            }
            HeapObjectData::EnvironmentSpecifier {
                env,
                // A flag.
                mutable: _,
            } => {
                self.visit_env(env);
            }
            HeapObjectData::MutableCell(cell) => {
                let inner = *cell.borrow();
                self.visit(inner);
            }
            HeapObjectData::VmClosure {
                // The VM's code id. The code's constants are rooted by the
                // VM's `code_store`, which keeps the code while a live
                // closure can run it (#338).
                code_id: _,
                free_vars,
                globals,
            } => {
                for &free_var in free_vars {
                    self.visit(free_var);
                }
                self.visit_env(globals);
            }
            HeapObjectData::Continuation(k) => {
                self.visit_continuation(k);
            }
        }
    }

    /// Trace a compiled macro: the literals in its patterns and templates,
    /// and the environments it keeps live. Every field is named (#623);
    /// pinned by the sentinel test `compiled_macro_fields`.
    fn trace_compiled_macro(&mut self, m: &CompiledMacro) {
        let CompiledMacro {
            // A string, for diagnostics.
            name: _,
            rules,
            // A count.
            max_pvars: _,
            // Scope ids.
            definition_scopes: _,
            // A handle to the heap its literals live in, which the collector
            // is already marking (design §9.6): not an edge into it.
            heap: _,
            // Names.
            template_symbols: _,
            // Names, each with the scope sets it arrived under.
            inherited_identifiers: _,
            definition_env,
            foreign_expansions,
        } = m;
        for rule in rules {
            let CompiledRule {
                pattern,
                template,
                // A count.
                num_pvars: _,
                // A depth.
                max_level: _,
                // Pattern-variable names, for diagnostics.
                pvar_names: _,
            } = rule;
            pattern.for_each_literal(&mut |tv| self.visit(tv));
            template.for_each_literal(&mut |tv| self.visit(tv));
        }
        // A live macro keeps its definition environment live: its templates
        // may reference bindings that exist nowhere else (#38).
        if let Some(env) = definition_env {
            self.visit_env(env);
        }
        // The definition environments of the macros from another library or
        // program whose expansions put this macro's inherited identifiers
        // into its templates (#446). Early binding asks them only where a
        // name's binding lives, never for a value, and today each is rooted
        // another way as well: by the registry while its library is
        // registered, and after a redefinition replaces the library, by the
        // `owners` list of whatever imported the generator, which never
        // shrinks (#614). Traced all the same, because the macro holds them by
        // `Rc` for as long as it lives, and an environment kept alive with its
        // values swept is #38's shape: #614's fix drops an owner once no link
        // points into it, and this edge is then what keeps such an
        // environment's values. Empty for a written macro, and otherwise one
        // or two environments the walk has nearly always seen already.
        for (_scope, env) in foreign_expansions {
            self.visit_env(env);
        }
    }

    /// Trace a captured continuation. Every field is named (#623); pinned by
    /// the sentinel test `cps_continuation_fields`.
    fn trace_continuation_children(&mut self, k: &CpsContinuation) {
        let CpsContinuation {
            body,
            // A name.
            param: _,
            env,
            // A prompt id.
            boundary: _,
            // A trampoline id.
            trampoline: _,
            // A flag.
            crosses_callback: _,
            dynamic_winds,
            prompt_stack,
            exception_handlers,
            captured_cont_env,
            resume,
        } = k;
        self.visit_expr_literals(body);
        self.visit_env(env);
        self.visit_winds(dynamic_winds);
        for handler in exception_handlers {
            trace_exception_handler(handler, self);
        }
        for frame in prompt_stack {
            trace_prompt_frame(frame, self);
        }
        trace_cont_env(captured_cont_env, self);
        // Every reify site today stores in `resume` a wrapper it read out of
        // `captured_cont_env`, so this trace looks redundant. That is an
        // accident of the construction sites, not an invariant anything
        // enforces, and it is what hid this edge untraced for 1.6 days (#47):
        // the sentinel test puts a value here that `captured_cont_env` does
        // not hold.
        if let Some(resume) = resume {
            trace_cont_value(resume, self);
        }
    }
}

// ============================================================================
// Sweep
// ============================================================================

/// Sweep one arena: pre-mark already-free slots (they are unmarked by
/// definition; re-pushing them would double-free on reuse), then reclaim
/// every remaining unmarked slot, showing each one's payload to
/// `record_freed` before its tombstone drops it (the byte account, and the
/// VM's code release, #338). Returns the number of slots reclaimed.
///
/// In check builds ([`GC_CHECK`](super::GC_CHECK)) the pre-mark doubles as
/// `docs/GC_DESIGN.md` §11 item 5's assertion: a free slot whose bit is
/// already set was reached by marking, so a root or a traced edge names a
/// slot that was free when this collection began. `checks` records each slot
/// freed here, through the reference `reference` builds for it
/// (`heap/check.rs`).
#[allow(clippy::too_many_arguments)]
fn sweep_arena<T>(
    arena_name: &'static str,
    arena: &mut [T],
    free_list: &mut Vec<HeapIndex>,
    marks: &mut BitSet,
    checks: &mut SlotChecks,
    reference: impl Fn(HeapIndex) -> TaggedValue,
    write_tombstone: bool,
    tombstone: impl Fn() -> T,
    mut record_freed: impl FnMut(&T),
) -> usize {
    for &idx in free_list.iter() {
        if !marks.set(idx as usize) && super::GC_CHECK {
            free_slot_reached_by_marking(arena_name, idx);
        }
    }
    let mut swept = 0;
    for (i, slot) in arena.iter_mut().enumerate() {
        if !marks.get(i) {
            checks.free(reference(i as HeapIndex));
            // Before the tombstone, so a consumer can see what died.
            record_freed(slot);
            if write_tombstone {
                *slot = tombstone();
            }
            free_list.push(i as HeapIndex);
            swept += 1;
        }
    }
    swept
}

/// The panic of `sweep_arena`'s pre-mark check, out of line so the loop
/// stays a set and a branch.
#[cold]
#[inline(never)]
fn free_slot_reached_by_marking(arena: &str, idx: HeapIndex) -> ! {
    panic!("dangling reference: {arena} slot {idx} is free, but marking reached it")
}

impl Heap {
    /// Reclaim every unmarked slot: push its index onto the arena's free list
    /// and tombstone the slot. Tombstoning drops `Rc` payloads eagerly, which
    /// is what breaks closure ↔ environment cycles (design §8) — it is
    /// load-bearing for objects (`Rc` payloads) and vectors/strings (element
    /// buffers). Pairs are `Copy` with nothing to drop, so a plain release
    /// build skips the store; a check build writes a poison value, so a
    /// stale reference that marking reaches traces nothing from the dead
    /// pair.
    ///
    /// Use-after-free detection does not depend on the tombstones: in a
    /// check build every arena accessor refuses a reference to a freed slot,
    /// or to a slot freed and reused since, by the slot's generation
    /// (`heap/check.rs`).
    ///
    /// Consumes the mark bits as scratch space (read [`MarkBits::marked`]
    /// first) and resets the allocation counters and the collection-pending
    /// flag — sweep completion is the "collection happened" boundary. Settles
    /// the byte account (`heap/account.rs`): L becomes the bytes marking
    /// found live, plus the external bytes held now, and each freed slot's
    /// bytes, its payload's included, are added to the bytes reclaimed.
    ///
    /// Crate-private with the rest of the collector (#624).
    pub(crate) fn sweep(&mut self, marks: &mut MarkBits) -> ArenaCounts {
        // Before the arenas are swept: `sweep_arena` sets the free slots'
        // bits, which are not live.
        let live = marks.live_bytes();
        // What the account holds for the tree-walker closures, which a check
        // build compares with the live ones' and the dead ones' once the
        // object arena's sweep has measured those.
        #[cfg(any(debug_assertions, feature = "gc-check"))]
        let charged_closures = self.account.closures;
        let freed_closure_payloads = self.settle_closure_payloads(marks);
        // A check build measures each dead tree-walker closure as the object
        // arena's sweep drops it.
        #[cfg(any(debug_assertions, feature = "gc-check"))]
        let mut measured_dead_closures = 0usize;
        // Provenance is not a root. Prune it before slots can be reused,
        // inspecting only annotated syntax rather than every freed datum.
        self.syntax_sources
            .retain(|&bits, _| marks.is_marked(TaggedValue::from_raw(bits)) == Some(true));
        // HashMap::retain visits capacity, so release a large table after a
        // source-heavy phase instead of rescanning it on every later read GC.
        if self.syntax_sources.capacity() > self.syntax_sources.len().saturating_mul(4).max(64) {
            self.syntax_sources.shrink_to(64);
        }
        // Moved out so the object arena's recording closure can hold it while
        // the arenas themselves are mutably borrowed.
        let mut freed_closures = self.gc_freed_closure_code_ids.take();
        // The payloads of the slots freed, measured before their tombstones
        // drop them.
        let mut freed_payload = 0usize;
        let swept = ArenaCounts {
            pairs: sweep_arena(
                "pair",
                &mut self.pairs,
                &mut self.free_pairs,
                &mut marks.pairs,
                &mut self.pair_checks,
                TaggedValue::pair,
                super::GC_CHECK,
                || (TaggedValue::GC_POISON, TaggedValue::GC_POISON),
                |_| {},
            ),
            vectors: sweep_arena(
                "vector",
                &mut self.vectors,
                &mut self.free_vectors,
                &mut marks.vectors,
                &mut self.vector_checks,
                TaggedValue::vector,
                true,
                Vec::new,
                |elements| freed_payload += account::vector_payload(elements),
            ),
            strings: sweep_arena(
                "string",
                &mut self.strings,
                &mut self.free_strings,
                &mut marks.strings,
                &mut self.string_checks,
                TaggedValue::string,
                true,
                Vec::new,
                |chars| freed_payload += account::string_payload(chars),
            ),
            objects: sweep_arena(
                "object",
                &mut self.objects,
                &mut self.free_objects,
                &mut marks.objects,
                &mut self.object_checks,
                TaggedValue::object,
                true,
                || HeapObjectData::Free,
                |old| {
                    freed_payload += old.payload_bytes();
                    #[cfg(any(debug_assertions, feature = "gc-check"))]
                    if let HeapObjectData::Procedure(procedure) = old {
                        measured_dead_closures += account::procedure_payload(procedure);
                    }
                    if let (Some(ids), HeapObjectData::VmClosure { code_id, .. }) =
                        (freed_closures.as_mut(), old)
                    {
                        ids.push(*code_id);
                    }
                },
            ),
        };
        self.gc_freed_closure_code_ids = freed_closures;
        #[cfg(any(debug_assertions, feature = "gc-check"))]
        assert_eq!(
            charged_closures,
            marks.live_closures + measured_dead_closures,
            "the tree-walker closures charged are not the live ones and the dead ones"
        );
        let freed_bytes = swept.pairs * PAIR_SLOT_BYTES
            + swept.vectors * VECTOR_SLOT_BYTES
            + swept.strings * STRING_SLOT_BYTES
            + swept.objects * OBJECT_SLOT_BYTES
            + freed_payload
            + freed_closure_payloads;
        let account = &mut self.account;
        account.reclaimed += freed_bytes as u64;
        account.through_last_gc = account
            .through_last_gc
            .saturating_add(account.shared.since_gc.get() as u64);
        account.shared.since_gc.set(0);
        // After the arenas, as `since_gc`: the file ports this sweep closed
        // took themselves off the count as they dropped, and the ones it
        // found live count toward no later collection (#607).
        account.shared.start_cycle();
        // Read after the arenas are swept: external bytes that the dead
        // slots' payloads gave back as they dropped — a dead environment
        // specifier's namespace, say — are not live.
        account.live = live.saturating_add(account.shared.external.get());
        self.allocs_since_gc = 0;
        self.gc_pending.set(false);
        self.gc_collections += 1;
        self.gc_last_swept = swept.total();
        swept
    }

    /// Sweep's part of a tree-walker closure's own path through the byte
    /// account (`heap/account.rs`): the dead closures' payloads, credited in
    /// one sum with no work per slot. Every closure in the arena was charged
    /// by `alloc_procedure`, and marking counted the live ones', so the dead
    /// ones' are the account's sum less those; the live ones' are the sum
    /// from here on. A check build also measures each dead closure in the
    /// object arena's sweep, and asserts in `sweep` that the charge is the
    /// live ones' and those together, so that a closure which bypassed the
    /// charge fails the first collection after it; the measuring rides the
    /// sweep's own walk, and a build without the check compiles none of it.
    fn settle_closure_payloads(&mut self, marks: &MarkBits) -> usize {
        let live = marks.live_closures;
        let dead = self.account.closures.saturating_sub(live);
        self.account.closures = live;
        dead
    }
}

// ============================================================================
// MarkSweepCollector
// ============================================================================

/// The floor of the adaptive interval, in bytes of allocation: GC_PRD §15's
/// 8 MiB, which keeps a small program's collections few while bounding its
/// garbage at a few times the size of its live data.
pub const DEFAULT_MIN_BYTES: usize = 8 << 20;

/// The v1 collector: stop-the-world mark-and-sweep, shared by both backends.
/// Adaptive trigger: collect on a `(gc)` request, or once the bytes allocated
/// since the last GC reach `max(8 MiB, 2·L)`, L being the bytes the last
/// collection found live (GC_PRD §15; `heap/account.rs`). The peak heap then
/// stays near three times what is live.
pub(crate) struct MarkSweepCollector {
    min_bytes: usize,
    live_bytes_after_last: usize,
    stats: GcStats,
}

impl MarkSweepCollector {
    pub fn new() -> Self {
        Self::with_min_bytes(DEFAULT_MIN_BYTES)
    }

    /// A custom byte floor. Note this is only a *floor*: the adaptive `2·L`
    /// term still applies, so a small value does not by itself produce
    /// stress-test behavior — that is [`GcMode::Stress`], which counts
    /// allocations and bypasses the adaptive term entirely.
    pub fn with_min_bytes(min_bytes: usize) -> Self {
        Self {
            min_bytes,
            live_bytes_after_last: 0,
            stats: GcStats::default(),
        }
    }

    /// The adaptive trigger: bytes of allocation since the last collection at
    /// which the next one fires. Installed into the heap via
    /// `GcController::current_threshold` so `note_alloc` can raise the
    /// pending flag without consulting policy.
    pub fn auto_threshold(&self) -> usize {
        self.min_bytes
            .max(self.live_bytes_after_last.saturating_mul(2))
    }
}

impl Default for MarkSweepCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// The complete mark phase: root tracing, one fixpoint over *both* weak
/// reference kinds — weak ids (design §9.5) and ephemerons (SRFI 124) —
/// breaking the ephemerons that fixpoint left unretained, and weak-entry
/// pruning, in the one order that is sound.
///
/// One function so a collector composes *around* it (triggering, sweep
/// strategy) without being able to mis-order its interior. Skipping the
/// fixpoint would sweep live continuation payloads, skipping `sweep_weak`
/// would reinstate the §9.5 monotonic leak, and the two weak kinds share one
/// loop for the reason the comment on it gives. Crate-private, like every
/// other way to run a collection (#624).
pub(crate) fn run_mark_phase(heap: &Heap, roots: &[&dyn GcRoots]) -> MarkBits {
    let mut visitor = GcVisitor::new(heap);
    for provider in roots {
        provider.trace_roots(&mut visitor);
    }
    visitor.drain();

    // One fixpoint over *both* kinds of weak reference, because each can feed
    // the other and neither is quiescent until both are.
    //
    // - Weak ids (design §9.5): a marked `VmContinuationRef` means its payload
    //   in `VmState`'s side tables is live; broadcasting the batch traces it.
    // - Ephemerons (SRFI 124): a pair whose key is *already* marked is
    //   retained, and only then are its key and datum traced.
    //
    // Running them in sequence is what an earlier version did, and it was
    // unsound: an ephemeron retained after the weak-id loop had finished could
    // mark a `VmContinuationRef` that was then never broadcast, so its payload
    // went untraced while `sweep_weak` kept the store entry — a continuation
    // pointing at swept slots. The reverse hazard is just as real: tracing a
    // weak payload can reach an ephemeron, which must not arrive after the
    // ephemeron loop has stopped.
    //
    // Terminates because progress is finite in both directions: an id enters
    // the queue at most once (the set-guard in `trace_object_children`), and
    // each retaining round permanently removes at least one pair from a finite
    // `pending_ephemerons`. Note it is *not* that `pending` shrinks each round
    // — `drain` can discover new ephemerons and grow it.
    loop {
        let mut progressed = false;

        let ids = std::mem::take(&mut visitor.new_weak_ids);
        if !ids.is_empty() {
            for provider in roots {
                provider.trace_weak_ids(&ids, &mut visitor);
            }
            visitor.drain();
            progressed = true;
        }

        let mut pending = std::mem::take(&mut visitor.pending_ephemerons);
        pending.retain(|&tv| {
            let Some((key, datum)) = heap.ephemeron_state(tv).flatten() else {
                return false; // not an ephemeron, or already broken
            };
            // An immediate key has no cell that can die, so `value_is_live`
            // answers true for it; a heap key must already be marked by some
            // path other than this pair.
            if visitor.value_is_live(key) {
                visitor.visit(key);
                visitor.visit(datum);
                progressed = true;
                false
            } else {
                true
            }
        });
        // `visit` only enqueues, so `pending_ephemerons` is still empty here
        // and `drain` is what can refill it — hence the append rather than an
        // assignment.
        visitor.drain();
        visitor.pending_ephemerons.append(&mut pending);

        if !progressed {
            break;
        }
    }

    // Whatever is still pending has an unreachable key: break it, so its datum
    // stops being a root and `ephemeron-broken?` answers #t.
    for &tv in &visitor.pending_ephemerons {
        heap.break_ephemeron(tv);
    }

    for provider in roots {
        provider.sweep_weak(&visitor);
    }

    visitor.finish()
}

impl Collector for MarkSweepCollector {
    fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
        let start = Instant::now();

        let mut marks = run_mark_phase(heap, roots);

        let marked = marks.marked();
        let swept = heap.sweep(&mut marks);

        self.live_bytes_after_last = heap.live_bytes();
        self.stats.collections += 1;
        self.stats.last_marked = marked;
        self.stats.last_swept = swept;
        self.stats.last_pause_micros = start.elapsed().as_micros();
        self.stats
    }
}

/// Run one full collection of `heap` from `roots` alone, outside every safe
/// point and every deferral rule.
///
/// **Not an API.** It exists for unit tests in other crates that drive the
/// collector against hand-built state and assert on what it freed —
/// `patina-vm`'s weak continuation table tests, and the sentinel tests of
/// each crate's root providers (#623). A backend or a host collects through
/// [`GcController::safe_point`], which is what keeps a collection from
/// running while a Rust frame holds values no root provider sees.
///
/// Compiled only with the `test-support` feature, which patina-vm,
/// patina-tree-walker and patina-runtime enable for their tests alone (a
/// dev-dependency feature, which resolver 2 keeps out of every normal
/// build): a build that ships has no way to collect outside `safe_point`.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub fn collect_for_tests(heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
    MarkSweepCollector::new().collect(heap, roots)
}

// ============================================================================
// Continuation-value tracing
// ============================================================================
//
// Moved here with ContValue itself: CpsContinuation stores a ContEnv, so the
// collector has to be able to walk one.

/// Trace a continuation environment.
///
/// Deduped by chain identity: `ContEnv` is a persistent `Rc` list and every
/// `ContValue::Local` captures the chain below it, so an un-memoized walk is
/// exponential (`2ⁿ − 1` node visits — measured at 6.8 s for one collection
/// at nesting depth 26). Skipping an already-seen chain is safe: its entries,
/// and therefore its whole tail, are already queued when it is first seen.
/// Queue an O(1) snapshot rather than tracing recursively: a local
/// continuation can capture another environment at every Scheme call depth.
pub fn trace_cont_env(cont_env: &ContEnv, visitor: &mut GcVisitor<'_>) {
    if visitor.visit_once(cont_env.gc_identity()) {
        visitor.cont_env_worklist.push(cont_env.clone());
    }
}

/// Trace a continuation value, walking the `Box<ContValue>` chain
/// iteratively — most variants differ only in what they visit before handing
/// off to the continuation they wrap. Local continuation environments and
/// captured continuations are queued on the visitor's worklists as well.
///
/// Every variant's fields are named (#623); pinned by the sentinel test
/// `cont_value_variants`.
pub fn trace_cont_value(cont: &ContValue, visitor: &mut GcVisitor<'_>) {
    let mut cont = cont;
    loop {
        cont = match cont {
            ContValue::Halt => return,

            ContValue::Local {
                // A name.
                param: _,
                body,
                env,
                cont_env,
            } => {
                visitor.visit_expr_literals(body);
                visitor.visit_env(env);
                trace_cont_env(cont_env, visitor);
                return;
            }

            ContValue::Captured(k) => return visitor.visit_continuation(k),

            ContValue::CallWithValuesConsumer {
                consumer,
                original_cont,
            } => {
                visitor.visit(*consumer);
                original_cont
            }

            ContValue::ForceCache {
                promise,
                original_cont,
            } => {
                visitor.visit(*promise);
                original_cont
            }

            ContValue::ResumePrimitive {
                // An index into the primitive registry.
                index: _,
                state,
                original_cont,
            } => {
                visitor.visit(*state);
                original_cont
            }

            ContValue::DynamicWindCleanup {
                after,
                // A number minted per `dynamic-wind` call.
                wind_id: _,
                original_cont,
            } => {
                visitor.visit(*after);
                original_cont
            }

            ContValue::DynamicWindSetup {
                wind_record,
                body,
                cleanup_cont,
            } => {
                visitor.visit_wind(wind_record);
                visitor.visit(*body);
                cleanup_cont
            }

            ContValue::DynamicWindAfterDone {
                result_value,
                original_cont,
            } => {
                visitor.visit(*result_value);
                original_cont
            }

            ContValue::Jump {
                entered,
                value,
                target,
            } => {
                if let Some(record) = entered {
                    visitor.visit_wind(record);
                }
                visitor.visit(*value);
                return visitor.visit_continuation(target);
            }

            ContValue::ExceptionHandlerCleanup { original_cont } => original_cont,

            ContValue::RaiseHandlerReturn {
                // A flag.
                continuable: _,
                original_exception,
                original_cont,
                popped_handler,
            } => {
                if let Some(exception) = original_exception {
                    visitor.visit(*exception);
                }
                if let Some(handler) = popped_handler {
                    trace_exception_handler(handler, visitor);
                }
                original_cont
            }

            ContValue::PromptBoundary {
                // A prompt id.
                id: _,
            } => return,

            ContValue::AbortLanding {
                handler,
                delimited,
                cont,
            } => {
                visitor.visit(*handler);
                visitor.visit(*delimited);
                cont
            }

            ContValue::ExitLanding {
                // An exit status.
                status: _,
            } => return,

            ContValue::ComposableInvokeStep {
                target,
                value,
                // A position in `target.dynamic_winds`, which `target`'s own
                // trace covers.
                index: _,
                cont,
            } => {
                visitor.visit_continuation(target);
                visitor.visit(*value);
                cont
            }
        };
    }
}

/// Trace a prompt frame: its handler and the continuation below it. Every
/// field is named (#623); pinned by the sentinel test `cps_continuation_fields`.
pub fn trace_prompt_frame(frame: &PromptFrame, visitor: &mut GcVisitor<'_>) {
    let PromptFrame {
        // A prompt id.
        id: _,
        // A plain Rust struct shared by `Rc` (a name and an id), not a heap
        // value.
        tag: _,
        handler,
        cont,
        // A depth.
        wind_depth: _,
        // A trampoline id.
        trampoline: _,
        // A depth.
        handler_depth: _,
    } = frame;
    visitor.visit(*handler);
    trace_cont_value(cont, visitor);
}

/// Trace an exception handler: the handler procedure it holds, which is all
/// it holds. It used to carry the wind depth `raise` unwound to; no raise path
/// unwinds now, so the field is gone and so is the retention. Named by field
/// (#623) so that a new one does not compile here until it is traced.
pub fn trace_exception_handler(handler: &ExceptionHandler, visitor: &mut GcVisitor<'_>) {
    let ExceptionHandler { handler } = handler;
    visitor.visit(*handler);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cps_expr::{CpsExpr, CpsExprKind};
    use std::cell::RefCell;
    use std::mem::size_of;

    /// Synthetic root provider for tests.
    #[derive(Default)]
    struct TestRoots {
        values: Vec<TaggedValue>,
        envs: Vec<Rc<Environment>>,
    }

    impl GcRoots for TestRoots {
        fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
            visitor.visit_slice(&self.values);
            for env in &self.envs {
                visitor.visit_env(env);
            }
        }
    }

    fn collect(heap: &mut Heap, roots: &TestRoots) -> GcStats {
        MarkSweepCollector::new().collect(heap, &[roots])
    }

    #[test]
    fn unreachable_pair_swept_reachable_survives() {
        let mut heap = Heap::new();
        let live = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2));
        let dead = heap.alloc_pair(TaggedValue::fixnum(3), TaggedValue::fixnum(4));

        let roots = TestRoots {
            values: vec![live],
            ..Default::default()
        };
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_marked.pairs, 1);
        assert_eq!(stats.last_swept.pairs, 1);
        assert_eq!(heap.free_pairs, vec![dead.heap_index()]);
        assert_eq!(heap.car(live), TaggedValue::fixnum(1));
        assert_eq!(heap.cdr(live), TaggedValue::fixnum(2));

        // The freed slot is reused in place by the existing allocation path.
        let reused = heap.alloc_pair(TaggedValue::fixnum(5), TaggedValue::fixnum(6));
        assert_eq!(reused.heap_index(), dead.heap_index());
        assert!(heap.free_pairs.is_empty());
    }

    #[test]
    fn wind_records_keep_thunks_and_backend_handler_payloads_alive() {
        // A backend handler may carry more than one root. The common wind
        // traversal must delegate all handlers and retain both thunks.
        struct Handler([TaggedValue; 2]);
        struct Winds(Vec<WindRecord<Handler>>);
        impl GcRoots for Winds {
            fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
                visitor.visit_winds_with(&self.0, |handler, visitor| {
                    visitor.visit_slice(&handler.0);
                });
            }
        }

        let mut heap = Heap::new();
        let mut winds = Winds(Vec::new());
        for _ in 0..2 {
            let before = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
            let after = heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
            let handlers = (0..2)
                .map(|_| {
                    Handler([
                        heap.alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL),
                        heap.alloc_pair(TaggedValue::fixnum(4), TaggedValue::NULL),
                    ])
                })
                .collect::<Vec<_>>();
            winds
                .0
                .push(WindRecord::new(before, after, handlers.into()));
        }
        let dead = heap.alloc_pair(TaggedValue::fixnum(5), TaggedValue::NULL);
        let stats = MarkSweepCollector::new().collect(&mut heap, &[&winds]);
        assert_eq!(stats.last_marked.pairs, 12);
        assert_eq!(heap.free_pairs, vec![dead.heap_index()]);

        // Removing the records must release the same payloads.
        winds.0.clear();
        let stats = MarkSweepCollector::new().collect(&mut heap, &[&winds]);
        assert_eq!(stats.last_swept.pairs, 12);
    }

    #[test]
    fn nested_structures_traced_transitively() {
        let mut heap = Heap::new();
        let s = heap.alloc_string("live".to_string());
        let inner = heap.alloc_pair(s, TaggedValue::NULL);
        let vec = heap.alloc_vector(vec![inner, TaggedValue::fixnum(7)]);
        let dead_string = heap.alloc_string("dead".to_string());

        let roots = TestRoots {
            values: vec![vec],
            ..Default::default()
        };
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_marked.strings, 1);
        assert_eq!(stats.last_swept.strings, 1);
        assert_eq!(heap.free_strings, vec![dead_string.heap_index()]);
        assert_eq!(heap.get_string_as_utf8(s), "live");
    }

    #[test]
    fn unreachable_cycle_reclaimed_reachable_cycle_survives() {
        let mut heap = Heap::new();

        let dead = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        heap.set_cdr(dead, dead);

        let live = heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
        heap.set_cdr(live, live);

        let roots = TestRoots {
            values: vec![live],
            ..Default::default()
        };
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_swept.pairs, 1);
        assert_eq!(heap.free_pairs, vec![dead.heap_index()]);
        assert_eq!(heap.cdr(live), live);
    }

    #[test]
    fn tombstone_drops_rc_payload_breaking_env_cycle() {
        let shared = crate::heap::new_shared_heap();
        let env = Rc::new(Environment::with_heap(shared.clone()));

        // heap → env edge (owning Rc inside the heap slot)…
        let spec = shared
            .borrow_mut()
            .alloc_environment_specifier(env.clone(), false);
        // …and env → heap edge (bare index in the binding map): a full cycle.
        env.define("self".to_string(), spec);
        assert_eq!(Rc::strong_count(&env), 2);

        {
            let mut heap = shared.borrow_mut();
            let stats = collect(&mut heap, &TestRoots::default());
            assert_eq!(stats.last_swept.objects, 1);
        }
        // Sweep tombstoned the slot, dropping its Rc<Environment>: the cycle
        // is broken even though environments are not GC-managed.
        assert_eq!(Rc::strong_count(&env), 1);
    }

    #[test]
    fn env_bindings_are_roots() {
        let shared = crate::heap::new_shared_heap();
        let env = Rc::new(Environment::with_heap(shared.clone()));

        let live = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        env.define("live".to_string(), live);
        let dead = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);

        let roots = TestRoots {
            envs: vec![env.clone()],
            ..Default::default()
        };
        let mut heap = shared.borrow_mut();
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_marked.pairs, 1);
        assert_eq!(heap.free_pairs, vec![dead.heap_index()]);
        assert_eq!(heap.car(live), TaggedValue::fixnum(1));
    }

    #[test]
    fn parent_env_chain_is_traced() {
        let shared = crate::heap::new_shared_heap();
        let parent = Rc::new(Environment::with_heap(shared.clone()));
        let live = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        parent.define("live".to_string(), live);
        let child = Rc::new(Environment::with_parent(parent));

        let roots = TestRoots {
            envs: vec![child],
            ..Default::default()
        };
        let mut heap = shared.borrow_mut();
        collect(&mut heap, &roots);

        assert!(heap.free_pairs.is_empty());
        assert_eq!(heap.car(live), TaggedValue::fixnum(1));
    }

    #[test]
    fn interned_symbols_are_immortal() {
        let mut heap = Heap::new();
        let sym = heap.intern_symbol("kept-alive");

        let stats = collect(&mut heap, &TestRoots::default());

        assert_eq!(stats.last_swept.objects, 0);
        assert_eq!(heap.get_symbol_name(sym), Some("kept-alive"));
    }

    #[test]
    fn mutable_cell_and_values_are_traced() {
        let mut heap = Heap::new();
        let boxed = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let cell = heap.alloc_mutable_cell(boxed);
        let grouped = heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
        let values = heap.alloc_values(vec![grouped]);

        let roots = TestRoots {
            values: vec![cell, values],
            ..Default::default()
        };
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_swept.pairs, 0);
        assert_eq!(heap.car(boxed), TaggedValue::fixnum(1));
        assert_eq!(heap.car(grouped), TaggedValue::fixnum(2));
    }

    #[test]
    fn procedure_body_literals_are_traced() {
        let shared = crate::heap::new_shared_heap();
        let literal = shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(42), TaggedValue::NULL);

        let env = Rc::new(Environment::with_heap(shared.clone()));
        let proc = Rc::new(Procedure::CpsLambda {
            params: vec![],
            variadic: None,
            cont_param: Rc::from("k"),
            body: CpsExpr::rc(CpsExprKind::Literal(literal)),
            env,
            binding_scopes: Rc::new(crate::ScopeSet::new()),
        });
        let proc_tv = shared.borrow_mut().alloc_procedure(proc);

        let roots = TestRoots {
            values: vec![proc_tv],
            ..Default::default()
        };
        let mut heap = shared.borrow_mut();
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_swept.pairs, 0);
        assert_eq!(heap.car(literal), TaggedValue::fixnum(42));
    }

    #[test]
    fn already_free_slots_are_not_double_freed() {
        let mut heap = Heap::new();
        heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);

        collect(&mut heap, &TestRoots::default());
        assert_eq!(heap.free_pairs.len(), 2);

        let stats = collect(&mut heap, &TestRoots::default());
        assert_eq!(stats.last_swept.pairs, 0);
        assert_eq!(heap.free_pairs.len(), 2);
    }

    #[test]
    fn arena_length_plateaus_under_alloc_and_drop() {
        let mut heap = Heap::new();
        let mut collector = MarkSweepCollector::new();
        let roots = TestRoots::default();

        for _ in 0..10 {
            for i in 0..100 {
                heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
            }
            collector.collect(&mut heap, &[&roots]);
        }
        // Every round's garbage is reclaimed, so the arena never grows past
        // one round's worth of pairs.
        assert_eq!(heap.pairs.len(), 100);
    }

    #[test]
    fn byte_counter_and_adaptive_threshold() {
        let mut heap = Heap::new();
        let mut collector = MarkSweepCollector::with_min_bytes(10 * PAIR_SLOT_BYTES);
        assert_eq!(collector.auto_threshold(), 10 * PAIR_SLOT_BYTES);

        for i in 0..9 {
            heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
        }
        assert_eq!(heap.allocs_since_gc(), 9);
        assert_eq!(heap.bytes_since_gc(), 9 * PAIR_SLOT_BYTES);
        assert!(heap.bytes_since_gc() < collector.auto_threshold());

        heap.alloc_pair(TaggedValue::fixnum(9), TaggedValue::NULL);
        assert!(heap.bytes_since_gc() >= collector.auto_threshold());

        collector.collect(&mut heap, &[&TestRoots::default()]);
        assert_eq!(heap.allocs_since_gc(), 0);
        assert_eq!(heap.bytes_since_gc(), 0);
        // Nothing survived (no roots), so the adaptive term stays at the
        // floor.
        assert_eq!(heap.live_bytes(), 0);
        assert_eq!(collector.auto_threshold(), 10 * PAIR_SLOT_BYTES);

        // A live vector of 100 elements: L is its slot and its elements, and
        // the next interval is twice that.
        let kept = heap.alloc_vector_fill(100, TaggedValue::NULL);
        let roots = TestRoots {
            values: vec![kept],
            ..Default::default()
        };
        collector.collect(&mut heap, &[&roots]);
        let live = VECTOR_SLOT_BYTES + 100 * size_of::<TaggedValue>();
        assert_eq!(heap.live_bytes(), live);
        assert_eq!(collector.auto_threshold(), 2 * live);
    }

    /// #606: one object's payload is charged with its slot, so a large
    /// vector costs the trigger what it occupies, not what a pair does.
    #[test]
    fn allocation_charges_the_slot_and_the_payload() {
        /// The bytes `alloc` charged the trigger.
        fn charged(heap: &mut Heap, alloc: impl FnOnce(&mut Heap)) -> usize {
            let before = heap.bytes_since_gc();
            alloc(heap);
            heap.bytes_since_gc() - before
        }
        let shared = crate::heap::new_shared_heap();
        let env = Rc::new(Environment::with_heap(shared.clone()));
        let heap = &mut *shared.borrow_mut();
        let value = size_of::<TaggedValue>();
        let pair = charged(heap, |h| {
            h.alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        });
        assert_eq!(pair, PAIR_SLOT_BYTES);
        let vector = charged(heap, |h| {
            h.alloc_vector_fill(100_000, TaggedValue::fixnum(0));
        });
        assert_eq!(vector, VECTOR_SLOT_BYTES + 100_000 * value);
        let string = charged(heap, |h| {
            h.alloc_string_chars(vec!['x'; 1000]);
        });
        assert_eq!(string, STRING_SLOT_BYTES + 1000 * 4);
        let bytevector = charged(heap, |h| {
            h.alloc_bytevector(vec![0; 5000]);
        });
        assert_eq!(bytevector, OBJECT_SLOT_BYTES + 5000);
        // 2^200 has 201 bits: four 64-bit limbs.
        let bignum = charged(heap, |h| {
            h.alloc_bigint(num_bigint::BigInt::from(1) << 200);
        });
        assert_eq!(bignum, OBJECT_SLOT_BYTES + 32);
        let real = charged(heap, |h| {
            h.alloc_real(1.5);
        });
        assert_eq!(real, OBJECT_SLOT_BYTES);
        let values = charged(heap, |h| {
            h.alloc_values(vec![TaggedValue::NULL; 3]);
        });
        assert_eq!(values, OBJECT_SLOT_BYTES + 3 * value);
        let closure = charged(heap, |h| {
            h.alloc_vm_closure(1, vec![TaggedValue::NULL; 7], env.clone());
        });
        assert_eq!(closure, OBJECT_SLOT_BYTES + 7 * value);
        let continuation = charged(heap, |h| {
            h.alloc_vm_continuation_ref(160_000);
        });
        assert_eq!(continuation, OBJECT_SLOT_BYTES + 160_000);
        // A tree-walker closure (#637): its `Rc<Procedure>`, its parameters
        // and their spilled scope sets, its rest parameter's too, and the
        // frame it is assumed to capture. A primitive, its slot.
        let closure = tree_walker_closure(&env, &["x", "y"], Some("rest"));
        let Procedure::CpsLambda {
            params, variadic, ..
        } = &*closure
        else {
            unreachable!()
        };
        let scopes = params
            .iter()
            .chain(variadic)
            .map(|param| param.scopes.heap_bytes())
            .sum::<usize>();
        assert!(scopes >= 3 * 9 * size_of::<crate::ScopeId>());
        let lambda = charged(heap, |h| {
            h.alloc_procedure(closure);
        });
        assert_eq!(
            lambda,
            OBJECT_SLOT_BYTES
                + 2 * size_of::<usize>()
                + size_of::<Procedure>()
                + 2 * size_of::<crate::core_expr::ScopedParam>()
                + scopes
                + account::CAPTURED_FRAME_BYTES
        );
        let primitive = charged(heap, |h| {
            h.alloc_procedure(Procedure::primitive(
                "car",
                crate::procedure::Arity::Exact(1),
                Rc::from("scheme.base/car"),
                None,
            ));
        });
        assert_eq!(primitive, OBJECT_SLOT_BYTES);
        // A closure's payload is charged with its slot, as one allocation.
        assert_eq!(heap.allocs_since_gc(), 11);
    }

    /// A tree-walker closure over `env` with parameters `params` and the
    /// rest parameter `variadic`, each bound under a scope set of nine
    /// scopes, which spills.
    fn tree_walker_closure(
        env: &Rc<Environment>,
        params: &[&str],
        variadic: Option<&str>,
    ) -> Rc<Procedure> {
        let mut scopes = crate::ScopeSet::new();
        for id in 0..9 {
            scopes.add_scope(crate::ScopeId(2000 + id));
        }
        let param = |name: &str| crate::core_expr::ScopedParam {
            name: Rc::from(name),
            scopes: scopes.clone(),
        };
        Rc::new(Procedure::CpsLambda {
            params: params.iter().map(|name| param(name)).collect(),
            variadic: variadic.map(param),
            cont_param: Rc::from("k"),
            body: CpsExpr::rc(CpsExprKind::Literal(TaggedValue::NULL)),
            env: env.clone(),
            binding_scopes: Rc::new(crate::ScopeSet::new()),
        })
    }

    /// Every byte allocation charges is either reclaimed by a sweep or still
    /// occupied, and after a collection what is occupied is L: allocation,
    /// marking and sweep measure each payload the same way. And
    /// `committed_bytes`, which takes the tree-walker closures' payloads from
    /// the account rather than measuring them, agrees with a measurement.
    #[test]
    fn the_byte_account_balances() {
        fn occupied(heap: &Heap) -> u64 {
            let slots = (heap.pairs.len() - heap.free_pairs.len()) * PAIR_SLOT_BYTES
                + (heap.vectors.len() - heap.free_vectors.len()) * VECTOR_SLOT_BYTES
                + (heap.strings.len() - heap.free_strings.len()) * STRING_SLOT_BYTES
                + (heap.objects.len() - heap.free_objects.len()) * OBJECT_SLOT_BYTES;
            (slots + payloads(heap)) as u64
        }
        // What `committed_bytes` should report, measured: every arena's
        // capacity in slots and the occupied slots' payloads, the tree-walker
        // closures' among them, which `committed_bytes` reads from the
        // account instead.
        fn committed(heap: &Heap) -> usize {
            heap.pairs.capacity() * PAIR_SLOT_BYTES
                + heap.vectors.capacity() * VECTOR_SLOT_BYTES
                + heap.strings.capacity() * STRING_SLOT_BYTES
                + heap.objects.capacity() * OBJECT_SLOT_BYTES
                + payloads(heap)
        }
        fn payloads(heap: &Heap) -> usize {
            // A free slot holds a tombstone, whose payload is zero.
            heap
                .vectors
                .iter()
                .map(account::vector_payload)
                .sum::<usize>()
                + heap
                    .strings
                    .iter()
                    .map(account::string_payload)
                    .sum::<usize>()
                + heap
                    .objects
                    .iter()
                    .map(HeapObjectData::payload_bytes)
                    .sum::<usize>()
                // The tree-walker closures', which `payload_bytes` leaves to
                // a path of their own (`heap/account.rs`).
                + heap
                    .objects
                    .iter()
                    .map(|data| match data {
                        HeapObjectData::Procedure(procedure) => {
                            account::procedure_payload(procedure)
                        }
                        _ => 0,
                    })
                    .sum::<usize>()
        }

        let shared = crate::heap::new_shared_heap();
        let env = Rc::new(Environment::with_heap(shared.clone()));
        let mut heap = shared.borrow_mut();
        let mut roots = TestRoots::default();
        // Each kind twice: one kept, one garbage.
        for keep in [true, false] {
            let mut scopes = crate::ScopeSet::new();
            for id in 0..5 {
                scopes.add_scope(crate::ScopeId(1000 + id));
            }
            let rtd = Rc::new(crate::record_type::RecordTypeDescriptor {
                id: 0,
                name: Rc::from("point"),
                fields: vec![Rc::from("x"), Rc::from("y")],
            });
            let made = [
                heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL),
                heap.alloc_vector_fill(300, TaggedValue::NULL),
                heap.alloc_str("payload"),
                heap.alloc_bytevector(vec![7; 77]),
                heap.alloc_bigint(num_bigint::BigInt::from(3) << 300),
                heap.alloc_rational(num_rational::BigRational::new(
                    num_bigint::BigInt::from(1) << 100,
                    num_bigint::BigInt::from(3),
                )),
                heap.alloc_record(rtd, Rc::new(RefCell::new(vec![TaggedValue::NULL; 2]))),
                heap.alloc_exception(
                    crate::error::ExceptionKind::Custom("kind".into()),
                    "message".into(),
                    vec![TaggedValue::NULL; 4],
                ),
                heap.alloc_identifier(Rc::from("x"), scopes),
                heap.alloc_values(vec![TaggedValue::NULL; 5]),
                heap.alloc_vm_closure(1, vec![TaggedValue::NULL; 6], env.clone()),
                heap.alloc_vm_continuation_ref(4096).0,
                heap.alloc_vm_delimited_continuation_ref(2048).0,
                heap.alloc_procedure(tree_walker_closure(&env, &["p", "q", "r"], Some("s"))),
                heap.alloc_procedure(Procedure::primitive(
                    "car",
                    crate::procedure::Arity::Exact(1),
                    Rc::from("scheme.base/car"),
                    None,
                )),
            ];
            if keep {
                roots.values.extend(made);
            }
        }
        heap.intern_symbol("a-symbol-the-table-roots");
        // The closures' namespace, charged once however many name it: in the
        // bytes allocated and in L, and never reclaimed while it lives.
        let external = heap.external_bytes() as u64;
        assert_eq!(Some(external as usize), env.charged_bytes());
        assert_eq!(heap.bytes_allocated(), occupied(&heap) + external);
        assert_eq!(heap.bytes_reclaimed(), 0);
        assert_eq!(heap.committed_bytes(), committed(&heap));

        MarkSweepCollector::new().collect(&mut heap, &[&roots]);
        assert!(heap.bytes_reclaimed() > 0);
        assert_eq!(
            heap.bytes_allocated() - heap.bytes_reclaimed(),
            occupied(&heap) + external
        );
        assert_eq!(heap.live_bytes() as u64, occupied(&heap) + external);
        assert_eq!(heap.committed_bytes(), committed(&heap));

        // Dropping the roots frees the rest, and the account still balances.
        let allocated = heap.bytes_allocated();
        MarkSweepCollector::new().collect(&mut heap, &[&TestRoots::default()]);
        assert_eq!(heap.bytes_allocated(), allocated);
        assert_eq!(
            heap.bytes_allocated() - heap.bytes_reclaimed(),
            occupied(&heap) + external
        );
        assert_eq!(heap.live_bytes() as u64, occupied(&heap) + external);
        assert_eq!(heap.committed_bytes(), committed(&heap));
    }

    /// Sweep credits the dead tree-walker closures' payloads in one sum
    /// (#637), and a check build holds the sum to a measurement of each dead
    /// closure: one that entered the heap without `alloc_procedure`'s charge
    /// fails the first collection after it, rather than leaving the account
    /// short.
    #[test]
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    #[should_panic(expected = "the tree-walker closures charged")]
    fn an_uncharged_closure_fails_the_sweep_check() {
        let shared = crate::heap::new_shared_heap();
        let env = Rc::new(Environment::with_heap(shared.clone()));
        let mut heap = shared.borrow_mut();
        heap.alloc_object(HeapObjectData::Procedure(tree_walker_closure(
            &env,
            &["x"],
            None,
        )));
        MarkSweepCollector::new().collect(&mut heap, &[&TestRoots::default()]);
    }

    /// #606's first program in miniature: large vectors, each garbage at
    /// once, raise the pending flag after 8 MiB of them, where an object
    /// count would have waited for 65,536 of them.
    #[test]
    fn the_adaptive_trigger_counts_bytes() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        let controller = GcController::new(GcMode::On);
        heap.set_gc_threshold(controller.current_threshold());

        // 8 MiB of pairs is half a million of them; a few thousand do not
        // cross it.
        for i in 0..5000 {
            heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
        }
        assert!(!pending.get());
        // Ten 100,000-element vectors are 8 MB of elements: the eleventh
        // crosses the floor.
        for _ in 0..10 {
            heap.alloc_vector_fill(100_000, TaggedValue::fixnum(0));
        }
        assert!(!pending.get(), "{} bytes", heap.bytes_since_gc());
        heap.alloc_vector_fill(100_000, TaggedValue::fixnum(0));
        assert!(pending.get(), "{} bytes", heap.bytes_since_gc());
    }

    /// Stress counts allocations, whatever their size, so its lanes' pinned
    /// collection counts do not move with the byte trigger.
    #[test]
    fn stress_counts_allocations_not_bytes() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        let controller = GcController::new(GcMode::Stress(3));
        heap.set_gc_threshold(controller.current_threshold());

        heap.alloc_vector_fill(2_000_000, TaggedValue::fixnum(0));
        heap.alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        assert!(!pending.get(), "16 MB in two allocations");
        heap.alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        assert!(pending.get(), "the third allocation");
    }

    /// L counts the payloads of the VM continuations a collection proves
    /// live, so a program that keeps its captures raises its interval with
    /// them instead of collecting every 8 MiB of capture; a capture that
    /// dies is reclaimed with its handle.
    #[test]
    fn live_continuation_snapshots_count_in_l() {
        let mut heap = Heap::new();
        let mut controller = GcController::new(GcMode::On);
        let snapshot = 4 << 20;
        let mut roots = TestRoots::default();
        for _ in 0..3 {
            roots
                .values
                .push(heap.alloc_vm_continuation_ref(snapshot).0);
        }
        let (_dead, _) = heap.alloc_vm_continuation_ref(snapshot);

        controller.collect(&mut heap, &[&roots]);
        assert!(heap.live_bytes() >= 3 * snapshot);
        assert!(heap.live_bytes() < 3 * snapshot + 4096);
        assert!(heap.bytes_reclaimed() >= snapshot as u64);
        // The interval is twice the snapshots kept, not the 8 MiB floor.
        let threshold = controller.current_threshold();
        assert_eq!(threshold.bytes, 2 * heap.live_bytes());
        assert!(threshold.bytes > 3 * DEFAULT_MIN_BYTES);
    }

    /// External bytes (GC_PRD §15): charged like an allocation, counted in L
    /// at each collection until they are released, and never counted as
    /// reclaimed. `committed-bytes` is the arenas' alone: GC_PRD's footprint
    /// adds the external bytes to it, so it must not hold them already.
    #[test]
    fn external_bytes_count_toward_the_trigger_and_l() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        heap.set_gc_threshold(GcThreshold::bytes(1 << 20));
        heap.charge_external_bytes(1 << 20);
        assert!(pending.get());
        assert_eq!(heap.external_bytes(), 1 << 20);
        assert_eq!(heap.bytes_allocated(), 1 << 20);

        let mut collector = MarkSweepCollector::new();
        collector.collect(&mut heap, &[&TestRoots::default()]);
        assert_eq!(heap.live_bytes(), 1 << 20);
        assert_eq!(heap.committed_bytes(), 0, "an empty heap's arenas");
        assert_eq!(heap.stats().external_bytes, 1 << 20);

        heap.release_external_bytes(1 << 20);
        collector.collect(&mut heap, &[&TestRoots::default()]);
        assert_eq!(heap.live_bytes(), 0);
        assert_eq!(heap.bytes_reclaimed(), 0);

        // A size an embedder got wrong saturates the count the trigger
        // compares rather than wrapping it past the threshold, and the
        // collection it brings on settles the account without overflowing.
        heap.charge_external_bytes(1);
        heap.charge_external_bytes(usize::MAX);
        assert_eq!(heap.bytes_since_gc(), usize::MAX);
        assert_eq!(heap.external_bytes(), usize::MAX);
        assert!(pending.get());
        collector.collect(&mut heap, &[&TestRoots::default()]);
        assert_eq!(heap.bytes_allocated(), u64::MAX);
        assert_eq!(heap.live_bytes(), usize::MAX);
        heap.release_external_bytes(usize::MAX);
        assert_eq!(heap.external_bytes(), 0);
    }

    /// A holder of external bytes usually dies inside a sweep, which holds
    /// the heap mutably while it drops the dead slots' `Rc` payloads, so it
    /// gives its bytes back through a handle that needs no heap borrow; the
    /// collection's L then leaves them out.
    #[test]
    fn external_bytes_released_during_a_collection_are_not_live() {
        /// Gives external bytes back from inside the collection, as a
        /// holder's `Drop` would while the sweep runs.
        struct ReleasingRoots {
            handle: crate::heap::ExternalBytes,
            bytes: usize,
        }
        impl GcRoots for ReleasingRoots {
            fn trace_roots(&self, _visitor: &mut GcVisitor<'_>) {}
            fn sweep_weak(&self, _visitor: &GcVisitor<'_>) {
                self.handle.release(self.bytes);
            }
        }

        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        let handle = shared.borrow().external_bytes_handle();
        shared.borrow_mut().charge_external_bytes(3 << 20);
        {
            // What a `Drop` during a sweep sees: the heap borrowed mutably.
            let _sweeping = shared.borrow_mut();
            handle.release(1 << 20);
        }
        assert_eq!(shared.borrow().external_bytes(), 2 << 20);

        let roots = ReleasingRoots {
            handle: handle.clone(),
            bytes: 1 << 20,
        };
        let mut controller = GcController::new(GcMode::On);
        controller.collect(&mut shared.borrow_mut(), &[&roots]);
        assert_eq!(handle.held(), 1 << 20);
        assert_eq!(shared.borrow().live_bytes(), 1 << 20);
    }

    /// A charge through the handle needs no heap, and raises the pending
    /// flag when it crosses the byte threshold, as an allocation does: a
    /// namespace charges as its tables grow, wherever a definition lands.
    #[test]
    fn an_external_charge_through_the_handle_raises_the_pending_flag() {
        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        let handle = shared.borrow().external_bytes_handle();
        let pending = shared.borrow().gc_pending_handle();
        shared
            .borrow_mut()
            .set_gc_threshold(GcThreshold::bytes(1 << 20));
        // What a definition under a heap borrow sees.
        let borrowed = shared.borrow_mut();
        handle.charge((1 << 20) - 1);
        assert!(!pending.get());
        handle.charge(1);
        assert!(pending.get());
        assert_eq!(handle.held(), 1 << 20);
        drop(borrowed);
        assert_eq!(shared.borrow().bytes_since_gc(), 1 << 20);
        handle.release(1 << 20);
    }

    /// A namespace a dead specifier held drops inside the sweep that frees
    /// the specifier, with the heap borrowed mutably, and gives back its
    /// tables through its handle; the collection's L leaves them out, and the
    /// live namespace's tables stay in it.
    #[test]
    fn a_namespace_dropped_by_a_sweep_gives_its_tables_back() {
        let shared: SharedHeap = Rc::new(RefCell::new(Heap::new()));
        let kept = Rc::new(Environment::with_heap(shared.clone()));
        let dead = Rc::new(Environment::with_heap(shared.clone()));
        for i in 0..100 {
            dead.define(format!("binding-{i}"), TaggedValue::fixnum(i));
        }
        let kept_charge = kept.charged_bytes().unwrap();
        // So that the account's falling to `kept_charge` below shows a
        // release: the hundred bindings are charged on top of what an empty
        // namespace is.
        assert!(dead.charged_bytes().unwrap() > kept_charge);
        assert_eq!(
            shared.borrow().external_bytes(),
            kept_charge + dead.charged_bytes().unwrap()
        );
        let mut roots = TestRoots::default();
        {
            let mut heap = shared.borrow_mut();
            roots
                .values
                .push(heap.alloc_environment_specifier(Rc::clone(&kept), false));
            heap.alloc_environment_specifier(dead, false);
        }
        drop(kept);

        let mut controller = GcController::new(GcMode::On);
        controller.collect(&mut shared.borrow_mut(), &[&roots]);
        let heap = shared.borrow();
        assert_eq!(heap.external_bytes(), kept_charge);
        assert!(heap.live_bytes() >= kept_charge + OBJECT_SLOT_BYTES);
        assert!(heap.live_bytes() < kept_charge + 4 * OBJECT_SLOT_BYTES);
    }

    /// An open file port is charged its buffer and its place in descriptor
    /// pressure when it gets its heap object, once, and gives both back when
    /// it closes: explicitly, or by dropping in the sweep that finds it dead.
    /// The count reaching the threshold posts a collection, once while one is
    /// pending; the collection starts the count again, and closing a port it
    /// found live gives back its bytes but takes nothing off the ports opened
    /// since (#607).
    #[test]
    fn a_file_port_is_charged_until_it_closes() {
        use crate::heap::FILE_PORT_BYTES;
        use crate::port::Port;

        let fs = crate::vfs::MemoryFs::new();
        fs.add_text_file("/f", "hello");
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        heap.set_gc_threshold(GcThreshold::NEVER.with_descriptors(3));
        let open = |heap: &mut Heap| heap.alloc_port(Port::open_input_file("/f", &fs).unwrap());
        let held = |heap: &Heap| (heap.descriptors_since_gc(), heap.external_bytes());

        let kept = open(&mut heap);
        let closed = open(&mut heap);
        assert_eq!(held(&heap), (2, 2 * FILE_PORT_BYTES));
        heap.get_port(closed).unwrap().close();
        assert_eq!(held(&heap), (1, FILE_PORT_BYTES));
        // Closed already: nothing more to give back.
        heap.get_port(closed).unwrap().close();
        assert_eq!(held(&heap), (1, FILE_PORT_BYTES));

        // Neither a port that is not a file's nor a second heap object for
        // a file port is charged.
        heap.alloc_port(Port::new_input_string("text".into()));
        let again = Rc::clone(heap.get_port(kept).unwrap());
        let kept_again = heap.alloc_port(again);
        assert_eq!(held(&heap), (1, FILE_PORT_BYTES));

        open(&mut heap);
        assert!(!pending.get());
        open(&mut heap);
        assert!(pending.get(), "three open: a collection is posted");
        assert_eq!(heap.descriptor_collections(), 1);
        open(&mut heap);
        assert_eq!(heap.descriptor_collections(), 1, "one is pending already");
        assert_eq!(held(&heap), (4, 4 * FILE_PORT_BYTES));

        let roots = TestRoots {
            values: vec![kept, kept_again],
            ..TestRoots::default()
        };
        collect(&mut heap, &roots);
        assert!(!pending.get());
        assert_eq!(
            held(&heap),
            (0, FILE_PORT_BYTES),
            "the dead ports closed in the sweep, and the live one counts no more"
        );

        let opened_since = open(&mut heap);
        heap.get_port(kept).unwrap().close();
        assert_eq!(held(&heap), (1, FILE_PORT_BYTES));
        heap.get_port(opened_since).unwrap().close();
        assert_eq!(held(&heap), (0, 0));
    }

    /// Descriptor pressure is part of every mode that collects on its own,
    /// and not of `PATINA_GC=0`'s (#607).
    #[test]
    fn descriptor_pressure_follows_the_mode() {
        let threshold = crate::heap::descriptor_pressure_threshold();
        assert!((1..=128).contains(&threshold), "{threshold}");
        for mode in [GcMode::On, GcMode::Stress(16), GcMode::Zeal] {
            assert_eq!(
                GcController::new(mode).current_threshold().descriptors,
                threshold,
                "{mode}"
            );
        }
        assert_eq!(
            GcController::new(GcMode::Off).current_threshold(),
            GcThreshold::NEVER
        );
    }

    #[test]
    fn alloc_crossing_threshold_raises_pending_flag() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        heap.set_gc_threshold(GcThreshold::allocations(3));

        heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
        assert!(!pending.get());

        heap.alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL);
        assert!(pending.get());

        // Sweep is the "collection happened" boundary: it lowers the flag
        // along with the counter.
        collect(&mut heap, &TestRoots::default());
        assert!(!pending.get());
        assert_eq!(heap.allocs_since_gc(), 0);
    }

    #[test]
    fn request_gc_raises_pending_flag_in_any_mode() {
        // A bare heap's threshold is the inert usize::MAX, so only `(gc)`
        // can raise the flag.
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();

        heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        assert!(!pending.get());

        heap.request_gc();
        assert!(pending.get());

        collect(&mut heap, &TestRoots::default());
        assert!(!pending.get());
    }

    #[test]
    fn lowering_threshold_below_counter_raises_flag_immediately() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        for i in 0..5 {
            heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
        }
        assert!(!pending.get());

        heap.set_gc_threshold(GcThreshold::allocations(4));
        assert!(pending.get());
    }

    #[test]
    fn controller_collect_rearms_adaptive_threshold() {
        let mut heap = Heap::new();
        let pending = heap.gc_pending_handle();
        // Simulate GcMode::On with a tiny floor so the adaptive `2·L` term
        // dominates.
        let mut controller = GcController {
            mode: GcMode::On,
            collector: MarkSweepCollector::with_min_bytes(1),
        };

        // Two live pairs held by a root, one garbage.
        let a = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        let b = heap.alloc_pair(TaggedValue::fixnum(2), TaggedValue::NULL);
        heap.alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL);
        let roots = TestRoots {
            values: vec![a, b],
            ..Default::default()
        };
        controller.collect(&mut heap, &[&roots]);
        assert!(!pending.get());

        // L is two pairs, so the re-armed threshold is max(1, 2·L), four
        // pairs' worth: three allocations stay quiet, the fourth raises the
        // flag.
        for i in 0..3 {
            heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
        }
        assert!(!pending.get());
        heap.alloc_pair(TaggedValue::fixnum(9), TaggedValue::NULL);
        assert!(pending.get());
    }

    #[test]
    fn safe_point_collects_only_when_outermost_and_pending() {
        let shared = crate::heap::new_shared_heap();
        let pending = shared.borrow().gc_pending_handle();
        let gc = RefCell::new(GcController::from_env());
        let no_roots = TestRoots::default();
        // A safe point runs inside its loop's own guard, and a collection
        // asserts it is the only one alive.
        let _loop_guard = GcDeferGuard::new(&shared);

        shared
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);

        // Flag down: nothing happens.
        let collected = GcController::safe_point(&gc, &shared, &pending, true, |collect| {
            collect(&[&no_roots]);
        });
        assert!(!collected);
        assert_eq!(shared.borrow().gc_collections(), 0);

        // Flag up but nested: deferred, flag stays up for the outer loop.
        shared.borrow_mut().request_gc();
        let collected = GcController::safe_point(&gc, &shared, &pending, false, |collect| {
            collect(&[&no_roots]);
        });
        assert!(!collected);
        assert_eq!(shared.borrow().gc_collections(), 0);
        assert!(pending.get());

        // Flag up, outermost, but the backend cannot supply its roots: no
        // collection, and the flag stays up for the next safe point.
        let collected = GcController::safe_point(&gc, &shared, &pending, true, |_collect| {});
        assert!(!collected);
        assert!(pending.get());

        // Flag up and outermost: collects and lowers the flag.
        let collected = GcController::safe_point(&gc, &shared, &pending, true, |collect| {
            collect(&[&no_roots]);
        });
        assert!(collected);
        assert_eq!(shared.borrow().gc_collections(), 1);
        assert!(!pending.get());
    }

    /// Zeal (#625): the collection that lowers the pending flag re-installs a
    /// threshold of 0, which raises it again, so every outermost safe point
    /// collects whether or not anything was allocated, and `safe_point` says
    /// that it did, since the flag cannot.
    #[test]
    fn zeal_collects_at_every_outermost_safe_point() {
        let shared = crate::heap::new_shared_heap();
        let pending = shared.borrow().gc_pending_handle();
        let gc = RefCell::new(GcController {
            mode: GcMode::Zeal,
            collector: MarkSweepCollector::new(),
        });
        let no_roots = TestRoots::default();
        let _loop_guard = GcDeferGuard::new(&shared);

        let threshold = gc.borrow().current_threshold();
        shared.borrow_mut().set_gc_threshold(threshold);
        assert!(pending.get(), "installing zeal's threshold raises the flag");

        for n in 1..=3 {
            let collected = GcController::safe_point(&gc, &shared, &pending, true, |collect| {
                collect(&[&no_roots]);
            });
            assert!(collected);
            assert_eq!(shared.borrow().gc_collections(), n);
            assert!(pending.get(), "raised again by the collection itself");
        }
        // Nested loops still never collect.
        let collected = GcController::safe_point(&gc, &shared, &pending, false, |collect| {
            collect(&[&no_roots]);
        });
        assert!(!collected);
        assert_eq!(shared.borrow().gc_collections(), 3);
    }

    /// A collection a primitive asks for at its call (#639): under the
    /// running loop's own guard alone it collects, in every mode; where
    /// collection is deferred, or the backend cannot supply its roots, or no
    /// loop is running — a holder's guard alone, or none — it is posted for
    /// the next safe point that may collect, and counted.
    #[test]
    fn collect_at_call_collects_only_under_the_loops_own_guard() {
        let shared = crate::heap::new_shared_heap();
        let pending = shared.borrow().gc_pending_handle();
        // `PATINA_GC=0`'s mode: no automatic trigger, and still a collection.
        let gc = RefCell::new(GcController::new(GcMode::Off));
        let no_roots = TestRoots::default();
        // `roots` as the backend supplies them, or `None` where it cannot.
        let collect_at_call = |roots: Option<&TestRoots>| {
            GcController::collect_at_call(&gc, &shared, CollectKind::Major, |collect| {
                if let Some(roots) = roots {
                    collect(&[roots]);
                }
            })
        };
        let counts = || {
            let h = shared.borrow();
            (
                h.gc_collections(),
                h.gc_deferred_collections(),
                pending.get(),
            )
        };
        let loop_guard = GcDeferGuard::new(&shared);

        assert!(collect_at_call(Some(&no_roots)));
        assert_eq!(
            counts(),
            (1, 0, false),
            "collected at the call, nothing posted"
        );

        // A library load holding the registry: no roots, so no collection.
        assert!(!collect_at_call(None));
        assert_eq!(counts(), (1, 1, true), "posted and counted");

        // A nested loop, and a holder's extent: deferred whatever the roots.
        let nested = GcDeferGuard::new(&shared);
        assert!(!collect_at_call(Some(&no_roots)));
        drop(nested);
        let holder = GcDeferGuard::holding(&shared);
        assert!(!collect_at_call(Some(&no_roots)));
        drop(holder);
        assert_eq!(counts(), (1, 3, true));

        // What was posted collects at the outermost loop's next safe point.
        let collected = GcController::safe_point(&gc, &shared, &pending, true, |collect| {
            collect(&[&no_roots]);
        });
        assert!(collected);
        assert_eq!(counts(), (2, 3, false));
        drop(loop_guard);

        // A host outside any loop, holding values under a holder's guard:
        // the one guard alive is no loop's, so the depth of 1 does not let it
        // collect inside the holder's extent (a check build's holder would
        // panic on its drop if it had).
        let holder = GcDeferGuard::holding(&shared);
        assert_eq!(shared.borrow().gc_defer_depth(), 1);
        assert!(!collect_at_call(Some(&no_roots)));
        drop(holder);
        assert_eq!(counts(), (2, 4, true));
        // Nor with no guard at all: no loop is running.
        assert!(!collect_at_call(Some(&no_roots)));
        assert_eq!(counts(), (2, 5, true));
    }

    /// The test switch (#639): with it on, no safe point collects, however
    /// long a request waits; a collection at a call still does.
    #[cfg(feature = "test-support")]
    #[test]
    fn skipping_safe_points_leaves_the_collections_at_calls() {
        let shared = crate::heap::new_shared_heap();
        let pending = shared.borrow().gc_pending_handle();
        let gc = RefCell::new(GcController::new(GcMode::Off));
        let no_roots = TestRoots::default();
        let _loop_guard = GcDeferGuard::new(&shared);
        shared.borrow_mut().set_skip_safe_points(true);

        shared.borrow_mut().request_gc();
        let collected = GcController::safe_point(&gc, &shared, &pending, true, |collect| {
            collect(&[&no_roots]);
        });
        assert!(!collected);
        assert!(pending.get());
        assert!(GcController::collect_at_call(
            &gc,
            &shared,
            CollectKind::Major,
            |collect| collect(&[&no_roots])
        ));
        assert_eq!(shared.borrow().gc_collections(), 1);
    }

    // ------------------------------------------------------------------------
    // Positive controls for the stale-reference checks (#621). Each one runs
    // in every check build (`GC_CHECK`: debug, or release with `gc-check`);
    // a build without the checks reports it ignored instead of compiling it
    // out, so a lane that meant to run the checks and did not is visible.
    // ------------------------------------------------------------------------

    /// Free the slot `make` allocates, then collect again with a root that
    /// still names it. The slot is on the free list and not reused, so the
    /// only check that can see it is the sweep's pre-mark (§11 item 5).
    fn collect_with_a_root_naming_a_free_slot(make: impl FnOnce(&mut Heap) -> TaggedValue) {
        let mut heap = Heap::new();
        let dead = make(&mut heap);
        collect(&mut heap, &TestRoots::default());
        let roots = TestRoots {
            values: vec![dead],
            ..Default::default()
        };
        collect(&mut heap, &roots);
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "dangling reference: pair slot 0 is free, but marking reached it")]
    fn marking_a_free_pair_panics() {
        collect_with_a_root_naming_a_free_slot(|heap| {
            heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL)
        });
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "dangling reference: vector slot 0 is free, but marking reached it")]
    fn marking_a_free_vector_panics() {
        collect_with_a_root_naming_a_free_slot(|heap| {
            heap.alloc_vector(vec![TaggedValue::fixnum(1)])
        });
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "dangling reference: string slot 0 is free, but marking reached it")]
    fn marking_a_free_string_panics() {
        collect_with_a_root_naming_a_free_slot(|heap| heap.alloc_str("dead"));
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "dangling reference: object slot 0 is free, but marking reached it")]
    fn marking_a_free_object_panics() {
        collect_with_a_root_naming_a_free_slot(|heap| heap.alloc_bytevector(vec![1, 2, 3]));
    }

    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "use-after-free")]
    fn swept_pair_access_panics_in_debug() {
        let mut heap = Heap::new();
        let dead = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
        collect(&mut heap, &TestRoots::default());
        let _ = heap.car(dead);
    }

    #[test]
    fn shared_record_fields_with_live_holder_survive() {
        let mut heap = Heap::new();
        let field_val = heap.alloc_pair(TaggedValue::fixnum(7), TaggedValue::NULL);
        let fields = Rc::new(RefCell::new(vec![field_val]));
        let rtd = Rc::new(crate::record_type::RecordTypeDescriptor {
            id: 0,
            name: Rc::from("point"),
            fields: vec![Rc::from("x")],
        });
        let record = heap.alloc_record(rtd, fields);

        let roots = TestRoots {
            values: vec![record],
            ..Default::default()
        };
        let stats = collect(&mut heap, &roots);

        assert_eq!(stats.last_swept.pairs, 0);
        assert_eq!(heap.car(field_val), TaggedValue::fixnum(7));
    }

    /// The deferral protocol and the no-collection windows (#624).
    ///
    /// The `#[should_panic]` tests are positive controls: each makes the
    /// mistake its check exists for and expects that check's panic. The
    /// depth, holder and poll checks are compiled into check builds only
    /// (`GC_CHECK`: debug, or release with `gc-check`); a build without them
    /// reports those controls ignored rather than compiling them out. The
    /// balance check is in every build, so its control runs in every build.
    mod protocol {
        use super::*;
        use crate::heap::new_shared_heap;

        /// Raise a `(gc)` request and run a safe point that would collect,
        /// as the loop holding `loop_guard` would.
        fn safe_point_that_collects(shared: &SharedHeap, loop_guard: &GcDeferGuard) {
            let pending = shared.borrow().gc_pending_handle();
            let gc = RefCell::new(GcController::from_env());
            shared.borrow_mut().request_gc();
            GcController::safe_point(
                &gc,
                shared,
                &pending,
                loop_guard.is_outermost(),
                |collect| {
                    collect(&[&TestRoots::default()]);
                },
            );
        }

        #[test]
        fn the_outermost_loop_collects_under_its_own_guard_alone() {
            let shared = new_shared_heap();
            let loop_guard = GcDeferGuard::new(&shared);
            safe_point_that_collects(&shared, &loop_guard);
            assert_eq!(shared.borrow().gc_collections(), 1);
        }

        /// A guard a callee took and kept past its instruction: the running
        /// loop read `is_outermost` at entry, so nothing but the depth check
        /// stops it collecting under the second guard.
        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = "a collection ran with 2 defer guard(s) alive")]
        fn a_collection_while_a_second_guard_is_alive_panics() {
            let shared = new_shared_heap();
            let loop_guard = GcDeferGuard::new(&shared);
            let _kept = GcDeferGuard::new(&shared);
            safe_point_that_collects(&shared, &loop_guard);
        }

        #[test]
        fn a_holding_extent_with_no_collection_drops_quietly() {
            let shared = new_shared_heap();
            let holder = GcDeferGuard::holding(&shared);
            // A loop entered under a holder is nested, and does not collect.
            let loop_guard = GcDeferGuard::new(&shared);
            assert!(!loop_guard.is_outermost());
            safe_point_that_collects(&shared, &loop_guard);
            drop(loop_guard);
            drop(holder);
            assert_eq!(shared.borrow().gc_collections(), 0);
            assert_eq!(shared.borrow().gc_defer_depth(), 0);
        }

        /// What a nested loop allowed to collect (GC_PRD stage 4e) would do
        /// under a holder: the collection itself is direct here, because the
        /// depth check would stop one at a safe point first.
        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = "ran inside a GcDeferGuard::holding extent")]
        fn a_collection_inside_a_holding_extent_panics() {
            let shared = new_shared_heap();
            let holder = GcDeferGuard::holding(&shared);
            collect(&mut shared.borrow_mut(), &TestRoots::default());
            drop(holder);
        }

        /// A loop's guard does not check its extent: the outermost loop
        /// collects inside its own.
        #[test]
        fn a_loop_guard_allows_a_collection_inside_its_extent() {
            let shared = new_shared_heap();
            let loop_guard = GcDeferGuard::new(&shared);
            collect(&mut shared.borrow_mut(), &TestRoots::default());
            drop(loop_guard);
            assert_eq!(shared.borrow().gc_collections(), 1);
        }

        /// Runs in every build: the balance check is an `assert!`, where an
        /// underflow in a release build would wrap the depth and stop
        /// collection for good.
        #[test]
        #[should_panic(expected = "unbalanced GC defer: exit without a matching enter")]
        fn an_unbalanced_defer_exit_panics() {
            Heap::new().exit_gc_defer(false);
        }

        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = "GC poll inside an AssertNoGc scope (1 open)")]
        fn a_poll_inside_a_no_gc_scope_panics() {
            let shared = new_shared_heap();
            let polls = NoGcScopes::of(&shared);
            let _window = AssertNoGc::new(&shared);
            polls.assert_none_open();
        }

        /// A collection at a call is a poll site too (#639): it checks for an
        /// open window before it decides whether it may collect, so a nested
        /// one, which only posts, panics as well.
        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = "GC poll inside an AssertNoGc scope (1 open)")]
        fn a_collection_at_a_call_inside_a_no_gc_scope_panics() {
            let shared = new_shared_heap();
            let gc = RefCell::new(GcController::from_env());
            let _loop_guard = GcDeferGuard::new(&shared);
            let _nested = GcDeferGuard::new(&shared);
            let _window = AssertNoGc::new(&shared);
            GcController::collect_at_call(&gc, &shared, CollectKind::Major, |collect| {
                collect(&[&TestRoots::default()]);
            });
        }

        #[test]
        fn no_gc_scopes_nest_and_close() {
            let shared = new_shared_heap();
            let polls = NoGcScopes::of(&shared);
            {
                let _outer = AssertNoGc::new(&shared);
                let inner = AssertNoGc::new(&shared);
                // Closing one needs no heap borrow.
                let held = shared.borrow_mut();
                drop(inner);
                drop(held);
            }
            polls.assert_none_open();
        }
    }
}
