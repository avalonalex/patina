# Documentation Reorganization Plan

**Date**: 2025-11-02

## Current State Analysis

### File Inventory

**Root (2 files):**
- `README.md` - Project overview ✅ Keep
- `CLAUDE.md` - Claude Code instructions ✅ Keep

**docs/ (11 files - ALL future/design):**
- Most are about Phase 4+ notebook features
- Not current implementation docs
- Should move to PRD/

**PRD/ (6 files):**
- Mix of proposals and getting-started guides
- Some overlap with docs/

**internal/ (5 files):**
- Test progress tracking (partially outdated)
- Some valuable references

**tests/ (1 file):**
- `FEATURE_MATRIX.md` - Current, valuable

## Problems

1. **Confusion**: `docs/` contains future features, not current docs
2. **Duplication**: Multiple testing docs, multiple quickstart guides
3. **Outdated**: Test progress files superseded by FEATURE_MATRIX
4. **Poor discoverability**: Hard to find current implementation docs

## Proposed Structure

```
/
├── README.md                          # Project overview (current)
├── CLAUDE.md                          # Claude Code instructions (current)
│
├── docs/                              # CURRENT implementation documentation
│   ├── README.md                      # Docs index
│   ├── GETTING_STARTED.md             # Quick start guide
│   ├── TESTING.md                     # How to run/write tests
│   ├── FEATURE_STATUS.md              # What's implemented (from FEATURE_MATRIX)
│   ├── DEVELOPMENT.md                 # How to contribute
│   ├── API.md                         # Interpreter API reference
│   └── CHIBI_REFERENCE.md             # How to use chibi for reference
│
├── PRD/                               # Product Requirements & Future Designs
│   ├── README.md                      # Index of proposals
│   ├── PROJECT_VISION.md              # Overall vision (from PROJECT_SUMMARY)
│   ├── ROADMAP.md                     # Phase 1-4 roadmap
│   │
│   ├── phase1/                        # Phase 1: R7RS compliance (current)
│   │   └── R7RS_ROADMAP.md
│   │
│   ├── phase2/                        # Phase 2: Gradual typing (future)
│   │   └── TYPING_DESIGN.md
│   │
│   ├── phase3/                        # Phase 3: Reactive concurrency (future)
│   │   └── REACTIVE_DESIGN.md
│   │
│   └── phase4/                        # Phase 4: Notebook system (future)
│       ├── NOTEBOOK_OVERVIEW.md       # Summary
│       ├── NOTEBOOK_FORMAT.md         # File format spec
│       ├── NOTEBOOK_DESIGN.md         # Architecture
│       ├── TUI_IMPLEMENTATION.md      # Terminal UI
│       ├── SYSTEM_INTEGRATION.md      # Three-tier system
│       ├── NATIVE_COMMANDS.md         # Command design
│       ├── DESIGN_DECISIONS.md        # Key decisions
│       ├── REPRODUCIBILITY.md         # Execution model
│       └── REPL_FEATURES.md           # REPL enhancements
│
├── internal/                          # Internal notes & milestones
│   ├── MILESTONES.md                  # Major accomplishments log
│   └── ARCHIVE/                       # Outdated docs (for reference)
│       ├── TEST_PROGRESS_20251102.md
│       └── TEST_RESULTS_20251102.md
│
└── tests/
    └── README.md                      # Testing guide (symlink to docs/TESTING.md)
```

## Migration Plan

### Phase 1: Create New Structure

1. Create `docs/README.md` as new docs index
2. Create `PRD/README.md` and phase subdirectories
3. Create `internal/ARCHIVE/`

### Phase 2: Move Future/Design Docs

Move from `docs/` → `PRD/phase4/`:
- NOTEBOOK_DESIGN.md
- NOTEBOOK_FORMAT.md
- TUI_IMPLEMENTATION.md
- SYSTEM_INTEGRATION.md
- NATIVE_COMMANDS.md
- DESIGN_DECISIONS.md
- REPRODUCIBILITY.md
- THREE_TIER_SUMMARY.md
- REPL_FEATURES.md
- GH_CLI_REFERENCE.md (if notebook-related)

### Phase 3: Create Current Documentation

**New files in `docs/`:**

1. `docs/GETTING_STARTED.md` - Consolidate from:
   - PRD/QUICKSTART.md
   - PRD/SETUP.md
   - Part of README.md

2. `docs/TESTING.md` - Consolidate from:
   - PRD/TESTING.md
   - tests/FEATURE_MATRIX.md (reference)
   - Part of CLAUDE.md testing section

3. `docs/FEATURE_STATUS.md` - Create from:
   - tests/FEATURE_MATRIX.md (enhanced version)
   - CLAUDE.md implementation status

4. `docs/DEVELOPMENT.md` - Create from:
   - CLAUDE.md architecture section
   - CLAUDE.md code organization section

5. `docs/API.md` - Extract from:
   - CLAUDE.md public API section
   - src/lib.rs docstrings

6. `docs/CHIBI_REFERENCE.md` - Move from:
   - internal/USING_CHIBI_REFERENCE.md

### Phase 4: Consolidate PRD

1. Create `PRD/PROJECT_VISION.md` from:
   - PRD/PROJECT_SUMMARY.md
   - README.md vision section

2. Create `PRD/ROADMAP.md` from:
   - PRD/NEXT_STEPS.md
   - PRD/ROADMAP_TO_R7RS_TESTS.md
   - internal/R7RS_ROADMAP.md

3. Move to phases:
   - `PRD/phase1/R7RS_ROADMAP.md`

### Phase 5: Archive Internal

Move to `internal/ARCHIVE/`:
- TEST_PROGRESS.md (rename with date)
- TEST_RESULTS.md (rename with date)
- TEST_ORGANIZATION_PROPOSAL.md (keep, or move to docs/)

Create `internal/MILESTONES.md`:
- 2025-11-02: Lambda implementation complete
- 2025-11-02: Test suite reorganization complete
- (future accomplishments)

### Phase 6: Update References

Update all cross-references in:
- README.md
- CLAUDE.md
- All moved files
- tests/compliance.rs comments

### Phase 7: Clean Up

Remove empty directories:
- Old `docs/` if empty
- Duplicate files after consolidation

## Benefits

1. **Clear Separation**: Current (docs/) vs Future (PRD/)
2. **Easy Discovery**: README → docs/README.md → specific topic
3. **Phase Organization**: Easy to see roadmap phases
4. **No Duplication**: Single source of truth for each topic
5. **Historical Record**: Archive preserves milestones

## Files to Delete (After Consolidation)

- `docs/INDEX.md` (replaced by docs/README.md)
- `PRD/QUICKSTART.md` (merged into docs/GETTING_STARTED.md)
- `PRD/SETUP.md` (merged into docs/GETTING_STARTED.md)
- `PRD/TESTING.md` (merged into docs/TESTING.md)
- Potentially: `PRD/PROJECT_SUMMARY.md` (if merged into PROJECT_VISION.md)
- Potentially: `PRD/NEXT_STEPS.md` (if merged into ROADMAP.md)

## Timeline

- Phase 1-2: 30 minutes (create structure, move future docs)
- Phase 3-4: 1-2 hours (create consolidated current docs)
- Phase 5-6: 30 minutes (archive, update references)
- Phase 7: 15 minutes (cleanup)

**Total: 2.5-3.5 hours**
