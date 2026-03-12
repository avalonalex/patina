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

#### A1 — Crate skeleton ✅
- Create `crates/patina-vm/` with `Cargo.toml`
- Define `CodeObject`, `Instruction` enum (from `VM_ISA.md §4`)
- Define `CallFrame` with `#[derive(Clone)]` — enforce in CI lint
- Define `VmState`, `VmClosure`, `PromptFrame`, `DynamicWindRecord`
- Define `VmContinuation`, `VmDelimitedContinuation`
- No execution yet — just types compile

#### A2 — Compiler: passes 1–3 ✅
- Pass 1: `NodeId` pre-pass + free variable analysis + mutation detection
- Pass 2: closure conversion → `ClosedExpr` (with MutableCell boxing, global-filtering)
- Pass 3: tail position marking → `TailExpr`
- Unit tests for each pass

#### A3 — Compiler: passes 4–5 ✅
- Pass 4: register allocation → `RegExpr`
  - Tail call no-overlap invariant: args materialised into fresh temps
  - Cell read/write variants for boxed params
- Pass 5: code generation → `CodeObject`
  - Label patching, nested lambda recursion
  - AllocCell prologue for boxed params
  - TailCall passes `arg_tmps` directly (no pre-move to r0..rn; avoids func clobbering)
- End-to-end: `CoreExpr → CodeObject` for simple expressions

#### A4 — Execution loop: basics ✅
- `LoadImmediate`, `LoadConst`, `Move`, `LoadGlobal`, `StoreGlobal`
- `LoadClosure`, `StoreClosure`
- `Jump`, `JumpIf`, `JumpUnless`
- `Call`, `Return` (non-tail)
- `Define`
- Primitive dispatch via `try_call_primitive()` / `VmApplyContext` (heap-only)
- `install_primitives()` — all patina-primitives wired into globals
- Can run: `(+ 1 2)`, `(if #t 1 2)`, `(define x 42) x`

#### A5 — Tail calls and closures ✅
- `TailCall` with frame reuse (register window grow if needed)
- `TailApply` for `apply` in tail position
- `MakeClosure`, `AllocCell`, `ReadCell`, `WriteCell`
- Variadic callee prologue (rest arg collection via `build_list`)
- Can run: fibonacci, map, filter, named-let loops

#### A6 — Continuation machinery ✅
**ISA instructions:**
- `CallWithPrompt`, `AbortToPrompt`, `CaptureComposable`, `InvokeContinuation`
- `ReturnMulti`, `ReceiveValues` + `vm.value_buffer`

**Control primitive interception:**
- `VmControlPrimitive` enum + `vm_control_primitive()` checks qualified name
- Intercepts in both `Call` and `TailCall` dispatch before `try_call_primitive`
- `handle_control_primitive()` handles all 6 operators:
  - `dynamic-wind` — `run_thunk` for before/body/after; detects abort-escaped body
  - `call-with-continuation-prompt` — pushes `PromptFrame`, calls body
  - `abort-current-continuation` — captures delimited continuation, runs exit winds, truncates, calls handler
  - `call/cc` — captures full `VmContinuation` (with `deliver_reg`), calls proc
  - `values` — writes to `value_buffer`
  - `call-with-values` — runs producer via `run_thunk`, calls consumer with produced values

**Continuation invocation:**
- `try_invoke_continuation()` — continuations callable as procedures (e.g. `(k 21)`)
- Full continuations: restore entire state, deliver to `deliver_reg`
- Delimited continuations: relocate + append frames, run wind entry thunks

**Infrastructure:**
- `run_loop_until(state, exit_depth)` — parameterized loop for nested thunk execution
- `run_thunk()` — synchronous 0-arg closure execution
- `run_wind_transition()` — before/after thunk transition between wind states
- `pop_resolved_prompts()` — cleanup prompts on normal Return
- Heap: `VmContinuationRef(u64)` / `VmDelimitedContinuationRef(u64)` opaque handles
  with side tables on `VmState` (avoids circular dep with patina-core)
- TailCall/TailApply: fixed `exit_depth` check (was `is_empty()`)
- TailCall of control prims: pops frame first, dispatches from parent perspective

**Verified working:**
- `(call-with-continuation-prompt body tag handler)` — normal return and abort
- `(abort-current-continuation tag val)` — with handler invocation
- `(dynamic-wind before body after)` — normal and abort-escaped
- `(call/cc (lambda (k) ...))` — capture and escape `(k val)`
- `(values v1 v2 ...)` + `(call-with-values producer consumer)`
- 24/24 VM tests pass, 1159/1159 chibi tests unaffected, clippy clean

#### A6.5 — REPL improvements ✅ (done alongside A6)
- Shared `SchemeHelper`, `make_editor()`, `run_repl_loop()` — both REPLs use same editing infra
- `(vm-compile expr)` — disassemble bytecode without executing
- `disasm.rs` — pretty-printer for CodeObjects with instruction mnemonics
- `VmBackend::disasm_source()` — parse → desugar → compile → disassemble pipeline

#### A7 — Backend trait + acceptance (in progress)
- `VmBackend` implements `Backend` trait (eval pipeline working)
- Test infrastructure: `cargo test --package patina-tests --features vm-backend`
- **Quasiquote expansion** ✅ — `quasiquote_expand.rs` compile-time expansion
- **Import handling** ✅ — `CoreExprKind::Import` intercepted in `Backend::eval()`
- **VmClosure as procedure** ✅ — `is_procedure()` recognizes VmClosure
- **Primitive dispatch from control ops** ✅ — `call_any()` for mixed VmClosure/Primitive
- **Current score:** ~850 pass, ~73 fail (out of ~920 feature-flag-affected tests)
- **Remaining blockers:**
  - Exception handling (`with-exception-handler`, `guard`, `raise`) — needs VM integration
  - Nested dynamic-wind + `set!` — ReadCell mutation analysis bug
  - Higher-order primitives (`vector-map`, `for-each`) — apply_proc stub
  - Hygiene + quasiquote in macros — symbol identity mismatch
- **Gate:** all ~1400 tests pass

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
