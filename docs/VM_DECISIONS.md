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
    code_id: u64,                    // CodeObjectId
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
    pub closure:       TaggedValue,     // the VmClosure, or CallFrame::NO_CLOSURE
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
`delimited_continuation_store`) with opaque `VmContinuationRef { id, bytes }`
handles stored as `HeapObjectData` variants. This avoids circular dependencies
between `patina-core` and `patina-vm`. `bytes` is the snapshot's size, charged
to the handle for the collection trigger's byte account (#606).

---

## 5. Garbage Collection

**Settled (2026-08-01):** Non-moving stop-the-world mark-and-sweep over the
typed arenas, shared by both backends — cycles from `set-car!`/`set-cdr!` and
from closure ↔ environment references are reclaimed. `Rc` still owns
individual heap payloads; the collector breaks cycles by tombstoning dead
slots at sweep, which drops those `Rc`s.

The VM roots `VmState` (`runtime/vm_state/gc_roots.rs`), including the one member no heap
scan can reach: the continuation side tables (the heap holds only an opaque
`VmContinuationRef { id, bytes }`). A frame's closure is a value, traced like the
frame's registers; until #682 it was a bare object index, with a visitor method of its own.

**On by default** since stage 4c (`PATINA_GC=0` opts out), at zero standing
cost since stage 4a's trigger redesign (the decision is made in `alloc_*`;
a safe point reads one word, the `Mutator`'s poll word since #699, and only
at frame entries and transfer returns since #700) — see `docs/GC_DESIGN.md`
for the design, root inventory, safe-point protocol, and staging.

---

## 6. Primitives: Shared `patina-primitives` Crate

**Decision:** ~290 heap-only primitives in `patina-primitives` (shared by
tree-walker and VM). ~6 higher-order primitives use `ApplyContext` trait, each
backend provides its own.

**The signature is moving to the context** (#706; `PRD/GC_PRD.md` §11.3's
`Prim`), one category per PR. The registry accepts both kinds while the files
convert, and the old one goes with the last file:

```rust
// A primitive that takes the context: reads and allocates through the `Cx`,
// and its arguments and value are values of it.
pub type CxHandler =
    for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>;
// The kind it replaces.
pub type TaggedHandler = fn(&SharedHeap, &[TaggedValue]) -> Result<TaggedValue, EvalError>;
```

- **The machine makes the context.** It does so where it calls the
  primitive, between its safe points, through `ApplyContext::call_cx`. The VM
  and the tree-walker hand the argument words over as values with no copy
  (`patina_core::heap::cx::enter_call`, `unsafe`, for the trusted crates), and
  lend the context the heap's instance, which each keeps from construction. A
  primitive that takes the context cannot call back, so it reaches no safe
  point, and nothing collects while it runs.
- **The VM calls one directly** at a `CallPrimitive` site
  (`PrimitiveRegistry::cx_handler`), with no `ApplyContext` and no `Step`
  between, since it can neither call back nor suspend. A heap handler's call
  pays about 10 instructions for the check until its category converts.
- **Without a machine**, an embedder's own `ApplyContext`, `call_cx`'s
  default crosses each word through a handle.
- **Converted so far:** the type predicates (`predicates.rs`), the
  characters (`characters.rs`), the strings (`strings.rs`) and the lists
  (`lists.rs`), but for `member` and `assoc`, which call a comparator and
  convert with the higher-order primitives.

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
| `patina-primitives` (~290 prims) | Yes | Shared crate; each machine makes the context for a primitive that takes one (§6) |
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
