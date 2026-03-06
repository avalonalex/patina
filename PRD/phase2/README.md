# Phase 2: VM Backend

**Status:** Planning → Implementation
**Branch:** phase2-planning
**Goal:** Register-based bytecode VM implementing the `Backend` trait; 5–10× speedup over tree-walker.

---

## Architecture Decisions

All settled decisions are in **[VM_DECISIONS.md](./VM_DECISIONS.md)** — the master reference.
Key points:

- **Register machine** (not stack-based)
- **NaN-boxed `VmVal`** for VM-internal values; floats are immediate
- **SRFI-226** (Final 2023) for delimited continuations — 3 ISA primitives
- **Tracing GC** (`rust-gc`) for VM heap — Phase 2B
- **`patina-primitives` crate** extracted before any VM code
- **`CallFrame: Clone`** — non-negotiable invariant

---

## Core Planning Documents

These define what we are building in Phase 2.

| Document | Purpose |
|----------|---------|
| **[VM_DECISIONS.md](./VM_DECISIONS.md)** | ⭐ Master reference — all settled decisions |
| **[VM_SPECIFICATION.md](./VM_SPECIFICATION.md)** | Bytecode ISA, execution model, register file, prompt stack |
| **[COMPILATION_DESIGN.md](./COMPILATION_DESIGN.md)** | Compiler: `CoreExpr → bytecode`, register allocation, flat closures |
| **[VM_BACKEND_DESIGN.md](./VM_BACKEND_DESIGN.md)** | Code sharing with tree-walker, `patina-primitives` refactoring |
| **[VM_CALLCC_DESIGN.md](./VM_CALLCC_DESIGN.md)** | `call/cc` + SRFI-226 delimited continuations in detail |
| **[VM_TESTING_DESIGN.md](./VM_TESTING_DESIGN.md)** | Testing strategy: differential tests, benchmarks |
| **[ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md)** | Comparative analysis — Lua, V8, Chez, Guile (reference) |

---

## Deferred to Phase 2+

Good ideas, but not in scope for the initial VM. ISA has reserved space where noted.

| Document | Deferred Feature | ISA Hook |
|----------|-----------------|----------|
| **[01_META_TRACING.md](./01_META_TRACING.md)** | Meta-tracing JIT (Cranelift) | `profile_id` field on `Add`/`Call` |
| **[04_PERSISTENT_HEAP.md](./04_PERSISTENT_HEAP.md)** | Time-travel debugging, copy-on-write heap | `Snapshot` instruction (no-op) |
| **[06_SELF_OPTIMIZING_AST.md](./06_SELF_OPTIMIZING_AST.md)** | Truffle-style AST specialization | — |
| **[07_SYMBOLIC_EXECUTION.md](./07_SYMBOLIC_EXECUTION.md)** | Hybrid concrete/symbolic execution | — |
| **[FILE_SYSTEM_ABSTRACTION.md](./FILE_SYSTEM_ABSTRACTION.md)** | `FileSystem` trait for WASM/embedded | — |

---

## Research Background

These informed the decisions above. Read for context; not directly actionable.

| Document | What It Informed |
|----------|-----------------|
| **[02_EFFECT_CONTINUATIONS.md](./02_EFFECT_CONTINUATIONS.md)** | Led to SRFI-226 choice over ad-hoc effect types |
| **[03_ADAPTIVE_NUMERIC.md](./03_ADAPTIVE_NUMERIC.md)** | Confirmed NaN-boxing + JIT specialization approach |
| **[05_DELIMITED_CONTINUATIONS.md](./05_DELIMITED_CONTINUATIONS.md)** | Informed SRFI-226 ISA primitive design |
| **[TAGGED_POINTERS.md](./TAGGED_POINTERS.md)** | Low-bit tagging analysis; NaN-boxing supersedes for VM |
| **[STRING_ABSTRACTION_DESIGN.md](./STRING_ABSTRACTION_DESIGN.md)** | String representation options (open question) |
| **[R7RS_LARGE_STATUS.md](./R7RS_LARGE_STATUS.md)** | R7RS-large library tracking (not VM-specific) |

---

## Outdated / Superseded

Kept for historical reference. Do not use as a guide for implementation.

| Document | Why Superseded |
|----------|---------------|
| **[VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md)** | Assumed tree-walker uses `Value` enum; reality is `TaggedValue` is already universal. NaN-boxing decision supersedes its dual-representation design. |
| **[SEXPR_SEPARATION_ARCHITECTURE.md](./SEXPR_SEPARATION_ARCHITECTURE.md)** | Predates current `TaggedValue`-everywhere architecture. |
| **[DESUGARER_DESIGN.md](./DESUGARER_DESIGN.md)** | Proposes a `DesugarBackend` trait for VM-specific IR; current plan reuses `CoreExpr` directly without a VM-specific IR layer. |

---

## Implementation Phases

### Phase 2A — Foundation (get tests passing)

1. **Primitives refactoring first**: extract `patina-primitives` crate, `ApplyContext` trait
2. **`patina-vm` crate skeleton**: `VmVal`, `CallFrame`, `VmState`, `CodeObject`
3. **Compiler**: `CoreExpr → bytecode` (register allocator, flat closures, free-var analysis)
4. **Execution loop**: fetch-decode-execute, tail calls, basic continuation support
5. **SRFI-226 continuations**: `CallWithPrompt` / `AbortToPrompt` / `CaptureComposable`
6. **`Backend` trait impl**: `Interpreter<VmBackend>` passes all 1400 existing tests

### Phase 2B — Correctness + Performance

7. **Tracing GC**: integrate `rust-gc` for VM heap (fixes cycle leaks)
8. **NaN-boxing `VmVal`**: floats as immediates (replaces Phase 2A's simpler encoding)
9. **Specialized opcodes**: `FixnumAdd`, `Car`, `Cdr`, etc. — no dispatch overhead
10. **Benchmarks**: target 5–10× over tree-walker baseline (832ms fib(25) → <200ms)

### Phase 2C+ — Advanced (Deferred)

11. Meta-tracing JIT (`01_META_TRACING.md`)
12. Persistent heap / time-travel debugging (`04_PERSISTENT_HEAP.md`)
