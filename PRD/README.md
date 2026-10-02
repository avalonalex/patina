# Product Requirements & Design Documents

Strategic planning, design documents, and roadmaps for Patina's development phases.

## Current Status

**Phase 1 and Phase 2A — complete.** The register VM is the default backend;
the CPS tree-walker remains available. Both pass 1226 of 1226 chibi R7RS tests
as of 2026-09-29. See the [Phase 2 overview](phase2/README.md).

**Track L — complete 2026-09-29.** #551 closed the last recorded item, #423.
The [archived completion record](ARCHIVE/TRACK_L_LEFTOVERS.md) preserves the
corpus measurements, standing rules and separate follow-ups. New work belongs
in GitHub issues.

**Red and Tangerine — complete 2026-09-30.** #577 and #578 delivered the last
two recorded items. The [archived edition tracker](ARCHIVE/R7RS_LARGE_STATUS.md)
preserves the coverage tables; the standing
[bundling policy](../docs/README.md#library-bundling-policy) lives in `docs/`.
Later R7RS-large work remains separate.

See [MILESTONES.md](MILESTONES.md) for full history.

The implemented filesystem abstraction and public directory API are documented
in [docs/VFS_DESIGN.md](../docs/VFS_DESIGN.md). Its March proposals have been
retired; the [earlier design](ARCHIVE/vfs_2026_03/FILE_SYSTEM_ABSTRACTION.md)
is preserved as historical reference.

## Phase 1 Cleanup — COMPLETE ✅

All 5 priorities done. Archived at `PRD/ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md`.

- ✅ Priority 1: Continuation and dynamic-wind correctness (0 ignored tests)
- ✅ Priority 2: Source location tracking (rich caret-style errors in REPL and scripts)
- ✅ Priority 3: Benchmark baseline (38 benchmarks; O(1) primitive dispatch fix: -57% fib)
- ✅ Priority 4: IR visitor completeness (ExprVisitor covers all 13 CoreExprKind variants)
- ✅ Priority 5: Stale documentation

## Development Phases

### Phase 2: Bytecode VM Backend
**Status**: Phase 2A complete; further performance work tracked separately

`patina-vm` compiles `CoreExpr` IR to bytecode and implements the `Backend`
trait. See the [VM decisions](../docs/VM_DECISIONS.md),
[Track P](TRACK_P_PERFORMANCE_PRD.md) and the [GC PRD](GC_PRD.md).

### Phase 3: syntax-case (Procedural Macros)
**Status**: Designed

Full `syntax-case` with `syntax->datum`, `datum->syntax`. See
[the syntax-case design](macro/SYNTAX_CASE_DESIGN.md), including the deferred
mechanization evaluation transferred from Track H.

### Phase 4: Gradual Typing
**Status**: Planned

Typed Racket-style type inference and checking. Requires VM (performance) and syntax-case (annotation processing).

### Phase 5: Reactive Streams
**Status**: Planned

Project Reactor-style observable streams with backpressure.

### Phase 6: Logic Programming
**Status**: Planned

miniKanren embedding.

## Active Design Documents

```
PRD/
├── MILESTONES.md                       # Achievement history
├── GC_PRD.md                           # GC redesign: design and plan
├── phase2/
│   └── README.md                      # Phase 2A completion and follow-ons
├── macro/
│   └── SYNTAX_CASE_DESIGN.md           # Includes deferred mechanization (H5)
└── study/                              # Research records; study/gc/ backs GC_PRD.md
```

## Deferred Ideas

- [Review current R7RS-large volumes and fascicles](https://github.com/avalonalex/patina/issues/580) —
  assess the current drafts separately from completed Red/Tangerine library coverage.
- [Editor integration and Scheme formatting](future/EDITOR_AND_FORMATTER_PRD.md) —
  preserve the reader audit's tooling ideas for later; no implementation is scheduled.

## Archive

Completed research and historical documents in `PRD/ARCHIVE/`. Key references:

- [R7RS-large — Red and Tangerine](ARCHIVE/R7RS_LARGE_STATUS.md) — completed edition coverage and verification references
- [Track L — Third-Party Library Compatibility](ARCHIVE/TRACK_L_LEFTOVERS.md) — completed backlog, dated measurements and standing rules
- [Track H — Hygiene Assurance](ARCHIVE/completed_planning/TRACK_H_HYGIENE_ASSURANCE_PRD.md) — H1–H3's bounded harnesses and H4's completed evaluation; runtime defects remain in their live issues and triage entries
- `ARCHIVE/numeric_research/NUMERIC_SUMMARY.md` — canonical numeric tower guide
- `ARCHIVE/source_info_2026_03/SOURCE_INFO_PLAN.md` — source tracking implementation
- `ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md` — Phase 1 cleanup tracker
- `ARCHIVE/core_ir_migration_2025_11/` — CoreExpr migration
- `ARCHIVE/macro_research/` — syntax-rules hygiene research
