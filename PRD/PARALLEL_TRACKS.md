# Patina: Parallel Development Tracks

**Status:** Track A complete, Track B1 complete
**Created:** 2026-03-08
**Updated:** 2026-03-15

---

## Track A — VM Bootstrap ✅ COMPLETE

**Goal:** Get `Interpreter<VmBackend>` passing all R7RS chibi tests.
**Result:** 1163/1163 (100%) — matches tree-walker.

**Design docs:** `docs/VM_ISA.md`, `docs/VM_COMPILER.md`, `docs/VM_RUNTIME.md`, `docs/VM_DECISIONS.md`

### Completed Milestones

- **A1** — Crate skeleton (types, `CallFrame: Clone`) ✅
- **A2** — Compiler passes 1–3 (analysis, closure conversion, tail marking) ✅
- **A3** — Compiler passes 4–5 (register allocation, codegen) ✅
- **A4** — Execution loop basics (load/store, control flow, call/return, primitives) ✅
- **A5** — Tail calls and closures (frame reuse, MakeClosure, MutableCell) ✅
- **A6** — Continuations (call/cc, dynamic-wind, prompts, values, exceptions) ✅
- **A6.5** — REPL improvements (shared editor infra, `(vm-compile expr)` disassembly) ✅
- **A7** — Backend trait + acceptance (quasiquote expansion, import handling, HO primitives, exception handling) ✅
- **A8** — Chibi R7RS compliance (quasiquote fix, alpha-rename hygiene, internal defines, value_buffer fix) ✅

---

## Track B — Tree-Walker Compliance and Extensions

**Goal:** Audit and extend the tree-walker for missing R7RS-small features,
priority SRFIs, and hygiene/error quality improvements. All work here is
immediately usable and will also be inherited by the VM.

**Reference:** `PRD/phase2/R7RS_LARGE_STATUS.md`, chibi-scheme at
`~/Project/reference/chibi-scheme`

### B1 — R7RS-small compliance audit ✅ COMPLETE

Full audit against R7RS-small spec (LaTeX source). All gaps found and resolved.
See `PRD/ARCHIVE/R7RS_AUDIT.md` for the detailed report.

**Completed (2026-03-15):**
- **`(scheme load)`** — `load` primitive with optional environment arg
- **`(scheme repl)`** — `interaction-environment` primitive
- **`(scheme r5rs)`** — complete rewrite: 12 library imports, ~160 exports, `exact->inexact`/`inexact->exact` aliases
- **`include` / `include-ci`** — expression-level file inclusion in desugarer
- **`syntax-error`** — compile-time error signaling in desugarer
- **Auxiliary syntax** — `else` and `=>` exported as bindings from `(scheme base)`
- **Circular data** — fixed stack overflow in `strip_identifiers_tagged` for quoted circular literals
- **VM nested `eval`** — fixed missing CodeObject bug when closures from prior evals are called through subsequent evals

**Result:** 16/16 R7RS-small libraries, all procedures, all syntax forms. 1163/1163 chibi tests on both backends.

### B2 — Priority SRFIs (Scheme-implementable)

| SRFI | Library | Why priority |
|---|---|---|
| SRFI 1 | `(scheme list)` | Red Edition, heavily used |
| SRFI 111 | `(scheme box)` | Simple, widely used |
| SRFI 128 | `(scheme comparator)` | Prerequisite for SRFI 113, 125, 132 |
| SRFI 133 | `(scheme vector)` | Red Edition |
| SRFI 132 | `(scheme sort)` | Depends on SRFI 128 |
| SRFI 113 | `(scheme set)` | Depends on SRFI 128 |
| SRFI 125 | `(scheme hash-table)` | Depends on SRFI 128 |
| SRFI 158 | `(scheme generator)` | Tangerine Edition |

### B3 — Priority SRFIs (require new Rust primitives)

| SRFI | What's needed in Rust |
|---|---|
| SRFI 125 `(scheme hash-table)` | `HeapObjectData::HashMap`, hash/eq primitives |
| SRFI 143 `(scheme fixnum)` | Fixnum-specific arithmetic ops |
| SRFI 151 `(scheme bitwise)` | Bitwise ops on exact integers |

### B4 — Error quality and condition system

Audit error types against R7RS condition system spec and fill gaps.

### B5 — `syntax-case` (Phase 3 preview)

See `PRD/phase2/SYNTAX_CASE_DESIGN.md`.

---

## Coordination Points

Track A and Track B share:
- **`patina-primitives`** — new primitives available to both backends
- **`patina-core` heap** — new `HeapObjectData` variants shared
- **`patina-tests`** — Track B tests must pass on VM too
- **Merge discipline** — changes to shared crates should be reviewed

---

## Success Criteria

| Track | Done when |
|---|---|
| **Track A** | ✅ Complete — 1163/1163 chibi tests passing |
| **Track B1** | ✅ Complete — R7RS-small audit: 16/16 libraries, all gaps resolved |
| **Track B2+** | Priority SRFIs implemented + all new tests pass on both backends |
