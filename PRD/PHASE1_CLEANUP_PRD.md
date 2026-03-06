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

## Priority 2: Source Location Tracking — COMPLETE ✅

All 5 phases implemented (2026-03-03 through 2026-03-05). Errors now show caret-style context with macro expansion chains in both REPL and script mode.

**Example output:**
```
Error: Undefined variable: y
  at <repl-1>:1:15
   1 | (let ((x 1)) (+ x y))
     ^^^^^^^^^^^^^^^^^^^^^
  macro expansion: let
```

### What Was Delivered

- **Phase 1** — CoreExpr/CpsExpr wrapper structs with `source: Option<SourceLocation>`; lexer line/col tracking; `SourceMap` side-table
- **Phase 2** — Parser records list/vector/quote positions; desugarer populates `CoreExpr.source`; `eval_with_source_map` on TreeWalker
- **Phase 3** — `EvalError::WithLocation`; CPS stamps source on `App`/`LetVal` nodes; `format_eval_error_with_source()`
- **Phase 4** — Gauche-style expansion chain tracking; `SourceMap::expansion_records`; `stamp_expansion_source()` in desugarer
- **Phase 5** — `eval_str/program_with_source_name()` APIs; REPL `<repl-N>` counter; script mode filename threading; `format_interpreter_error()`

**Archived:** `PRD/ARCHIVE/source_info_2026_03/SOURCE_INFO_PLAN.md`

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

## Priority 4: IR Visitor Completeness — COMPLETE ✅

`ExprVisitor` in `patina-ir/src/visitor.rs` now covers all 13 `CoreExprKind` variants with a working default-dispatch design.

### What Was Delivered

- Dropped the generic `Output` associated type — the trait is now a pure walk/analysis visitor (side effects via `&mut self`); transforms use `CoreExpr::map_children`
- `visit_expr` has a default that calls `walk_expr` (dispatch to per-variant methods)
- `walk_expr` matches all 13 variants and routes to the correct `visit_*` method
- Leaf nodes (`visit_literal`, `visit_var`, `visit_quote`, `visit_quasiquote`, `visit_import`) default to no-op
- Interior nodes (`visit_lambda`, `visit_if`, `visit_set`, `visit_begin`, `visit_define`, `visit_expand`, `visit_app`, `visit_apply`) default to recursing into children
- 5 new tests: node counter, nested define counter, free-variable collector (simple + nested lambda), default walk over all 13 variants without panicking

---

## Priority 5: Update Stale Documentation — COMPLETE ✅

### What Was Done

- **`CLAUDE.md`**: Reduced from 892 → 147 lines per Anthropic's recommendations. Removed sections Claude can infer from source (deep-dive architecture, API signatures, code snippets). Kept: commands, file locations, non-obvious rules (RefCell discipline, TaggedValue Copy, Arc vs Rc, macros-vs-forms), feature recipes, error type table. Removed stale references to `DefineSyntax` CoreExpr variant (deleted 2025-12-01), `SpecialFormRegistry` (removed), `docs/FEATURE_STATUS.md` (archived), `SOURCE_INFO_PLAN.md` (archived 2026-03-05).
- **`PRD/README.md`**: Full rewrite — was "47% complete / let in progress", now reflects Phase 1 complete, Phase 2 (VM) next, with correct phase numbering. Removed dead links.
- **`PRD/MILESTONES.md`**: Removed stale "Future Milestones", "Metrics Tracking", and "Archive" sections from November 2025 (showing items long completed as pending). Historical entries preserved.

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
| Source info Phase 6 (stack traces) | Call-stack tracking with definition-site locations deferred to post-VM. |
| Source info Phase 4b (L3/Racket-style) | Identifier-level origin tracking deferred; L2 (Gauche-style) implemented in Phase 4 gives the majority of the user-visible improvement. |

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
| Error messages with source locations | 0% (was) | >90% of eval errors ✅ |
| Benchmark baseline recorded | Partial (Dec 2025) | Complete, committed |
| IR visitor coverage | 5/13 variants (was) | 13/13 variants ✅ |
| Stale documentation | 3 files (was) | 0 files ✅ |
