# Phase 2 VM: Settled Architecture Decisions

**Status:** Authoritative Reference — all decisions implemented and verified
**Last updated:** 2026-03-14

This document records every architecture decision for the Phase 2 VM backend.
It is the single source of truth. All other VM docs should be consistent with it.

---

## 1. Machine Architecture: Register Machine

**Decision:** Register-based VM (not stack-based).

Each `CallFrame` owns a register window: a contiguous slice of a shared
`Vec<TaggedValue>`. Local variables and temporaries are accessed by index (O(1),
no hash lookup). Only global variables use name lookup.

**Why:** Register machines are easier to optimize (SSA, inlining, type
specialization). Lua 5, V8, and Chez Scheme all use register/slot machines.

---

## 2. Value Representation: Unified `TaggedValue`

**Decision:** The VM uses `TaggedValue` as its value type — the same type as the
tree-walker. No separate `VmVal`, no conversion overhead, no boundary cost on
primitive calls.

**Why:** Chez Scheme uses a single `ptr` type everywhere. One type, shared heap,
no conversion is the right baseline.

---

## 3. Closures: Flat Representation

**Decision:** VM closures use a flat `free_vars: Vec<TaggedValue>` array.

```rust
// Stored on heap as HeapObjectData::VmClosure
HeapObjectData::VmClosure {
    code_id: u32,                    // CodeObjectId
    free_vars: Vec<TaggedValue>,     // captured values by index, O(1) access
}
```

Free variable analysis runs at compile time. Variables are resolved to
`Move` (local), `LoadClosure { slot }`, or `LoadGlobal { name }` at bytecode
emission. No runtime hash lookup for local/closure variables.

**Global variables** are never captured into closure `free_vars` — they are
always looked up dynamically via `LoadGlobal`.

**Mutable captured variables** (those `set!` after capture, or internal defines
captured by nested lambdas) are wrapped in a heap-allocated `MutableCell` at
the binding site. Only the cell pointer is captured.

---

## 4. Continuations: Stack-Snapshot + Control Primitives

**Decision:**
- Continuations are captured by snapshotting call frames, registers, and
  dynamic state.
- `CallFrame` must `#[derive(Clone)]` — enforced from day 1.
- Continuation operations are **VM control primitives** intercepted at call
  dispatch, not ISA instructions.

### `CallFrame` (non-negotiable invariant)

```rust
#[derive(Clone)]
pub struct CallFrame {
    pub code_id:       CodeObjectId,
    pub pc:            usize,
    pub register_base: usize,
    pub num_regs:      u16,
    pub closure:       Option<HeapIndex>,
    pub return_reg:    Reg,
}
```

### Control Primitives

| `VmControlPrimitive` | Scheme form |
|---|---|
| `CallWithCurrentContinuation` | `call/cc` — snapshot full stack |
| `CallWithContinuationPrompt` | `call-with-continuation-prompt` — push prompt |
| `AbortCurrentContinuation` | `abort-current-continuation` — unwind to prompt |
| `DynamicWind` | `dynamic-wind` — before/body/after |
| `Values` / `CallWithValues` | `values` / `call-with-values` |
| `WithExceptionHandler` | `with-exception-handler` |
| `Raise` / `RaiseContinuable` / `Error` | `raise` / `raise-continuable` / `error` |

### Continuation Storage

Continuations use **side tables** on VmState (`continuation_store`,
`delimited_continuation_store`) with opaque `VmContinuationRef(u64)` handles
stored as `HeapObjectData` variants. This avoids circular dependencies between
`patina-core` and `patina-vm`.

---

## 5. Garbage Collection

**Settled (2026-08-01):** Non-moving stop-the-world mark-and-sweep over the
typed arenas, shared by both backends — cycles from `set-car!`/`set-cdr!` and
from closure ↔ environment references are reclaimed. `Rc` still owns
individual heap payloads; the collector breaks cycles by tombstoning dead
slots at sweep, which drops those `Rc`s.

The VM roots `VmState` (`runtime/gc_roots.rs`), including two members no heap
scan can reach: the continuation side tables (the heap holds only an opaque
`VmContinuationRef(u64)`) and `CallFrame::closure` (a bare `HeapIndex`).

**Off by default** until stage 4 — see `docs/GC_DESIGN.md` for the design,
root inventory, safe-point protocol, and staging.

---

## 6. Primitives: Shared `patina-primitives` Crate

**Decision:** ~290 heap-only primitives in `patina-primitives` (shared by
tree-walker and VM). ~6 higher-order primitives use `ApplyContext` trait, each
backend provides its own.

```rust
pub type TaggedHandler = fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;
```

---

## 7. Tail Calls

**Decision:** Explicit `TailCall` instruction in ISA. Compiler detects tail
positions and emits `TailCall` instead of `Call`. `TailCall` reuses the current
`CallFrame`.

**TailCall arg handling:** Arguments are read into temporaries, then the VM
collects all values first before writing to callee base. This avoids clobbering
when `func.dst` falls in the param slot range.

---

## 8. Code Sharing with Tree-Walker

~80% of the codebase is reused. The VM receives `CoreExpr` from the same
frontend pipeline and compiles direct-style (no CPS transform).

| Component | Shared? | Notes |
|-----------|---------|-------|
| `TaggedValue` / `SharedHeap` | Yes | Same type throughout |
| `CoreExpr` IR | Yes | VM compilation source |
| Frontend (lexer/parser/desugarer) | Yes | Unchanged |
| Library system | Yes | Backend-agnostic |
| `Backend` trait | Yes | VM implements same trait |
| `patina-primitives` (~290 prims) | Yes | Shared crate |
| CPS transform / `CpsExpr` | No | VM compiles direct-style |

---

## 9. String Representation: Fixed-Width Character Array

**Decision:** Strings are stored as `Vec<char>` (32-bit Unicode codepoints).
O(1) `string-ref`/`string-set!` as required by R7RS. External interface is UTF-8.

---

## 10. Compiler Pipeline

**Decision:** 2 pre-passes + 5 compilation passes, all pure stateless
transformations. See [VM_COMPILER.md](./VM_COMPILER.md).

```
CoreExpr → Quasiquote Expansion → Alpha Rename →
  Analysis → Closure Conversion → Tail Marking →
  Register Allocation → Code Generation → CodeObject
```

The alpha-rename pre-pass bridges macro hygiene (scope-set resolution) from the
tree-walker's runtime approach to compile-time resolution.
