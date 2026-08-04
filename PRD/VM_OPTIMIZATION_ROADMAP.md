# VM Optimization Roadmap

**Created:** 2026-03-22
**Status:** Planning
**Baseline:** Register-based bytecode VM, 1163/1163 R7RS tests, ~4.2x faster than tree-walker

This document catalogs the gaps between Patina's first-generation VM and state-of-the-art Scheme/Lisp VMs (Chez, Guile 3, Racket, LuaJIT). Each area is independent and can be picked up as a standalone work item.

---

## 1. Specialized Primitive Opcodes

**Current:** Every primitive (`+`, `car`, `cons`, `null?`, ...) goes through the generic `Call` path: heap borrow, string-keyed registry lookup, frame push/pop.

**Target:** Dedicated instructions for the ~20 hottest primitives. Guile has ~50. Eliminates call overhead entirely for trivial operations.

**Candidates:** `Add`, `Sub`, `Mul`, `Lt`, `Eq`, `Car`, `Cdr`, `Cons`, `NullP`, `PairP`, `VectorRef`, `VectorSet`, `Not`.

**Estimated impact:** 2-5x on arithmetic/list-heavy code.

**Note:** `CallPrimitive` instruction is already defined in the ISA but unwired at runtime.

---

## 2. Flat Instruction Encoding

**Current:** Instructions are Rust enum variants in `Vec<Instruction>`. Each dispatch clones the enum.

**Target:** Flat `Vec<u32>` with packed opcode+operands (shift+mask decode). Improves cache locality, eliminates clone, and enables bytecode serialization.

**Reference:** Guile uses 32-bit words; Lua uses 32-bit packed instructions.

**Estimated impact:** ~30% dispatch speedup.

---

## 3. Garbage Collection

**Status (2026-08-01):** Done for correctness — non-moving mark-and-sweep over the typed arenas, on **both** backends, reclaiming cycles that `Rc` cannot (PRs #4-#6). Currently **off by default**; `(gc)` collects on request, `PATINA_GC`/`PATINA_GC_STRESS` enable it. Design and staging: `docs/GC_DESIGN.md`.

**Update (2026-08-03, PR #8):** the safe-point trigger redesign landed — the collection decision moved to alloc time and the safe point is a single flag load. GC-off is at parity with pre-GC `main` and the GC-on standing penalty (was 13.7%) is gone; see `docs/GC_DESIGN.md` §6.1 for the re-measurements.

**Remaining (stage 4b):** flip to default-on (now affordable: enabling GC costs only actual pauses), two CI lanes, SourceMap pruning hook, stress proofs.

**Target (later):** Generational collection. Young generation for short-lived allocations.

**Reference:** Chez uses generational copying GC; Guile uses mark-sweep with generational hints.

**Estimated impact:** Correctness for long-running programs. Required before production use.

**Existing design doc:** `PRD/phase1/GC_DESIGN.md`

---

## 4. Compiler Optimizations

**Current:** Zero optimization passes between analysis and codegen.

**Target (incremental):**

| Optimization | Description | Complexity |
|-------------|-------------|------------|
| Constant folding | Fold `(+ 1 2)` at compile time | Low |
| Dead code elimination | Remove unused bindings | Low |
| Closure lifting | Promote zero-free-var closures to top-level | Medium |
| Contification | Known-call-only closures become labels (jump, not call) | Medium |
| Peephole | `LoadConst + Add` -> `AddImmediate`, dead `Move` removal | Low-Medium |
| Copy propagation | Eliminate redundant `Move` chains | Medium |

**Reference:** Guile 3 has CPS-based IR with CSE, DCE, type inference, closure lifting. Chez has full CPS optimizer with contification.

---

## 5. Register Allocation

**Current:** Monotonic watermark allocator. No liveness analysis, no coalescing. High register pressure, many redundant `Move`s.

**Target:** Linear-scan with liveness intervals. Proper interference tracking and register coalescing.

**Reference:** Guile uses slot allocation with liveness; LuaJIT uses linear-scan for its IR.

**Estimated impact:** Reduced register footprint, fewer Move instructions, better cache behavior.

---

## 6. Threaded Dispatch

**Current:** Rust `match` (switch dispatch). Extra indirect branch prediction miss per instruction.

**Target:** Direct/indirect threaded dispatch. Each instruction handler jumps directly to the next, avoiding the central dispatch switch.

**Implementation options in Rust:** `unsafe` computed-goto patterns, macro-generated state machines, or a `#[musttail]` approach if stabilized.

**Estimated impact:** ~20-30% interpreter speedup.

**Note:** Only worthwhile after flat instruction encoding (area 2) is done.

---

## 7. Value Representation

**Current:** 64-bit low-3-bit tagged pointers. Floats are heap-allocated as `HeapObjectData::Real(f64)`.

**Target:** True NaN-boxing. IEEE 754 NaN payload encodes pointers, fixnums, booleans, chars. Floats stay inline without heap allocation.

**Estimated impact:** Significant for float-heavy code; marginal for typical Scheme (fixnum-dominant). Lower priority.

---

## 8. Bytecode Serialization

**Current:** Code compiled fresh every run. No caching.

**Target:** Serialize `CodeObject`s to disk. Load pre-compiled libraries on startup.

**Reference:** Guile `.go` files, Racket `.zo` files, Chez `.so` object files.

**Estimated impact:** Startup time improvement. Prerequisite: stable instruction encoding (area 2).

---

## 9. Continuation Optimization

**Current:** Full stack snapshot on every `call/cc` — clones entire `Vec<CallFrame>` and register array.

**Target:** Segmented stacks or stack slicing. Only copy the delimited portion between prompt and capture point.

**Reference:** Chez uses segmented stacks with copy-on-demand. Guile uses prompt-based stack slicing.

**Estimated impact:** Only matters for call/cc-heavy code (coroutines, backtracking, web continuations).

---

## 10. JIT Compilation

**Current:** Interpreter only.

**Target:** Baseline JIT compiling hot bytecode to native code. Potentially tiered: interpreter -> baseline JIT -> optimizing JIT.

**Reference:** LuaJIT (tracing JIT), Chez (AOT native), Racket CS (via Chez), V8/SpiderMonkey (tiered).

**Estimated impact:** 10-100x for hot loops. Enormous engineering effort. Long-term goal.

---

## Suggested Priority Order

| Priority | Area | Rationale |
|----------|------|-----------|
| P1 | Specialized primitive opcodes | Biggest single perf win, moderate effort |
| P2 | Flat instruction encoding | Enables serialization, improves dispatch |
| P3 | Garbage collection | Correctness requirement |
| P4 | Constant folding + DCE | Easy wins in the compiler |
| P5 | Closure lifting / contification | Eliminates unnecessary heap allocation |
| P6 | Threaded dispatch | Requires flat encoding first |
| P7 | Register allocation | Moderate win, moderate effort |
| P8 | Bytecode serialization | Requires stable encoding first |
| P9 | Continuation optimization | Niche workloads |
| P10 | JIT | Long-term, high effort |
