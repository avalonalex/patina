# Snow Compatibility & Performance Roadmap (Umbrella)

**Created:** 2026-06-19
**Status:** Planning → ready to execute
**Owner decisions:** interleave both tracks · library-compat-first (defer the fetcher) · clarity-safe optimizations only

This is the **cross-track overview**. Per-item detail lives in the two track PRDs:
- **Track P — VM performance (clarity-safe):** `PRD/TRACK_P_PERFORMANCE_PRD.md`
- **Track L — Snow library compatibility:** `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md`

---

## Context

Patina's two backends (register VM default + CPS tree-walker) both pass 1226/1226 chibi R7RS tests. The next goals are to (1) **consume existing Snow libraries** and (2) **improve VM performance without sacrificing educational clarity too much**. These run as **parallel, interleaved tracks**.

### Assessment summary
- **Performance.** The VM is a clean *first-generation* register machine (~4.2× the tree-walker). Hot path is heavy: string-`HashMap` primitive dispatch, string-hashed globals, per-call free-var `Vec` clones, **no GC** (arenas never reclaim). The `CallPrimitive` fast-path opcode exists but is unwired. Criterion benches measure the tree-walker, not the VM. → **Track P.**
- **Snow.** The `define-library`/`import`/`cond-expand`/`include`/`features` machinery is **R7RS-complete and ready to consume portable source**. Blockers are content/edge-cases: only `(chibi test)` + 9 SRFIs bundled, a few loading-gap edge cases, no fetcher, no FFI. → **Track L.**

### Relationship to existing docs
- `PRD/VM_OPTIMIZATION_ROADMAP.md` — 10-item perf catalog (P1–P10); Track P executes its clarity-safe subset and defers the rest.
- `PRD/future/PACKAGE_MANAGER_DESIGN.md` — `patina pkg` (deferred).
- `PRD/FFI_DESIGN.md` — two-layer FFI (deferred).

---

## Interleaving plan (milestones)

Tracks run in parallel; Track P's GC (P6) overlaps as a correctness sub-track.

| Milestone | Track P | Track L | Outcome |
|-----------|---------|---------|---------|
| **M1** | P0 baseline · P1 clones | L0 loading-gap fixes | Measurable VM; graceful loading; quick perf win banked |
| **M2** | P2 dispatch · P3 inline opcodes | L1 SRFIs (pure-Scheme set) | 2–5× on hot code; more portable packages load |
| **M3** | P4 globals · P5 cheap passes | L1 SRFIs (primitive-backed) · L2 `(chibi …)` | Broader speedups; dependency coverage for real packages |
| **M4** | P6 GC (pairs+vectors → objects) | L3 Snow validation harness | Long-running packages don't leak; Snow packages demonstrably run |

GC (P6) is the cross-cutting unblocker: real Snow workloads run long enough that the leaking arena matters, so it lands by M4 to make the L3 demonstration credible.

---

## Explicitly deferred (cross-cutting rationale)

**Performance — clarity tradeoff too high for now** (`PRD/VM_OPTIMIZATION_ROADMAP.md` P2/P6/P7/P8/P9/P10): flat `Vec<u32>` bytecode, threaded dispatch, liveness register allocation, NaN-boxed inline floats, bytecode serialization, continuation stack-slicing, JIT. If revisited, keep the readable match-based loop as a documented reference path.

**Snow — bigger scope, later phase:** `patina pkg` auto-fetcher (`PRD/future/PACKAGE_MANAGER_DESIGN.md`; L0's `./.patina/lib/` path is the forward hook) and FFI Layer 1/2 for C-shim packages (`PRD/FFI_DESIGN.md`).

---

## Housekeeping (fold in opportunistically)
- **Open bug:** `PRD/bugs/TREE_WALKER_CALLCC_MULTI_VALUES.md` — tree-walker wrong-arity error when a `call/cc` continuation is invoked with multiple values via `call-with-values` (VM already fixed). Blocks some SRFI-1 abort patterns.
- **Stale docs:** `PRD/phase2/R7RS_LARGE_STATUS.md` ("not started") contradicts `PRD/PARALLEL_TRACKS.md` (9 SRFIs done) — reconcile.
- **CLAUDE.md link drift:** points to `PRD/phase1/GC_DESIGN.md` / `CLONE_OPTIMIZATION_ANALYSIS.md` / `PRD/phase2/SYNTAX_CASE_DESIGN.md`, now under `PRD/ARCHIVE/phase1_optimization_2026_02/` and `PRD/macro/`; `docs/VM_STEPPER.md` exists but isn't listed.

---

## Verification (end to end)
- Routine: `cargo build --release && ./scripts/run_chibi_tests.sh` after every item (must stay 1226/1226).
- Perf: `cargo bench -p patina-tests` (VM-backed after P0) vs baseline; `./scripts/bench_compare.sh` cross-check.
- GC: dual CI lanes (`--no-default-features` vs `--features gc`) + `--gc-stress`.
- Snow: the L3 integration tests load and exercise real packages.
- Quality gate: `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt`.
