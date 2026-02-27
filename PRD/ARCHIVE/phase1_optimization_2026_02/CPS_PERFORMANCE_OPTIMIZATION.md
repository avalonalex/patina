# CPS Evaluator Performance Optimization Plan

**Status**: Ready to implement
**Date**: 2026-02-27
**Related**: `PROFILING_BENCHMARK_PLAN.md`, `CLONE_OPTIMIZATION_ANALYSIS.md`

---

## Profiling Summary

Profiled using macOS `sample` tool on release builds with debug symbols (`cargo build --release`). Two workloads:

- **fib(25)**: 2316 samples, ~291ms mean (Criterion)
- **tak(18,12,6)**: Recursive arithmetic stress test

Both workloads show the same bottleneck pattern: **~48% of CPU time spent in HashMap lifecycle operations** for the CPS continuation environment.

---

## Optimization 1: cont_env Clone/Drop Cycle (P0 — ~48% CPU)

### Problem

The CPS evaluator stores local continuation bindings in `HashMap<Rc<str>, ContValue>`. Every CPS step clones this map, and the old one is dropped. This creates a clone→use→drop→malloc→free cycle that dominates runtime.

### Profiling Evidence (fib25, 2316 samples)

| Function | Samples | % | Category |
|----------|---------|---|----------|
| `hashbrown::raw::RawTable<T>::clone_from_impl` | ~150 | 6.5% | Clone |
| `clone_from_impl` + related HashMap clone | ~145 | 6.2% | Clone |
| `RawTableInner::drop_inner_table` | ~130 | 5.6% | Drop |
| `drop_in_place<ContValue>` + related | ~140 | 6.1% | Drop |
| `malloc` (HashMap bucket allocation) | ~280 | 12.1% | Alloc |
| `free` (HashMap bucket deallocation) | ~275 | 11.9% | Dealloc |
| **Total** | **~1120** | **~48%** | |

### Root Cause

In `crates/patina-tree-walker/src/eval/cps_eval/step.rs`, each CPS step creates a new continuation that clones the environment:

```rust
// Pseudocode of the hot path
let new_env = current_env.clone(); // Clones entire HashMap
// ... use new_env for one step ...
// old env dropped → free all buckets
```

For fib(25), there are ~240K+ CPS steps, each cloning and dropping a HashMap.

### Proposed Solutions (Pick One)

#### Option A: `Rc<HashMap>` with Copy-on-Write (Recommended — Low Risk)

Wrap cont_env in `Rc<HashMap<Rc<str>, ContValue>>`. Most continuations share the same environment unchanged:

- **Read path**: `Rc::clone()` is a pointer-width increment (near zero cost)
- **Write path**: `Rc::make_mut()` only clones when refcount > 1
- **Expected savings**: ~40-45% of total CPU (most steps don't modify env)
- **Risk**: Low — semantics unchanged, just deferred cloning

#### Option B: Persistent HashMap (`im` crate)

Replace `HashMap` with `im::HashMap` (HAMT — Hash Array Mapped Trie):

- **Clone**: O(1) — structural sharing
- **Lookup**: O(log32 n) — slightly slower than HashMap's O(1)
- **Insert**: O(log32 n) — creates new path, shares rest
- **Expected savings**: ~45%+ of total CPU
- **Risk**: Medium — new dependency, slightly different perf characteristics
- **Caveat**: `im` uses `Arc` internally; may need `im-rc` for single-threaded use

#### Option C: Small-Vec Flat Array

If cont_env typically has few entries (< 8), a `SmallVec<[(Rc<str>, ContValue); 8]>` with linear scan:

- **Clone**: Memcpy of small inline buffer
- **Lookup**: O(n) but cache-friendly for small n
- **Expected savings**: ~35-40% if env is typically small
- **Risk**: Low, but need to verify typical env sizes first

### Implementation Steps (Option A)

1. Change `cont_env` type from `HashMap<Rc<str>, ContValue>` to `Rc<HashMap<Rc<str>, ContValue>>`
2. Replace `.clone()` with `Rc::clone()`
3. Replace mutation sites with `Rc::make_mut(&mut env).insert(k, v)`
4. Benchmark: expect 30-45% wall-clock improvement on fib/tak

---

## Optimization 2: Primitive Name Lookup (P1 — ~6-8% CPU)

### Problem

`PrimitiveRegistry::apply_tagged()` constructs a qualified name string (`"scheme.base/+"`) via `format!()` on every primitive call, then does a HashMap lookup with this string.

### Profiling Evidence

| Function | Samples | % |
|----------|---------|---|
| `PrimitiveRegistry::apply_tagged` | ~150 | 6.5% |
| `memchr_aligned` (string ops in lookup) | ~188 | 8.1% |

The `memchr_aligned` is from string search operations in the HashMap lookup path (hashing + comparison of the formatted string key).

### Proposed Fix: Pre-compute Qualified Name

Store the qualified name in `Procedure::Primitive` at registration time:

```rust
// Before (every call)
let qualified = format!("{}/{}", library, name);
let entry = self.primitives.get(&qualified);

// After (once at registration)
struct PrimitiveEntry {
    qualified_name: Rc<str>,  // Pre-computed "scheme.base/+"
    func: PrimitiveFn,
    arity: Arity,
}
```

Then lookup by a pre-stored index or direct function pointer instead of string lookup.

### Implementation Steps

1. Add `qualified_name: Rc<str>` (or `usize` index) to the primitive storage in `Procedure::Primitive`
2. Compute the qualified name once during `register()` in `PrimitiveRegistry`
3. In `apply_tagged()`, use the stored name/index directly instead of `format!()`
4. Consider storing the `PrimitiveFn` pointer directly in the procedure value to skip the registry lookup entirely
5. Benchmark: expect 5-8% improvement

### Alternative: Direct Function Pointer

Even better than caching the name: store a direct function pointer or registry index in the TaggedValue/HeapObject for primitives. This would eliminate the HashMap lookup entirely:

```rust
// Current: TaggedValue → name → format!() → HashMap lookup → PrimitiveFn → call
// Proposed: TaggedValue → PrimitiveFn → call
```

This requires storing the function pointer in `HeapObjectData::Procedure` at creation time.

---

## Optimization 3: Faster Hasher for Internal Maps (P2 — ~2% CPU)

### Problem

Rust's default `HashMap` uses SipHash, which is designed for DoS resistance. For interpreter-internal maps (cont_env, environments, registries), this is unnecessary overhead.

### Proposed Fix

Switch to `FxHashMap` (from `rustc-hash` crate) or `AHashMap` (from `ahash` crate) for internal maps:

```rust
use rustc_hash::FxHashMap;
type ContEnv = FxHashMap<Rc<str>, ContValue>;
```

- **FxHash**: Used by rustc, very fast for small keys, no collision resistance
- **AHash**: Good balance of speed and quality, hardware-accelerated on modern CPUs

### Implementation Steps

1. Add `rustc-hash` (or `ahash`) dependency
2. Replace `HashMap` with `FxHashMap` in:
   - `cont_env` in CPS evaluator
   - `Environment::bindings`
   - `PrimitiveRegistry::primitives`
   - `SpecialFormRegistry` (if applicable)
3. Do NOT change maps exposed to untrusted input (none currently)
4. Benchmark: expect 1-3% improvement

---

## Expected Combined Impact

| Optimization | Estimated Savings | Effort | Priority |
|-------------|-------------------|--------|----------|
| cont_env Rc<HashMap> CoW | 30-45% | Medium (2-3 hours) | P0 |
| Pre-computed primitive name | 5-8% | Low (1 hour) | P1 |
| FxHash for internal maps | 1-3% | Low (30 min) | P2 |
| **Combined** | **~40-50%** | | |

On fib(25) baseline of ~291ms, a 40-50% improvement would bring it to ~145-175ms.

---

## Verification Plan

1. After each optimization:
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test --all --lib --tests`
   - `cargo build --release && ./scripts/run_chibi_tests.sh`

2. Benchmark after each:
   - `cargo bench --package patina-tests -- --quick` (Criterion)
   - Compare against pre-optimization baseline

3. Profile after all optimizations:
   - Re-run macOS `sample` on fib(25) and tak(18,12,6)
   - Verify HashMap operations are no longer dominant
   - Identify next bottleneck tier

---

## Notes

- These optimizations are independent and can be applied in any order
- Option A (Rc<HashMap> CoW) for Optimization 1 is recommended as the first step due to low risk and high impact
- If Option A shows < 20% improvement, investigate Option B (persistent HashMap) next
- The profiling was done on the tree-walker backend; Phase 2 VM backend will have entirely different characteristics
