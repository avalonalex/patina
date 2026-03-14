# Phase 2 VM: Settled Architecture Decisions

**Status:** Authoritative Reference — do not contradict without updating this file
**Branch:** phase2-planning
**Last updated:** 2026-03-05

This document records every architecture decision that has been made for the Phase 2 VM
backend. It is the single source of truth. All other Phase 2 docs should be consistent
with it.

---

## 1. Machine Architecture: Register Machine

**Decision:** Register-based VM (not stack-based).

Each `CallFrame` owns a register window: a contiguous slice of a shared `Vec<VmVal>`.
Local variables and temporaries are accessed by index (O(1), no hash lookup).
Only global variables use name lookup.

**Why:** Register machines are easier to optimize (SSA, inlining, type specialization)
and are the foundation for the meta-tracing JIT (reserved for Phase 2+). Lua 5, V8,
and Chez Scheme all use register/slot machines.

---

## 2. Value Representation: Unified `TaggedValue` (Phase 2A)

**Decision:** The VM uses `TaggedValue` as its value type — the same type as the
tree-walker. No separate `VmVal`, no conversion overhead, no boundary cost on
primitive calls.

**Why:** Chez Scheme (the primary reference implementation) uses a single `ptr` type
everywhere — both its bytecode VM and native-code compiler manipulate the same 64-bit
tagged representation. Floats are heap-allocated in Chez too. This is the right
baseline: one type, shared heap, no conversion.

The original `VmVal` NaN-boxing idea (LuaJIT/V8 style) trades 61-bit fixnums for
45-bit fixnums in exchange for immediate floats. That regression is not worth it for
Phase 2A, and the benefit (float arithmetic without heap allocation) only matters
inside the VM's hot register loop — which still hits the heap on every
`patina-primitives` call anyway.

**Deferred optimization (Phase 2B+):** If profiling shows float allocation is a
measurable bottleneck in the VM execution loop (after benchmarks exist), introduce a
`VmVal` type with NaN-boxing for the register file only, with conversion at primitive
call sites. Do not do this speculatively.

---

## 3. Closures: Flat Representation

**Decision:** VM closures use a flat `free_vars: Vec<VmVal>` array, not an
`Rc<Environment>` chain.

```rust
pub struct VmClosure {
    code: CodeObjectId,
    free_vars: Vec<VmVal>,   // captured values by index, O(1) access
}
```

Free variable analysis runs at compile time. Variables are resolved to
`LoadLocal { index }`, `LoadClosure { slot }`, or `LoadGlobal { name }` at
bytecode emission. No runtime hash lookup for local/closure variables.

**Mutable captured variables** (those `set!` after capture) are wrapped in a
heap-allocated `MutableCell` at the binding site. Only the cell pointer is captured.

---

## 4. Continuations: Stack-Snapshot + SRFI-226

**Decision:**
- Continuations are captured by snapshotting call frames (stack-snapshot model).
- The delimited continuation API follows **SRFI-226** (Final 2023).
- `CallFrame` must `#[derive(Clone)]` — enforced from day 1.

### Three Core ISA Instructions

```
CallWithPrompt  body_reg, prompt_tag, handler_reg, dst
AbortToPrompt   prompt_tag, value_regs…
CaptureComposable dst, prompt_tag
```

Everything else is derived:

| Scheme form | Built from |
|-------------|------------|
| `call/cc` | `CaptureComposable` at default prompt + non-composable invoke (replaces stack) |
| `call-with-composable-continuation` | `CaptureComposable` + composable invoke (appends frames) |
| `shift` / `reset` | Standard encoding via composable continuations + `AbortToPrompt` |
| `with-exception-handler` | `CallWithPrompt` with the exception prompt tag |
| `raise` | `AbortToPrompt` with exception tag |
| generators / coroutines | `CaptureComposable` with a generator-specific prompt tag |
| `dynamic-wind` | hooks on `CallWithPrompt` entry/exit |

The prompt-tag system lets new control abstractions (async/await, etc.) be added
by defining a new tag — no ISA changes required.

### `CallFrame` invariant (non-negotiable)

```rust
#[derive(Clone)]   // ← must never be removed
pub struct CallFrame {
    pub code:          CodeObjectId,
    pub pc:            usize,
    pub register_base: usize,        // offset into VM's shared register array
    pub env:           Option<Rc<VmClosure>>,  // for closure slot access
    pub arg_count:     usize,
}
```

A non-composable continuation snapshots `vm.frames.clone()` entirely.
A composable (delimited) continuation snapshots `vm.frames[prompt_depth..]`.
Composable invocation appends those frames to the current stack.

---

## 5. Garbage Collection

**Decision (tree-walker):** Deferred. Rc-based; cycles leak. Acceptable for a
reference implementation. Not changing in Phase 2.

**Decision (VM):** Proper tracing GC. The VM heap uses mark-and-sweep via the
`rust-gc` crate (derive-macro based). Handles cycles from `set-car!`/`set-cdr!`.

**Phasing:**
- **Phase 2A (initial VM):** Use `Rc`/arena allocation while getting tests green.
  Known limitation: cycles leak. Acceptable for bootstrap.
- **Phase 2B:** Integrate `rust-gc` for the VM heap. All `VmPair`, `VmVector`,
  `VmClosure` etc. become `Gc<T>`. Mark-sweep collects cycles.
- **Phase 3+ (optional):** Generational GC if profiling shows GC pressure.

---

## 6. Primitives: `patina-primitives` Crate First

**Decision:** Before writing any VM execution code, extract primitives to a shared
crate.

**New signature (backend-agnostic):**
```rust
// patina-primitives
pub type TaggedHandler = fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;
```

**Split:**
- ~290 heap-only primitives → `patina-primitives` (shared by tree-walker and VM)
- ~6 higher-order primitives (need `apply`) → `ApplyContext` trait, each backend provides its own

The tree-walker's `PrimitiveRegistry` uses `patina-primitives` after refactoring.
The VM registers the same primitives but dispatches through its own `CallWithPrompt`-aware apply.

---

## 7. Profiling: Reserved Space, Not Implemented Yet

**Decision:** Arithmetic and call instructions carry a `profile_id` field in the ISA
(zero-cost in initial interpreter — stored but never read). This keeps the door open
for the meta-tracing JIT without a future ISA break.

```rust
Add { dst, src1, src2, profile_id: ProfileId }  // profile_id is a u32; ignored initially
Call { func, args, dst, profile_id: ProfileId }
```

The meta-tracing JIT (ISA doc: `01_META_TRACING.md`) is **Phase 2+**, not Phase 2.

---

## 8. Tail Calls

**Decision:** Explicit `TailCall` instruction in ISA. Compiler detects tail positions
in `CoreExpr` and emits `TailCall` instead of `Call`. `TailCall` reuses the current
`CallFrame` (in-place PC/register-base update) rather than pushing a new frame.

---

## 9. Code Sharing with Tree-Walker

**Decision:** ~80% of the codebase is reused. The VM receives `CoreExpr` from the
same frontend pipeline. It compiles `CoreExpr → bytecode` directly (no CPS transform).

| Component | Shared? | Notes |
|-----------|---------|-------|
| `TaggedValue` / `SharedHeap` | ✓ (at boundary) | VM uses `VmVal` internally |
| `CoreExpr` IR | ✓ | VM compilation source |
| Frontend (lexer/parser/desugarer) | ✓ | Unchanged |
| Library system | ✓ | Backend-agnostic |
| `Backend` trait | ✓ | VM implements same trait |
| `patina-primitives` (~290 prims) | ✓ | After Phase 2A refactoring |
| CPS transform / `CpsExpr` | ✗ | VM compiles direct-style |

---

## 10. Value Type: `TaggedValue` Throughout (Settled)

**Decision:** The VM uses `TaggedValue` as its internal value type throughout — the
same type as the tree-walker and all primitives. No separate `VmVal`, no conversion.
See §2 for full rationale.

---

## 11. String Representation: Fixed-Width Character Array (Settled)

**Decision:** Strings are stored as `Vec<char>` (fixed-width 32-bit Unicode codepoints),
same as the current tree-walker heap. This gives O(1) `string-ref`/`string-set!` as
required by R7RS without an index cache.

The external interface is UTF-8 (I/O, display, `symbol->string`, etc.). Internal
storage is fixed-width for correctness and simplicity.

**Deferred optimizations** (Phase 2B+):
- Small-string optimization (inline short strings in `VmVal` payload)
- Latin-1 / compact ASCII representation for ASCII-only strings
- UTF-8 + sparse index cache for memory-heavy workloads

`STRING_ABSTRACTION_DESIGN.md` has analysis of the trade-offs; revisit after basic VM works.

---

## Open Questions (Not Yet Decided)

1. **Symbol representation in `VmVal`**: share interning with tree-walker's
   `SharedHeap` symbol table, or maintain a separate VM-local symbol table?
   Shared is simpler; separate is cleaner. Defer decision to implementation.
