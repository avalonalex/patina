# Workspace Cleanup Plan

Generated: 2024-11-11
Status: Review needed before execution

## Executive Summary

After migrating to workspace structure, we have:
- ✅ Clean crate organization (7 crates)
- ✅ All tests passing (435 tests)
- ⚠️ Outdated documentation (93 .md files, many referencing old structure)
- ⚠️ Some redundant files from pre-workspace era

## Issues Found

### 1. Documentation is Outdated (HIGH PRIORITY)

**Problem:** CLAUDE.md and other docs reference old `src/` structure that no longer exists.

**Files needing updates:**
- `CLAUDE.md` - References `src/lexer/`, `src/eval/`, etc.
- `docs/DEVELOPMENT.md` - May have outdated paths
- `docs/API.md` - May reference old API
- `DOCUMENTATION_MAP.md` - May need restructure

**Action Items:**
- [ ] Update CLAUDE.md to reflect workspace structure
- [ ] Update all documentation references from `src/` to `crates/`
- [ ] Add workspace architecture section to CLAUDE.md
- [ ] Update command examples to use workspace crates

### 2. Root Directory Files (MEDIUM PRIORITY)

**Current root files:**
```
├── Cargo.toml         ✅ Clean (workspace only)
├── Cargo.lock         ✅ Keep
├── README.md          ⚠️ May need update
├── CLAUDE.md          ❌ Needs major update
├── DOCUMENTATION_MAP.md ⚠️ Check if current
├── .gitignore         ✅ Keep
├── docker-compose.yml ⚠️ Still needed?
├── Dockerfile         ⚠️ Still needed?
```

**Questions to answer:**
- Do we still use Docker? (docker-compose.yml, Dockerfile)
- Is DOCUMENTATION_MAP.md still useful or redundant with CLAUDE.md?

### 3. Test Organization References (LOW PRIORITY)

**Status:** Already documented well in `docs/TEST_ORGANIZATION.md`

**Action:** Just ensure other docs point to this canonical source.

### 4. Examples Directory (LOW PRIORITY)

**Contents:**
```
examples/
├── demo.scm               ✅ Basic demos
├── demo_r7rs.scm          ✅ R7RS features
├── test_lambda.rs         ❌ Rust test? Outdated?
└── *.scm.nb               ⚠️ Notebook files (future feature)
```

**Action Items:**
- [ ] Review if `test_lambda.rs` is still needed or should be in tests
- [ ] Consider moving notebook examples to separate docs folder (not implemented yet)

### 5. Crate Documentation (MEDIUM PRIORITY)

**Status:**
- Most crates have basic `//!` doc comments
- Could add more comprehensive crate-level docs

**Action Items:**
- [ ] Add README.md to each crate explaining its purpose
- [ ] Or ensure lib.rs has comprehensive crate docs
- [ ] Document inter-crate relationships

## Proposed Cleanup Actions

### Phase 1: Critical Documentation Updates

**Priority: HIGH - Do First**

1. **Update CLAUDE.md**
   ```markdown
   ## Workspace Architecture

   Patina uses a workspace structure with 7 crates:

   - `patina-runtime/` - Core types (Value, Environment)
   - `patina-frontend/` - Lexer, Parser, Macro Expander
   - `patina-tree-walker/` - Tree-walking interpreter backend
   - `patina-interpreter/` - High-level Interpreter API
   - `patina-ir/` - Core IR for nanopass (future)
   - `patina-repl/` - REPL with executable
   - `patina-tests/` - All integration tests

   ### Development Commands

   ```bash
   # Build workspace
   cargo build --release

   # Run REPL
   cargo run --release

   # Test everything
   cargo test

   # Test specific crate
   cargo test --package patina-frontend
   ```
   ```

2. **Global Search & Replace**
   - Replace `src/lexer/` → `crates/patina-frontend/src/lexer/`
   - Replace `src/parser/` → `crates/patina-frontend/src/parser/`
   - Replace `src/eval/` → `crates/patina-tree-walker/src/eval/`
   - Replace `src/value/` → `crates/patina-runtime/src/value/`
   - Replace `src/env/` → `crates/patina-runtime/src/environment/`

3. **Update All Architecture Diagrams**
   - Show workspace structure, not monolithic src/

### Phase 2: File Organization Review

**Priority: MEDIUM - Can wait**

1. **Review Docker files**
   - Are they still used?
   - If not, move to archive or delete
   - If yes, update to work with workspace

2. **Review examples/**
   - Move `test_lambda.rs` to appropriate location (or delete)
   - Document notebook format (or move to future docs)

3. **Review root README.md**
   - Ensure it reflects workspace structure
   - Update build/test instructions
   - Add workspace diagram

### Phase 3: Enhanced Documentation

**Priority: LOW - Nice to have**

1. **Per-Crate READMEs**
   ```
   crates/
   ├── patina-runtime/
   │   ├── README.md      # What is this crate?
   │   └── src/
   ├── patina-frontend/
   │   ├── README.md      # Frontend pipeline
   │   └── src/
   ...
   ```

2. **Architecture Documentation**
   - Create `docs/ARCHITECTURE.md`
   - Document dependency flow
   - Explain design decisions
   - Include diagrams

3. **Migration Guide**
   - Create `docs/MIGRATION_GUIDE.md`
   - Document the workspace refactoring
   - Explain what changed and why
   - Guide for contributors on new structure

## Recommended Immediate Actions

### Must Do Now (High Priority)

1. ✅ **Update CLAUDE.md** - This is the primary AI assistant guide
2. ✅ **Update Development Commands** - Ensure all examples work
3. ✅ **Fix Broken Path References** - Global search for `src/`

### Should Do Soon (Medium Priority)

4. Review and update README.md
5. Check if Docker files still needed
6. Review DOCUMENTATION_MAP.md relevance
7. Clean up examples/ directory

### Nice to Have (Low Priority)

8. Add per-crate documentation
9. Create architecture diagrams
10. Write migration guide

## Questions for User

Before proceeding with cleanup:

1. **Docker**: Do you still use Docker for development?
   - If NO: Delete docker-compose.yml and Dockerfile
   - If YES: Keep and update for workspace structure

2. **Notebooks**: The `.scm.nb` files - are these active or future?
   - If FUTURE: Move to docs/future/ or PRD/future/
   - If ACTIVE: Document the format

3. **Documentation Philosophy**:
   - Keep heavy docs (93 .md files)?
   - Or consolidate to fewer canonical sources?
   - Current structure: docs/ (user), PRD/ (design), internal/ (notes)

4. **Examples**:
   - Keep `test_lambda.rs` or move to tests?
   - Keep all example .scm files or consolidate?

## Estimated Effort

- **Phase 1 (Critical)**: 1-2 hours
- **Phase 2 (Medium)**: 2-3 hours
- **Phase 3 (Nice to have)**: 4-5 hours

Total cleanup: ~8-10 hours for comprehensive update.

## Success Criteria

After cleanup:
- ✅ No broken path references in documentation
- ✅ CLAUDE.md accurately describes workspace
- ✅ All command examples work
- ✅ New contributors can understand structure
- ✅ Documentation reflects reality
