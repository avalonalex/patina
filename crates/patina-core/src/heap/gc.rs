//! Garbage collection infrastructure (stage 1 of `docs/GC_DESIGN.md`).
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
//! - [`Collector`] — the swappable algorithm. [`MarkSweepCollector`] is the
//!   v1 implementation shared by both backends. Implementations must be
//!   non-moving: live slots may never be relocated.
//!
//! Nothing in this module runs automatically; collection happens only when a
//! backend calls [`Collector::collect`] at a safe point (stages 2–3).

use std::rc::Rc;
use std::time::Instant;

use rustc_hash::FxHashSet;

use super::{Heap, HeapObjectData, PromiseState};
use crate::continuation::CpsContinuation;
use crate::environment::Environment;
use crate::procedure::Procedure;
use crate::tagged_value::{HeapIndex, TaggedValue};

// ============================================================================
// Bit-vectors
// ============================================================================

/// A fixed-size bit set, one bit per arena slot.
#[derive(Debug, Clone)]
pub struct BitSet {
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
    pub fn get(&self, i: usize) -> bool {
        debug_assert!(i < self.len, "bit index {i} out of range {}", self.len);
        self.words[i / 64] & (1u64 << (i % 64)) != 0
    }

    fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// Mark bits for all four arenas, sized to arena lengths at collection start.
#[derive(Debug, Clone)]
pub struct MarkBits {
    pub pairs: BitSet,
    pub vectors: BitSet,
    pub strings: BitSet,
    pub objects: BitSet,
}

impl MarkBits {
    fn for_heap(heap: &Heap) -> Self {
        Self {
            pairs: BitSet::new(heap.pairs.len()),
            vectors: BitSet::new(heap.vectors.len()),
            strings: BitSet::new(heap.strings.len()),
            objects: BitSet::new(heap.objects.len()),
        }
    }

    /// Live slots per arena.
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default)]
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
}

/// A pluggable collection algorithm. Implementations must be **non-moving**:
/// live slots may never be relocated, because raw arena indices escape
/// `TaggedValue` (symbol table, `SourceMap` keys, `eq?` semantics, VM
/// constants — design §3.4).
pub trait Collector {
    /// Whether a collection is warranted (checked by backends at safe points).
    fn should_collect(&self, heap: &Heap) -> bool;

    /// Run a full collection. The caller must be at a safe point: every live
    /// value reachable from `roots`, and no outstanding heap borrow other
    /// than the one behind `heap`.
    fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats;

    fn stats(&self) -> &GcStats;
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
    seen_envs: FxHashSet<usize>,
    seen_conts: FxHashSet<usize>,
    seen_exprs: FxHashSet<usize>,
}

impl<'h> GcVisitor<'h> {
    pub fn new(heap: &'h Heap) -> Self {
        Self {
            heap,
            marks: MarkBits::for_heap(heap),
            worklist: Vec::new(),
            cont_worklist: Vec::new(),
            seen_envs: FxHashSet::default(),
            seen_conts: FxHashSet::default(),
            seen_exprs: FxHashSet::default(),
        }
    }

    /// The normal edge: mark and enqueue a heap reference; no-op for
    /// immediates (fixnum, special, char).
    #[inline]
    pub fn visit(&mut self, tv: TaggedValue) {
        if tv.is_pair() {
            if self.marks.pairs.set(tv.heap_index() as usize) {
                self.worklist.push(tv);
            }
        } else if tv.is_vector() {
            if self.marks.vectors.set(tv.heap_index() as usize) {
                self.worklist.push(tv);
            }
        } else if tv.is_string() {
            self.marks.strings.set(tv.heap_index() as usize);
        } else if tv.is_object() && self.marks.objects.set(tv.heap_index() as usize) {
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
    /// (e.g. the VM's `CallFrame.closure`).
    pub fn visit_object_index(&mut self, index: HeapIndex) {
        if self.marks.objects.set(index as usize) {
            self.worklist.push(TaggedValue::object(index));
        }
    }

    /// Trace through an environment chain. Deduped by the identity of the
    /// shared bindings map, so the global environment is walked once no
    /// matter how many closures point at it.
    pub fn visit_env(&mut self, env: &Environment) {
        let mut current = Some(env);
        while let Some(e) = current {
            // If this env was already walked, its parents were too.
            if !self.seen_envs.insert(e.gc_identity()) {
                break;
            }
            e.for_each_local_value(&mut |tv| self.visit(tv));
            current = e.parent().map(|p| p.as_ref());
        }
    }

    fn visit_continuation(&mut self, k: &Rc<CpsContinuation>) {
        if self.seen_conts.insert(Rc::as_ptr(k) as usize) {
            self.cont_worklist.push(k.clone());
        }
    }

    /// Literals embedded in live code (`CpsExprKind::Literal` / `Quasiquote`).
    /// Memoized per collection by node address — bodies are shared via `Rc`
    /// across closures. Depth is bounded by program size, not data size, so
    /// recursion is acceptable here (data tracing stays iterative).
    fn visit_expr_literals(&mut self, expr: &crate::cps_expr::CpsExpr) {
        let mut seen = std::mem::take(&mut self.seen_exprs);
        expr.for_each_literal(&mut seen, &mut |tv| self.visit(tv));
        self.seen_exprs = seen;
    }

    /// Interned symbols are roots in v1: symbols are `eq?`-identity-bearing
    /// and never collected (design §9.2).
    fn trace_symbol_table(&mut self) {
        // Copy the indices out first: visit_object_index needs &mut self while
        // the table iterator would borrow self.heap through self.
        let indices: Vec<HeapIndex> = self.heap.symbol_table.values().copied().collect();
        for idx in indices {
            self.visit_object_index(idx);
        }
    }

    /// Process the worklists to a fixed point.
    fn drain(&mut self) {
        loop {
            if let Some(tv) = self.worklist.pop() {
                self.trace_children(tv);
            } else if let Some(k) = self.cont_worklist.pop() {
                self.trace_continuation_children(&k);
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
            for &element in &heap.vectors[idx] {
                self.visit(element);
            }
        } else if tv.is_object() {
            self.trace_object_children(&heap.objects[idx]);
        }
    }

    fn trace_object_children(&mut self, data: &'h HeapObjectData) {
        match data {
            // Leaves: no embedded heap references.
            HeapObjectData::BigInt(_)
            | HeapObjectData::Rational(_)
            | HeapObjectData::Real(_)
            | HeapObjectData::Symbol(_)
            | HeapObjectData::Bytevector(_)
            | HeapObjectData::Port(_)
            | HeapObjectData::RecordType(_)
            | HeapObjectData::Identifier { .. }
            | HeapObjectData::PromptTag(_)
            | HeapObjectData::LabelPlaceholder(_)
            | HeapObjectData::Free => {}

            // Leaves for the heap tracer: the payload lives in VmState's side
            // tables, which the VM's GcRoots impl traces directly (design §5.2).
            HeapObjectData::VmContinuationRef(_)
            | HeapObjectData::VmDelimitedContinuationRef(_) => {}

            HeapObjectData::Complex { real, imag } => {
                self.visit(*real);
                self.visit(*imag);
            }
            HeapObjectData::Exception { irritants, .. } => {
                for &irritant in irritants {
                    self.visit(irritant);
                }
            }
            HeapObjectData::Procedure(p) => match p.as_ref() {
                Procedure::Primitive { .. } => {}
                Procedure::CpsLambda { body, env, .. } => {
                    self.visit_expr_literals(body);
                    self.visit_env(env);
                }
            },
            HeapObjectData::Macro(m) => {
                m.for_each_literal(&mut |tv| self.visit(tv));
            }
            HeapObjectData::Record { fields, .. } => {
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
            HeapObjectData::Promise(state) => match *state.borrow() {
                PromiseState::Delayed(tv) | PromiseState::Forced(tv) => self.visit(tv),
            },
            HeapObjectData::Library(lib) => {
                for (_, tv) in lib.exports_iter_tagged() {
                    self.visit(tv);
                }
                self.visit_env(&lib.env);
            }
            HeapObjectData::Values(values) => {
                for &value in values {
                    self.visit(value);
                }
            }
            HeapObjectData::EnvironmentSpecifier { env, .. } => {
                self.visit_env(env);
            }
            HeapObjectData::MutableCell(cell) => {
                let inner = *cell.borrow();
                self.visit(inner);
            }
            HeapObjectData::VmClosure {
                free_vars, globals, ..
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

    fn trace_continuation_children(&mut self, k: &CpsContinuation) {
        self.visit_expr_literals(&k.body);
        self.visit_env(&k.env);
        for wind in &k.dynamic_winds {
            self.visit(wind.before);
            self.visit(wind.after);
        }
        for (_, captured) in &k.captured_cont_bindings {
            self.visit_continuation(captured);
        }
    }

    fn finish(self) -> MarkBits {
        self.marks
    }
}

// ============================================================================
// Sweep
// ============================================================================

impl Heap {
    /// Reclaim every unmarked slot: push its index onto the arena's free list
    /// and tombstone the slot. Tombstoning drops `Rc` payloads eagerly, which
    /// is what breaks closure ↔ environment cycles (design §8) — it is
    /// load-bearing, not hygiene.
    ///
    /// Slots already on a free list are skipped (they are unmarked by
    /// definition; re-pushing them would double-free on reuse).
    pub fn sweep(&mut self, marks: &MarkBits) -> ArenaCounts {
        fn already_free(free_list: &[HeapIndex], len: usize) -> BitSet {
            let mut set = BitSet::new(len);
            for &idx in free_list {
                set.set(idx as usize);
            }
            set
        }

        let mut swept = ArenaCounts::default();

        let free = already_free(&self.free_pairs, self.pairs.len());
        for i in 0..self.pairs.len() {
            if !marks.pairs.get(i) && !free.get(i) {
                self.pairs[i] = (TaggedValue::NULL, TaggedValue::NULL);
                self.free_pairs.push(i as HeapIndex);
                swept.pairs += 1;
            }
        }

        let free = already_free(&self.free_vectors, self.vectors.len());
        for i in 0..self.vectors.len() {
            if !marks.vectors.get(i) && !free.get(i) {
                self.vectors[i] = Vec::new();
                self.free_vectors.push(i as HeapIndex);
                swept.vectors += 1;
            }
        }

        let free = already_free(&self.free_strings, self.strings.len());
        for i in 0..self.strings.len() {
            if !marks.strings.get(i) && !free.get(i) {
                self.strings[i] = Vec::new();
                self.free_strings.push(i as HeapIndex);
                swept.strings += 1;
            }
        }

        let free = already_free(&self.free_objects, self.objects.len());
        for i in 0..self.objects.len() {
            if !marks.objects.get(i) && !free.get(i) {
                self.objects[i] = HeapObjectData::Free;
                self.free_objects.push(i as HeapIndex);
                swept.objects += 1;
            }
        }

        swept
    }
}

// ============================================================================
// MarkSweepCollector
// ============================================================================

/// Default allocation-count floor before a collection is considered.
pub const DEFAULT_MIN_THRESHOLD: usize = 65_536;

/// The v1 collector: stop-the-world mark-and-sweep, shared by both backends.
/// Adaptive trigger: collect once allocations since the last GC exceed
/// `max(min_threshold, 2 × live-after-last-GC)`.
pub struct MarkSweepCollector {
    min_threshold: usize,
    live_after_last: usize,
    stats: GcStats,
}

impl MarkSweepCollector {
    pub fn new() -> Self {
        Self::with_min_threshold(DEFAULT_MIN_THRESHOLD)
    }

    /// A custom floor; `1` gives the `--gc-stress` behavior (collect at every
    /// safe point).
    pub fn with_min_threshold(min_threshold: usize) -> Self {
        Self {
            min_threshold,
            live_after_last: 0,
            stats: GcStats::default(),
        }
    }
}

impl Default for MarkSweepCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector for MarkSweepCollector {
    fn should_collect(&self, heap: &Heap) -> bool {
        heap.allocs_since_gc() >= self.min_threshold.max(2 * self.live_after_last)
    }

    fn collect(&mut self, heap: &mut Heap, roots: &[&dyn GcRoots]) -> GcStats {
        let start = Instant::now();

        let mut visitor = GcVisitor::new(heap);
        visitor.trace_symbol_table();
        for provider in roots {
            provider.trace_roots(&mut visitor);
        }
        visitor.drain();
        let marks = visitor.finish();

        let marked = marks.marked();
        let swept = heap.sweep(&marks);
        heap.allocs_since_gc = 0;
        heap.gc_requested = false;

        self.live_after_last = marked.total();
        self.stats.collections += 1;
        self.stats.last_marked = marked;
        self.stats.last_swept = swept;
        self.stats.last_pause_micros = start.elapsed().as_micros();
        self.stats.clone()
    }

    fn stats(&self) -> &GcStats {
        &self.stats
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cps_expr::{CpsExpr, CpsExprKind};
    use std::cell::RefCell;

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
            binding_scope: None,
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
    fn alloc_counter_and_adaptive_threshold() {
        let mut heap = Heap::new();
        let mut collector = MarkSweepCollector::with_min_threshold(10);

        for i in 0..9 {
            heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL);
        }
        assert_eq!(heap.allocs_since_gc(), 9);
        assert!(!collector.should_collect(&heap));

        heap.alloc_pair(TaggedValue::fixnum(9), TaggedValue::NULL);
        assert!(collector.should_collect(&heap));

        let roots = TestRoots::default();
        collector.collect(&mut heap, &[&roots]);
        assert_eq!(heap.allocs_since_gc(), 0);
        assert!(!collector.should_collect(&heap));
    }

    #[test]
    fn gc_request_flag_roundtrip() {
        let mut heap = Heap::new();
        assert!(!heap.take_gc_request());
        heap.request_gc();
        assert!(heap.gc_requested());
        assert!(heap.take_gc_request());
        assert!(!heap.gc_requested());
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
}
