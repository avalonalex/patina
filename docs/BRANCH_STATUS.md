# Branch Status Summary

## Main Branch (`main`)
**Status:** ✅ Stable - Production Ready

```
Compliance Tests:   347/347 (100%)
Chibi Tests:        704/1108 (63.5%)
Clippy Warnings:    0
```

**What's Here:**
- Stable R7RS interpreter
- Tree-walking evaluator
- Full macro system (syntax-rules)
- Tail call optimization
- 63.5% chibi test compliance

**Safe to Use:** Yes

---

## CoreIR Migration Branch (`core-ir-migration`)
**Status:** ⚠️ Experimental - Not Ready for Main

```
Compliance Tests:   347/347 (100%) ✅
Chibi Tests:        704/1108 (63.5%) ✅ (with CoreExpr disabled)
                     18/1108 (1.6%) ❌ (with CoreExpr enabled)
```

**What's Here:**
- Everything from main branch
- **PLUS:** Complete CoreExpr infrastructure
- **PLUS:** Comprehensive test suites
- **PLUS:** Architecture documentation

**What's Broken:**
- Backend integration causes chibi test regression
- CoreExpr path is **disabled** to maintain stability

**Safe to Use:** Yes (CoreExpr disabled, same as main)
**Safe to Merge:** Not yet (need to fix integration)

---

## Comparison

| Feature | Main | CoreIR Branch |
|---------|------|---------------|
| Stable Interpreter | ✅ | ✅ |
| Chibi Tests | 704/1108 | 704/1108 (disabled) |
| CoreExpr Infrastructure | ❌ | ✅ (complete) |
| CoreExpr Enabled | ❌ | ❌ (disabled) |
| Test Coverage | Good | Excellent |
| Documentation | Good | Extensive |
| Risk Level | Low | Low (CoreExpr off) |

---

## Decision Guide

### Use Main Branch If:
- ✅ You want maximum stability
- ✅ You don't need CoreExpr optimization
- ✅ You're doing production work

### Use CoreIR Branch If:
- ✅ You want to work on CoreExpr integration
- ✅ You want the extra test coverage
- ✅ You want the architecture documentation
- ✅ You're okay with experimental code (even if disabled)

### Merge CoreIR → Main When:
- ✅ Macro expansion integration is fixed
- ✅ Chibi tests back to 704 with CoreExpr enabled
- ✅ Full regression testing complete

---

## How to Switch Branches

```bash
# Save current work
git add -A
git commit -m "WIP: current work"

# Switch to main
git checkout main

# Or switch to core-ir-migration
git checkout core-ir-migration

# Check status
./scripts/run_chibi_tests.sh
cargo test
```

---

## Current Recommendation

**Keep both branches alive:**

1. **Main** - For stable development and releases
2. **CoreIR** - For experimental CoreExpr work

**When to merge:**
- After fixing macro expansion integration
- After full testing confirms no regressions
- When CoreExpr benefits outweigh the risk

**For now:**
- Continue R7RS compliance work on **main**
- Work on CoreExpr integration on **core-ir-migration**
- Merge when ready, not before

---

## Branch History

```
main (stable)
  ├─ 70f4fa7 Merge macro hygiene fix
  ├─ fa3921d Rerun chibi test
  └─ ...

core-ir-migration (experimental)
  ├─ [Current HEAD] CoreExpr infrastructure complete
  ├─ Added comprehensive tests
  ├─ Fixed clippy warnings
  ├─ Disabled CoreExpr integration (regression)
  └─ Based on: main @ 70f4fa7
```

---

## Questions?

See:
- `BRANCH_README.md` - Detailed branch documentation
- `docs/MACRO_ARCHITECTURE_PROPOSAL.md` - How to fix integration
- `docs/SESSION_2025_11_22.md` - What was done today
