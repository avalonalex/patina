# Patina: Parallel Development Tracks

**Status:** Active plan
**Created:** 2026-03-08

Two independent tracks that can proceed in parallel. Neither blocks the other.
Both share the same `main` branch and the same test suite.

---

## Track A — VM Bootstrap

**Goal:** Get `Interpreter<VmBackend>` passing all 1400 existing tests.

**Owner:** VM-focused work
**Reference docs:** `PRD/phase2/VM_ISA.md`, `VM_COMPILER.md`, `VM_RUNTIME.md`

### Milestones

#### A1 — Crate skeleton
- Create `crates/patina-vm/` with `Cargo.toml`
- Define `CodeObject`, `Instruction` enum (from `VM_ISA.md §4`)
- Define `CallFrame` with `#[derive(Clone)]` — enforce in CI lint
- Define `VmState`, `VmClosure`, `PromptFrame`, `DynamicWindRecord`
- Define `VmContinuation`, `VmDelimitedContinuation`
- No execution yet — just types compile

#### A2 — Compiler: passes 1–3
- Pass 1: `NodeId` pre-pass + free variable analysis + mutation detection
- Pass 2: closure conversion → `ClosedExpr`
- Pass 3: tail position marking → `TailExpr`
- Unit tests for each pass (VM_TESTING.md §3.1)

#### A3 — Compiler: passes 4–5
- Pass 4: register allocation → `RegExpr`
  - Tail call no-overlap invariant tests
  - Two-pass top-level define handling
- Pass 5: code generation → `CodeObject`
  - Label patching
  - Nested lambda recursion
- End-to-end: `CoreExpr → CodeObject` for simple expressions

#### A4 — Execution loop: basics
- `LoadImmediate`, `LoadConst`, `Move`, `LoadGlobal`, `StoreGlobal`
- `LoadClosure`, `StoreClosure`
- `Jump`, `JumpIf`, `JumpUnless`
- `Call`, `Return` (non-tail)
- `CallPrimitive`
- `Define`
- Can run: `(+ 1 2)`, `(if #t 1 2)`, `(define x 42) x`

#### A5 — Tail calls and closures
- `TailCall` with frame reuse
- `MakeClosure`
- Variadic callee prologue (rest arg collection)
- Can run: fibonacci, map, filter, named-let loops

#### A6 — Continuation machinery
- `HeapObjectData::PromptTag` (opaque, with monotonic id)
- `CallWithPrompt`, `AbortToPrompt`, `CaptureComposable`, `InvokeContinuation`
- `run_wind_transition` shared function
- `ReturnMulti`, `ReceiveValues` + `vm.value_buffer`
- Continuation unit tests (VM_TESTING.md §3.3)

#### A7 — Backend trait + acceptance
- Implement `Backend` trait for `VmBackend`
- Wire into `patina-interpreter`
- Run `cargo test --package patina-tests --features vm`
- Triage failures: literals → calls → closures → continuations → libraries
- **Gate:** all 1400 tests pass

#### A8 — Chibi tests
- Run `./scripts/run_chibi_tests.sh` against VM backend
- **Gate:** 1159/1159 pass (same as tree-walker)

---

## Track B — Tree-Walker Compliance and Extensions

**Goal:** Audit and extend the tree-walker for missing R7RS-small features,
priority SRFIs, and hygiene/error quality improvements. All work here is
immediately usable and will also be inherited by the VM once it bootstraps.

**Owner:** Language compliance / library work
**Reference:** `PRD/phase2/R7RS_LARGE_STATUS.md`, chibi-scheme at
`~/Project/reference/chibi-scheme`

### B1 — Audit: what's missing from R7RS-small

Walk through the R7RS-small spec and chibi's test suite to identify any gaps
in the tree-walker. Categories to check:

| Area | Notes |
|---|---|
| `(scheme base)` | 1159/1159 chibi tests pass — likely complete, verify edge cases |
| `(scheme char)` | Unicode category predicates, case folding |
| `(scheme file)` | File I/O — partial |
| `(scheme read)` | `read` with datum labels (`#0=`, `#0#`) |
| `(scheme write)` | `write-shared`, `write-simple` |
| `(scheme eval)` | `eval`, `environment` |
| `(scheme repl)` | `interaction-environment` |
| `(scheme process-context)` | `command-line`, `get-environment-variable` |
| `(scheme time)` | `current-jiffy`, `jiffies-per-second` |
| `(scheme lazy)` | `make-promise`, `force`, `delay-force` |
| `(scheme inexact)` | Transcendentals — likely complete |
| `(scheme complex)` | `make-rectangular`, `make-polar` etc. |
| `(scheme cxr)` | `caaar`..`cddddr` |
| `(scheme r5rs)` | R5RS compatibility layer |
| Error messages | Quality, R7RS condition types |
| Tail position | Verify all R7RS-mandated tail positions are correct |

Produce a gap report: a short list of what's missing or incorrect with priority.

### B2 — Priority SRFIs (Scheme-implementable, high value)

These can be implemented entirely in Scheme as `.sld` + `.scm` files, requiring
no new Rust primitives. Chibi's implementations at
`~/Project/reference/chibi-scheme/lib/srfi/` serve as reference.

| SRFI | Library | Why priority |
|---|---|---|
| SRFI 1 | `(scheme list)` | Red Edition, heavily used in Scheme code |
| SRFI 111 | `(scheme box)` | Simple, one file, widely used |
| SRFI 128 | `(scheme comparator)` | Prerequisite for SRFI 113, 125, 132 |
| SRFI 133 | `(scheme vector)` | Red Edition, extends built-in vectors |
| SRFI 132 | `(scheme sort)` | Depends on SRFI 128, practical utility |
| SRFI 113 | `(scheme set)` | Depends on SRFI 128 |
| SRFI 125 | `(scheme hash-table)` | Depends on SRFI 128 |
| SRFI 158 | `(scheme generator)` | Tangerine Edition, widely used |

**Implementation approach:** For each SRFI:
1. Add `.sld` library definition to `lib/scheme/`
2. Implement in `.scm` using existing primitives where possible
3. Add Rust primitives only where Scheme can't do it (e.g. hash tables need a Rust backing)
4. Add tests to `patina-tests`

### B3 — Priority SRFIs (require new Rust primitives)

These need new heap object types or primitives in `patina-primitives` /
`patina-tree-walker`.

| SRFI | What's needed in Rust |
|---|---|
| SRFI 125 `(scheme hash-table)` | `HeapObjectData::HashMap`, hash/eq primitives |
| SRFI 143 `(scheme fixnum)` | Fixnum-specific arithmetic ops (overflow detection, bit ops) |
| SRFI 151 `(scheme bitwise)` | Bitwise ops on exact integers |
| SRFI 106 / ports | Binary port extensions |

### B4 — Error quality and condition system

R7RS specifies a condition system (`error-object?`, `error-object-message`,
`error-object-irritants`, `condition-type`). Audit current error types against
the spec and fill gaps. This improves both the REPL experience and library
interoperability.

### B5 — `syntax-case` (Phase 3 preview)

`syntax-case` is Phase 3 but foundational work can start here:
- Audit what `syntax-rules` currently can't express
- Identify which R7RS-large SRFIs require `syntax-case`
- See `PRD/phase2/SYNTAX_CASE_DESIGN.md`

---

## Coordination Points

Track A and Track B are independent but share:

- **`patina-primitives`** — Track B may add new primitives; Track A uses the
  same crate. New primitives added in Track B are immediately available to the VM.
- **`patina-core` heap** — Track B SRFI implementations that need new heap object
  types (e.g. hash tables) require adding `HeapObjectData` variants. These
  changes are in `patina-core` which both tracks share.
- **`patina-tests`** — Track B adds new tests; Track A must ensure the VM passes
  them. When Track A gates on "all tests pass", it includes tests added by Track B.
- **Merge discipline** — Both tracks work on `main`. Changes to shared crates
  (`patina-core`, `patina-primitives`, `patina-runtime`) should be small and
  reviewed to avoid breaking the other track.

---

## Success Criteria

| Track | Done when |
|---|---|
| **Track A** | `Interpreter<VmBackend>` passes all tests including chibi R7RS suite |
| **Track B** | Gap audit complete + priority SRFIs implemented + all new tests pass on tree-walker |
| **Both** | Track B tests also pass on VM backend (inherited via `Backend` trait) |
