# Phase 2: VM Backend

**Status:** Planning complete → Implementation ready
**Goal:** Register-based bytecode VM implementing the `Backend` trait; 5–10× speedup over tree-walker.

---

## Architecture Decisions

All settled decisions are in **[VM_DECISIONS.md](./VM_DECISIONS.md)** — the master reference.
Key points:

- **Register machine** (not stack-based)
- **`TaggedValue` throughout** — same type as tree-walker, no conversion overhead
- **SRFI-226** (Final 2023) for delimited continuations — 3 ISA primitives
- **`CallFrame: Clone`** — non-negotiable invariant for stack-snapshot continuations
- **`patina-primitives` crate** extracted ✅ done (`b24c8ad`)
- **Flat closures** — `free_vars: Vec<TaggedValue>`, O(1) access
- **Callee-side variadic collection** — rest args built by callee prologue
- **Multi-value side buffer** — `vm.value_buffer`, no heap allocation for `values`
- **Prompt tags are opaque heap objects** — `HeapObjectData::PromptTag { id: u64 }`

---

## Core Documents

These three docs define what we are building. Start here.

| Document | Purpose |
|----------|---------|
| **[VM_DECISIONS.md](./VM_DECISIONS.md)** | ⭐ Master reference — all settled decisions |
| **[VM_ISA.md](./VM_ISA.md)** | Instruction set, semantics, tail calls, continuations, GC roots |
| **[VM_COMPILER.md](./VM_COMPILER.md)** | 5-pass compiler: `CoreExpr → CodeObject` |
| **[VM_RUNTIME.md](./VM_RUNTIME.md)** | `VmState`, execution loop, continuation machinery |
| **[VM_TESTING.md](./VM_TESTING.md)** | Testing discipline: layers, reuse strategy, development order |

---

## Forward-Looking

Not in scope for Phase 2A but tracked here.

| Document | Feature | Phase |
|----------|---------|-------|
| **[SYNTAX_CASE_DESIGN.md](./SYNTAX_CASE_DESIGN.md)** | `syntax-case` procedural macros | Phase 3 |
| **[R7RS_LARGE_STATUS.md](./R7RS_LARGE_STATUS.md)** | R7RS-large library tracking | Ongoing |
| **[reference/01_META_TRACING.md](./reference/01_META_TRACING.md)** | Meta-tracing JIT (Cranelift) | Phase 2D+ |
| **[reference/03_ADAPTIVE_NUMERIC.md](./reference/03_ADAPTIVE_NUMERIC.md)** | Specialized fixnum/float opcodes | Phase 2B |
| **[reference/04_PERSISTENT_HEAP.md](./reference/04_PERSISTENT_HEAP.md)** | Time-travel debugging | Phase 2D+ |
| **[reference/FILE_SYSTEM_ABSTRACTION.md](./reference/FILE_SYSTEM_ABSTRACTION.md)** | `FileSystem` trait for WASM/embedded | Future |
| **[reference/STRING_ABSTRACTION_DESIGN.md](./reference/STRING_ABSTRACTION_DESIGN.md)** | Compact string representation | Phase 2B+ |
| **[reference/R7RS_PORTABLE_LIBRARIES.md](./reference/R7RS_PORTABLE_LIBRARIES.md)** | Portable library research | Future |

---

## Reference

Documents that informed decisions above. Read for context.

| Document | What It Informed |
|----------|-----------------|
| **[reference/ARCHITECTURE_LESSONS.md](./reference/ARCHITECTURE_LESSONS.md)** | Comparative analysis — Lua, V8, Chez, Gauche |
| **[reference/VM_CALLCC_DESIGN.md](./reference/VM_CALLCC_DESIGN.md)** | `call/cc` strategy selection (Strategy B chosen) |
| **[reference/VM_BACKEND_DESIGN.md](./reference/VM_BACKEND_DESIGN.md)** | Code sharing strategy with tree-walker |
| **[reference/02_EFFECT_CONTINUATIONS.md](./reference/02_EFFECT_CONTINUATIONS.md)** | Led to SRFI-226 choice |
| **[reference/05_DELIMITED_CONTINUATIONS.md](./reference/05_DELIMITED_CONTINUATIONS.md)** | Informed SRFI-226 ISA primitive design |

---

## Archive

Superseded documents. Do not use as implementation guides.
See [`archive/`](./archive/) — contains `VM_SPECIFICATION.md`, `COMPILATION_DESIGN.md`,
`VM_VALUE_ARCHITECTURE.md`, `SEXPR_SEPARATION_ARCHITECTURE.md`, `DESUGARER_DESIGN.md`,
`TAGGED_POINTERS.md`, `06_SELF_OPTIMIZING_AST.md`, `07_SYMBOLIC_EXECUTION.md`.

---

## Implementation Phases

### Phase 2A — Foundation (get tests passing)

1. ✅ **Primitives refactoring** — `patina-primitives` crate extracted (`b24c8ad`)
2. **`patina-vm` crate skeleton** — `CallFrame` (with `#[derive(Clone)]`), `VmState`, `CodeObject`
3. **Compiler** — 5-pass pipeline: Analysis → Closure Conversion → Tail Marking → Register Allocation → Codegen
4. **Execution loop** — fetch-decode-execute, `Call`/`TailCall`/`Return`
5. **SRFI-226 continuations** — `CallWithPrompt` / `AbortToPrompt` / `CaptureComposable`
6. **`Backend` trait impl** — `Interpreter<VmBackend>` passes all 1400 existing tests

### Phase 2B — Performance

7. **Benchmarks** — establish baseline vs tree-walker (target 5–10× on fib/numeric)
8. **Specialized opcodes** — `FixnumAdd`, `Car`, `Cdr`, etc.
9. **Tracing GC** — `rust-gc` for VM heap (fixes cycle leaks)
10. **String optimization** — compact representation for ASCII-heavy strings

### Phase 2C — Advanced (if benchmarks justify)

11. **NaN-boxing `VmVal`** — immediate floats, only if profiling shows float allocation is a bottleneck
12. **Generational GC** — if Phase 2B GC shows pressure

### Phase 2D+ — Deferred

13. Meta-tracing JIT (`reference/01_META_TRACING.md`)
14. Persistent heap / time-travel debugging (`reference/04_PERSISTENT_HEAP.md`)
