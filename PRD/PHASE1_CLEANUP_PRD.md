# Phase 1 Cleanup: Foundation Hardening Before VM Backend

**Status**: Active
**Created**: 2026-02-27
**Context**: Phase 1 (tree-walker interpreter) is functionally complete with 1159/1159 chibi R7RS tests passing and ~1400 internal tests. This document defines the cleanup work needed to harden the foundation before starting Phase 2 (VM backend).

## Motivation

The tree-walker is the only backend today. Any bugs or infrastructure gaps fixed now only need to be fixed once. Once the VM backend exists, every cross-cutting concern (error reporting, continuation semantics, etc.) must be maintained in two places. The work described here is ordered to maximize the leverage of fixing things while there's a single backend.

A secondary motivation is developer experience. The VM backend will be built and debugged using this interpreter. Better error messages and correct continuation semantics will make that work significantly easier.

## Priority 1: Continuation and Dynamic-Wind Correctness — COMPLETE ✅

All 8 originally-ignored tests resolved. 0 ignored tests remain.

### Fixes Applied

1. **`test_dynamic_wind_callcc_exit`** — was already passing, re-enabled (2026-02-27)
2. **`test_raise_continuable_multiple_times`** — handler was popped but not re-pushed after continuable raise. Fixed by storing popped handler in `RaiseHandlerReturn` and re-pushing on return (2026-02-27)
3. **`test_dynamic_wind_with_exception`** — `raise` didn't unwind dynamic-winds before invoking handler. Fixed by storing `dynamic_winds` in `ExceptionHandler` at installation time; `apply_raise` and `maybe_route_error_through_cps` now call `run_wind_handlers` before handler invocation (2026-03-01)
4. **`test_exception_in_dynamic_wind_body_after_runs`** — same fix as #3 (2026-03-01)
5. **`test_callcc_escape_runs_dynamic_wind_after`** — test expectation was wrong. Verified against chibi: `(before)` is correct (escape value computed before after-thunk runs) (2026-03-01)
6. **`test_callcc_across_dynamic_wind_boundary`** — test expectation was wrong (condition never triggered re-entry). Corrected expectation, verified against chibi. Added new `test_callcc_reentry_replays_wind_thunks` test that verifies wind replay works (2026-03-01)
7. **`test_exception_with_stored_continuation`** — original program was an infinite loop (verified: chibi hangs). Replaced with terminating test of same pattern (2026-03-01)
8. **`test_with_exception_handler_nested`** — `call/cc` capture of `ExceptionHandlerCleanup` continuation returned `#<unspecified>` placeholder. Fixed by properly reifying `ExceptionHandlerCleanup` as its `original_cont` (2026-03-01)

### Files Changed

- `crates/patina-tree-walker/src/eval/cps_eval/types.rs` — Added `dynamic_winds` to `ExceptionHandler`, `popped_handler` to `RaiseHandlerReturn`
- `crates/patina-tree-walker/src/eval/cps_eval/application.rs` — `apply_with_exception_handler` captures winds; `apply_raise` unwinds before handler invocation
- `crates/patina-tree-walker/src/eval/cps_eval/exceptions.rs` — `maybe_route_error_through_cps` unwinds before handler invocation
- `crates/patina-tree-walker/src/eval/cps_eval/continuation.rs` — Re-push handler after continuable raise; properly reify `ExceptionHandlerCleanup` in `call/cc` capture

---

## Priority 2: Source Location Tracking

### Problem

All error messages lack source location information:

```
Error: Unbound variable: x
```

This makes debugging non-trivial programs extremely difficult, and will be a major pain point during VM development.

### Current State

Infrastructure partially exists:
- `SourceLocation` struct in `patina-core/src/error.rs` — fully defined with `source`, `line`, `column`, `length`
- `ErrorDetail` has `location: Option<SourceLocation>` field with builder methods
- `CoreExpr` has **no** source location fields yet
- Lexer does **not** track line/column positions
- A detailed 6-phase plan exists in `PRD/phase1/SOURCE_INFO_PLAN.md`

### Scope

Implement Phases 1–3 of `SOURCE_INFO_PLAN.md` (the high-value portion):

**Phase 1 — Non-breaking foundation:**
- Add `source: Option<SourceLocation>` to `CoreExpr` variants
- Make the lexer track line/column positions in tokens
- Add a `SourceMap` side-table mapping AST nodes to source positions

**Phase 2 — Parser/desugarer wiring:**
- Parser populates `SourceMap` during parsing
- Desugarer reads `SourceMap` to populate `CoreExpr` source fields

**Phase 3 — Error integration:**
- Wire `SourceLocation` into `EvalError` at key evaluation points
- Pretty-print errors with source context (file, line, column)
- REPL shows location in error display

Phases 4–6 (macro expansion tracking, stack traces) are deferred to post-VM.

### Open Question

The existing `SOURCE_INFO_PLAN.md` references `CpsExpr` extensively. The current architecture uses `CoreExpr` for both direct and CPS evaluation. The plan should be reconciled — likely just dropping `CpsExpr` references and applying all changes to `CoreExpr` only.

### Success Criteria

- Error messages include file:line:column for evaluation errors originating from source code
- REPL errors show line/column within the input
- Script mode errors show filename:line:column
- No measurable performance regression (source tracking is `Option<SourceLocation>`, which is zero-cost when `None`)

### Effort Estimate

3–5 days for Phases 1–3.

---

## Priority 3: Benchmark Baseline

### Problem

The VM backend's primary motivation is performance. Without a solid tree-walker baseline, there's no way to measure improvement. Current benchmark data is from December 2025 (commit `90cc977`) and only partially captured.

### Current State

Benchmark infrastructure exists and works:
- Criterion benchmarks in `crates/patina-tests/benches/scheme_benchmarks.rs`
- 4 benchmark groups: r7rs (7 programs), continuations (3), data structures (2), numeric (3)
- Script at `scripts/run_benchmarks.sh` generates reports
- Partial historical data in `benchmark_reports/` (only `sum` tracked, from Dec 2025)

### Scope

1. Run the full benchmark suite on current `main` with standard settings (20 samples)
2. Record all results as the official tree-walker baseline
3. Commit the baseline report to the repo
4. Consider adding 2–3 additional benchmarks that stress areas the VM should improve:
   - Deep recursion (e.g., `fib(30)`) — measures call overhead
   - Tight arithmetic loop — measures dispatch overhead
   - List processing (map/filter over large lists) — measures allocation pressure
5. Document the baseline in `MILESTONES.md`

### Known Data Point

From the December 2025 run: `fib(25)` took **2.94s**. Historical profiling showed `fib(25)` at ~0.92s before the TaggedValue migration. This regression should be investigated — it may indicate an optimization opportunity in the tree-walker worth addressing, or it may reflect measurement differences.

### Success Criteria

- Full benchmark results committed and reproducible
- Baseline numbers documented for all benchmark programs
- Clear methodology for comparing VM results against baseline

### Effort Estimate

1 day for running benchmarks, recording, and committing. 1–2 additional days if adding new benchmark programs.

---

## Priority 4: IR Visitor Completeness

### Problem

The `ExprVisitor` trait in `patina-ir/src/visitor.rs` is a skeleton. It has 5 `unimplemented!()` default methods and covers only 5 of 13 `CoreExpr` variants. The VM compiler will need to traverse `CoreExpr` for:
- Free variable analysis (determines closure captures)
- Constant folding
- Tail position marking
- Bytecode compilation

### Scope

1. Add visitor methods for all 13 `CoreExpr` variants
2. Implement default dispatch in `visit_expr` that routes to per-variant methods
3. Add a `walk_children` utility for default recursive traversal
4. Replace `unimplemented!()` with proper panicking messages or make methods required

### Success Criteria

- `ExprVisitor` covers all `CoreExpr` variants
- Default `visit_expr` implementation dispatches correctly
- At least one test pass (e.g., a simple expression counter) validates the visitor works

### Effort Estimate

1–2 days.

---

## Priority 5: Update Stale Documentation

### Problem

Several documents are significantly out of date:

- `PRD/README.md` — Still says Phase 1 is "47% complete" and lists `let/let*` as in progress
- `PRD/MILESTONES.md` — Narrative stops at December 2025; doesn't reflect 100% chibi compliance
- `docs/FEATURE_STATUS.md` — Referenced in `CLAUDE.md` but the file was archived; no active replacement exists

### Scope

1. Update `PRD/README.md` to reflect current project state (Phase 1 complete, Phase 2 planned)
2. Add milestone entries to `MILESTONES.md` for: 100% chibi compliance, TaggedValue migration, benchmark infrastructure, current state
3. Either restore `docs/FEATURE_STATUS.md` with current status or remove the reference from `CLAUDE.md`

### Effort Estimate

1–2 hours.

---

## Non-Goals (Deferred)

The following are explicitly **not** in scope for this cleanup:

| Item | Reason for Deferral |
|------|---------------------|
| Garbage collection | Correctly deferred to VM backend's arena-based heap. Tree-walker uses Rc which can't collect cycles but this is acceptable for Phase 1. |
| Clone optimization | Analysis complete (378 clone sites identified), but without profiling data the optimization targets are speculative. Run benchmarks first, profile, then optimize. |
| Delimited continuations (`call-with-continuation-prompt`) | The full prompt-based continuation system is designed in the VM spec. Fixing the existing `call/cc` + `dynamic-wind` bugs (Priority 1) is sufficient for Phase 1. |
| `(scheme load)` library | Rarely used, not part of standard test suites. |
| `unwrap()` audit | Investigation showed only 2 production `unwrap()` calls in parser and lexer, both guarded by preceding checks. The 101/83 counts reported earlier were dominated by test code. Not a real risk. |
| Source info Phases 4–6 | Macro expansion source tracking, stack traces, and advanced REPL integration can wait until after the VM backend proves the Phase 1–3 foundation works. |

---

## Sequencing

```
Priority 1 (Continuation bugs)  ──→  Priority 2 (Source locations)  ──→  Phase 2 (VM)
         │                                      │
         └── Priority 3 (Benchmarks) ───────────┘
                                                │
Priority 4 (IR Visitor) ───────────────────────┘
Priority 5 (Docs) ── anytime ──────────────────
```

- **P1 and P3** can run in parallel (independent work areas)
- **P2** benefits from P1 being done (source locations on errors that P1 fixes)
- **P4** should be done before Phase 2 starts (VM compiler depends on visitor)
- **P5** is low-effort housekeeping, do anytime

### Total Estimated Effort

8–13 days of focused work before starting Phase 2.

---

## Success Metrics

| Metric | Current | Target |
|--------|---------|--------|
| Ignored tests | 0 (was 8) | 0 ✅ |
| Error messages with source locations | 0% | >90% of eval errors |
| Benchmark baseline recorded | Partial (Dec 2025) | Complete, committed |
| IR visitor coverage | 5/13 variants | 13/13 variants |
| Stale documentation | 3 files | 0 files |
