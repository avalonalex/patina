# Phase 2: VM Backend

**Status:** Phase 2A complete — 1163/1163 R7RS chibi tests passing (100%)
**Goal:** Register-based bytecode VM implementing the `Backend` trait.

---

## Design Documents

Phase 2A design docs have been moved to `docs/` (updated to match implementation):

| Document | Purpose |
|----------|---------|
| **[docs/VM_DECISIONS.md](../../docs/VM_DECISIONS.md)** | Master reference — all settled decisions |
| **[docs/VM_ISA.md](../../docs/VM_ISA.md)** | Instruction set, semantics, tail calls, control primitives |
| **[docs/VM_COMPILER.md](../../docs/VM_COMPILER.md)** | 2 pre-passes + 5-pass compiler: `CoreExpr → CodeObject` |
| **[docs/VM_RUNTIME.md](../../docs/VM_RUNTIME.md)** | `VmState`, execution loop, control primitives, exceptions |
| **[docs/VM_TESTING.md](../../docs/VM_TESTING.md)** | Testing layers and commands |

---

## Forward-Looking

Not in scope for Phase 2A but tracked here.

| Document | Feature | Phase |
|----------|---------|-------|
| **[SYNTAX_CASE_DESIGN.md](./SYNTAX_CASE_DESIGN.md)** | `syntax-case` procedural macros | Phase 3 |
| **[R7RS_LARGE_STATUS.md](./R7RS_LARGE_STATUS.md)** | R7RS-large library tracking | Ongoing |
| **[reference/01_META_TRACING.md](./reference/01_META_TRACING.md)** | Meta-tracing JIT (Cranelift) | Phase 2D+ |
| **[reference/03_ADAPTIVE_NUMERIC.md](./reference/03_ADAPTIVE_NUMERIC.md)** | Specialized fixnum/float opcodes | Phase 2B |

---

## Implementation Status

### Phase 2A — Foundation ✅ Complete

- ✅ Primitives refactoring — `patina-primitives` crate
- ✅ `patina-vm` crate — `CallFrame`, `VmState`, `CodeObject`
- ✅ Compiler — quasiquote expansion + alpha-rename + 5-pass pipeline
- ✅ Execution loop — all instructions, call dispatch
- ✅ Control primitives — continuations, exceptions, dynamic-wind, values
- ✅ `Backend` trait impl — `Interpreter<VmBackend>` passes all tests
- ✅ R7RS compliance — 1163/1163 chibi tests passing

### Phase 2B — Performance (future)

- Benchmarks — establish baseline vs tree-walker
- Specialized opcodes — `FixnumAdd`, `Car`, `Cdr`, etc.
- Tracing GC — handles cycle leaks
- String optimization

---

## Archive

Superseded documents in [`archive/`](./archive/).
