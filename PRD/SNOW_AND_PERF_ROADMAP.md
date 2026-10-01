# Snow Compatibility & Performance Roadmap (Umbrella)

**Created:** 2026-06-19
**Status:** Track L complete 2026-09-29; performance follow-ups remain in Track P and the GC PRD
**Owner decisions:** interleave both tracks · library-compat-first (defer the fetcher) · clarity-safe optimizations only

This is the **cross-track overview**. Current status and standing rules live in:

- **Track P — VM performance:** [performance PRD](TRACK_P_PERFORMANCE_PRD.md)
  and [GC PRD](GC_PRD.md).
- **Track L — Snow library compatibility:** [archived completion record](ARCHIVE/TRACK_L_LEFTOVERS.md).
  #551 closed the last recorded item, #423. The 2026-09-28 corpus measurement
  passes 143 of 143 in-scope packages on both backends; #429 removed all
  import-only passes. Exclusions and classified reference differences remain
  explicit in the completion record.

The context, milestones and deferrals below preserve the **original June 2026
plan**, not a current implementation inventory. In particular, GC is now
always on, the VM's call paths have changed and Track L's library work is complete.

---

## Original context (2026-06-19)

Patina's two backends (register VM default + CPS tree-walker) both pass 1226/1226 chibi R7RS tests. The next goals are to (1) **consume existing Snow libraries** and (2) **improve VM performance without sacrificing educational clarity too much**. These run as **parallel, interleaved tracks**.

### Assessment summary
- **Performance.** The VM is a clean *first-generation* register machine (~4.2× the tree-walker). Hot path is heavy: string-`HashMap` primitive dispatch, string-hashed globals, per-call free-var `Vec` clones, **no GC** (arenas never reclaim). The `CallPrimitive` fast-path opcode exists but is unwired. Criterion benches measure the tree-walker, not the VM. → **Track P.**
- **Snow.** The `define-library`/`import`/`cond-expand`/`include`/`features` machinery is **R7RS-complete and ready to consume portable source**. Blockers are content/edge-cases: only `(chibi test)` + 9 SRFIs bundled, a few loading-gap edge cases, no fetcher, no FFI. → **Track L.**

### Relationship to existing docs
- `PRD/VM_OPTIMIZATION_ROADMAP.md` — 10-item perf catalog (P1–P10); Track P executes its clarity-safe subset and defers the rest.
- `PRD/future/PACKAGE_MANAGER_DESIGN.md` — `patina pkg` (deferred).
- `PRD/FFI_DESIGN.md` — two-layer FFI (deferred).

---

## Original interleaving plan (milestones)

Tracks run in parallel; Track P's GC (P6) overlaps as a correctness sub-track.

| Milestone | Track P | Track L | Outcome |
|-----------|---------|---------|---------|
| **M1** | P0 baseline · P1 clones | L0 loading-gap fixes | Measurable VM; graceful loading; quick perf win banked |
| **M2** | P2 dispatch · P3 inline opcodes | L1 SRFIs (pure-Scheme set) | 2–5× on hot code; more portable packages load |
| **M3** | P4 globals · P5 cheap passes | L1 SRFIs (primitive-backed) · L2 `(chibi …)` | Broader speedups; dependency coverage for real packages |
| **M4** | P6 GC (pairs+vectors → objects) | L3 Snow validation harness | Long-running packages don't leak; Snow packages demonstrably run |

GC (P6) is the cross-cutting unblocker: real Snow workloads run long enough that the leaking arena matters, so it lands by M4 to make the L3 demonstration credible.

---

## Original deferrals (cross-cutting rationale)

**Performance — clarity tradeoff too high for now** (`PRD/VM_OPTIMIZATION_ROADMAP.md` P2/P6/P7/P8/P9/P10): flat `Vec<u32>` bytecode, threaded dispatch, liveness register allocation, NaN-boxed inline floats, bytecode serialization, continuation stack-slicing, JIT. If revisited, keep the readable match-based loop as a documented reference path.

**Snow — bigger scope, later phase:** `patina pkg` auto-fetcher (`PRD/future/PACKAGE_MANAGER_DESIGN.md`; L0's `./.patina/lib/` path is the forward hook) and FFI Layer 1/2 for C-shim packages (`PRD/FFI_DESIGN.md`).

---

## Follow-ups outside Track L's completed backlog

- Reporting: retain failure evidence
  ([#379](https://github.com/avalonalex/patina/issues/379)) and remove timing-only
  churn from committed chibi reports ([#380](https://github.com/avalonalex/patina/issues/380)).
- Package distribution and FFI remain separate, deferred efforts in their
  designs linked above.
- Use the [bundling policy](../docs/README.md#library-bundling-policy) for library scope,
  [VM decisions](../docs/VM_DECISIONS.md) for the implemented architecture and
  [GC design](../docs/GC_DESIGN.md) for collection rules. New defects belong
  in GitHub issues.

---

## Verification (current guidance)

Use [AGENTS.md](../AGENTS.md) for checks appropriate to a change and
[test organization](../docs/TEST_ORGANIZATION.md) for commands and coverage.

- Routine code verification: `cargo build --release && ./scripts/run_chibi_tests.sh`
  and affected Rust tests; backend semantics also run `./scripts/run_chibi_tests_tree_walker.sh`.
- Performance: follow Track P's interleaved baseline/change/baseline protocol.
- GC/rooting: `./scripts/run_gc_differential.sh` against release and debug
  builds, the latter with poison assertions.
- Corpus: `cargo run --release -p patina-compat -- run`; maintained smoke
  drivers are gated in CI on both backends.
- Full Rust gate: `cargo test --all --lib --tests`,
  `cargo clippy --all-targets --all-features -- -D warnings` and
  `cargo fmt --all -- --check`.
