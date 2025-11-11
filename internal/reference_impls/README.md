# Reference Implementation Guides

This directory contains guides for leveraging reference Scheme implementations during Patina development.

## Quick Reference

| Implementation | Primary Use | Location |
|---------------|-------------|----------|
| **Chibi** | R7RS testing & simple implementations | `CHIBI_REFERENCE.md` |
| **Gauche** | Complex macros & production patterns | `GAUCHE_REFERENCE.md` |
| **Chez** | Performance & optimization strategies | `CHEZ_REFERENCE.md` |

## When to Use Which Reference

### Start Here: Chibi Scheme 🥇
**File:** `CHIBI_REFERENCE.md`

**Use for:**
- ✅ R7RS compliance testing (`tests/r7rs-tests.scm` - canonical test suite)
- ✅ Simple derived form implementations (`lib/init-7.scm`)
- ✅ Understanding expected behavior and edge cases
- ✅ First reference for any new feature

**Location:** `~/Project/reference/chibi-scheme`

**Example:**
```bash
# Check how a feature should behave
grep -A 10 "test.*lambda" \
  ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

# See simple implementation
grep "define.*not" \
  ~/Project/reference/chibi-scheme/lib/init-7.scm
```

### Complex Features: Gauche 🥈
**File:** `GAUCHE_REFERENCE.md`

**Use for:**
- ✅ **Macro system** - Nested ellipsis, complex patterns, auxiliary keywords
- ✅ Complex standard library implementations
- ✅ C implementation patterns (modern, readable C code)
- ✅ Production-grade robustness and edge case handling
- ✅ R7RS library system architecture

**Location:** `~/Project/reference/Gauche`

**Example:**
```bash
# Study macro pattern matching algorithm
less ~/Project/reference/Gauche/src/macro.c

# Complex library implementations
grep -A 20 "define.*fold" \
  ~/Project/reference/Gauche/lib/srfi-1.scm
```

**⭐ Use Gauche when:**
- Chibi's implementation is too simple
- Implementing complex macros (let-values, syntax-case)
- Need battle-tested production patterns
- Debugging tricky macro expansion issues

### Optimization: Chez Scheme 🥉
**File:** `CHEZ_REFERENCE.md`

**Use for:**
- ✅ High-performance string representation (O(1) indexing)
- ✅ Compiler architecture (future Phase 2+ work)
- ✅ Advanced optimization techniques
- ⚠️ Note: R6RS-focused, not pure R7RS

**Location:** `~/Project/reference/ChezScheme`

**Example:**
```bash
# Study string representation
less ~/Project/reference/ChezScheme/c/types.h
```

**⭐ Use Chez when:**
- Optimizing performance-critical code
- Designing internal representations
- Planning future compiler work

## Decision Flow

```
Implementing a feature?
│
├─ Need test cases?
│  └─→ Chibi: tests/r7rs-tests.scm
│
├─ Simple derived form?
│  └─→ Chibi: lib/init-7.scm
│
├─ Complex macro?
│  └─→ Gauche: src/macro.c ⭐⭐⭐
│
├─ Complex library function?
│  ├─→ Try Chibi first
│  └─→ Gauche if Chibi is too simple
│
├─ C implementation pattern?
│  └─→ Gauche: src/*.c (modern, readable)
│
└─ Performance optimization?
   └─→ Chez: c/*.c (for ideas, not direct copy)
```

## Current Phase 1 Priorities

### High Priority - Chibi
- ✅ Test suite for R7RS compliance (`tests/r7rs-tests.scm`)
- ✅ Simple derived forms (`lib/init-7.scm`)
- ✅ Basic numeric operations
- ✅ List operations
- ✅ String operations

### Medium Priority - Gauche
- ⚠️ Macro system improvements (if let-values migration needed)
- ⚠️ Complex list operations (fold, unfold, etc.)
- ⚠️ Exception handling (guard, raise)

### Lower Priority - Chez
- 💡 String optimization (Phase 2+)
- 💡 Compiler architecture (Phase 3+)

## Related Documentation

### Macro System
- `../NESTED_ELLIPSIS_ROADMAP.md` - Migration to Gauche's approach
- `../MACRO_ARCHITECTURE_DECISIONS.md` - Comparison of approaches
- `../MACRO_MIGRATION_STATUS.md` - Current migration status

### Testing Strategy
- `../../PRD/phase1/R7RS_TESTING_STRATEGY.md` - Overall testing approach
- Uses Chibi as primary reference for test cases

### Feature Status
- `../../docs/FEATURE_STATUS.md` - What's implemented, what's next
- Cross-reference with Chibi's test suite

## Repository Locations

All reference implementations are in `~/Project/reference/`:

```
~/Project/reference/
├── chibi-scheme/          # R7RS reference, test suite
├── Gauche/                # Production patterns, complex macros
└── ChezScheme/            # Performance, optimization
```

## Version Information

- **Chibi Scheme:** 0.11 (installed via Homebrew)
- **Gauche:** (local git clone)
- **Chez Scheme:** (local git clone)

All are actively maintained, production-quality implementations.

## Summary

**Three-tier strategy:**

1. **Chibi (Primary)** - "What to implement and how it should behave"
   - Pure R7RS-small reference
   - Canonical test suite
   - Simple, clean implementations

2. **Gauche (Complex Features)** - "How to implement complex things robustly"
   - Production-grade patterns
   - Complex macro system
   - Modern C integration

3. **Chez (Optimization)** - "How to make it fast"
   - Performance techniques
   - Internal representations
   - Compiler architecture

**For Phase 1 R7RS compliance:** Focus on Chibi, consult Gauche for complex features.

**For Phase 2+ (gradual typing, optimization):** Chez becomes more relevant.

---

**Last Updated:** 2025-11-10

See individual reference guides for detailed usage instructions.
