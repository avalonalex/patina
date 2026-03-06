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

## 2. Value Representation: VM-Internal NaN-Boxing (`VmVal`)

**Decision:** The VM uses its own 64-bit NaN-boxed value type `VmVal`, distinct from
the tree-walker's `TaggedValue`. Valid IEEE 754 floats are immediate (no heap
allocation). All other types are encoded in the quiet-NaN space.

```
VmVal bit layout:
  - Valid float (non-NaN): stored as-is (64-bit IEEE 754)
  - NaN-tagged value: high 13 bits = 0x1FFF (quiet NaN prefix)
      bits[47:45] = type tag (up to 8 types in tagged space)
      bits[44:0]  = payload (fixnum, char, bool, heap index, etc.)
```

**Conversion boundary:** The `Backend` trait API uses the shared `TaggedValue`. The
VM compiler converts `TaggedValue` constants → `VmVal` when building bytecode. The
result is converted back at `Backend::eval` return time. This keeps the tree-walker's
`TaggedValue` unchanged.

**Why:** Floats are immediate — no allocation on `(+ 1.0 2.0)`. Profiling shows numeric
benchmarks are heavily float-intensive. This is the approach used by LuaJIT and V8.

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

## Open Questions (Not Yet Decided)

1. **Symbol representation in `VmVal`**: share interning with tree-walker's
   `SharedHeap` symbol table, or maintain a separate VM-local symbol table?
   Shared is simpler; separate is cleaner. Defer decision to implementation.

2. **String representation**: keep `Vec<char>` (R7RS O(1) `string-ref`) or switch
   to UTF-8 + sparse index cache in VM? `STRING_ABSTRACTION_DESIGN.md` has analysis.
   Defer to Phase 2B (after basic VM works).
