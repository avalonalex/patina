# Product Requirements & Design Documents

Strategic planning, design documents, and roadmaps for Patina's development phases.

## Current Status

**Phase 1 (R7RS-small tree-walker interpreter) — COMPLETE ✅**
- 1159/1159 chibi r7rs-tests.scm passing (100%)
- ~1400 internal tests passing
- All 15 R7RS-small libraries implemented (14 fully, `(scheme load)` omitted by design)

See `PRD/MILESTONES.md` for full history.

## Phase 1 Cleanup — COMPLETE ✅

All 5 priorities done. Archived at `PRD/ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md`.

- ✅ Priority 1: Continuation and dynamic-wind correctness (0 ignored tests)
- ✅ Priority 2: Source location tracking (rich caret-style errors in REPL and scripts)
- ✅ Priority 3: Benchmark baseline (38 benchmarks; O(1) primitive dispatch fix: -57% fib)
- ✅ Priority 4: IR visitor completeness (ExprVisitor covers all 13 CoreExprKind variants)
- ✅ Priority 5: Stale documentation

## Development Phases

### Phase 2: Bytecode VM Backend (Next)
**Status**: Planning

Compile `CoreExpr` IR to bytecode for 5–10× speedup. New `patina-vm/` crate implementing the `Backend` trait.

### Phase 3: syntax-case (Procedural Macros)
**Status**: Designed

Full `syntax-case` with `syntax->datum`, `datum->syntax`. See `phase2/SYNTAX_CASE_DESIGN.md`.

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
├── phase2/
│   ├── VM_BACKEND_DESIGN.md            # (to be created) VM backend design
│   ├── SYNTAX_CASE_DESIGN.md
│   └── R7RS_LARGE_STATUS.md
```

## Archive

Completed research and historical documents in `PRD/ARCHIVE/`. Key references:
- `ARCHIVE/numeric_research/NUMERIC_SUMMARY.md` — canonical numeric tower guide
- `ARCHIVE/source_info_2026_03/SOURCE_INFO_PLAN.md` — source tracking implementation
- `ARCHIVE/phase1_cleanup_2026_03/PHASE1_CLEANUP_PRD.md` — Phase 1 cleanup tracker
- `ARCHIVE/core_ir_migration_2025_11/` — CoreExpr migration
- `ARCHIVE/macro_research/` — syntax-rules hygiene research
