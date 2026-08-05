# Patina R7RS Compatibility Report

**Generated:** 2026-08-05 16:21:27
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 1163 | 100.0% |
| ❌ Failed | 0 | 0.0% |
| ⚠️ Error (crashed) | 0 | 0.0% |
| **Total** | **1163** | **100%** |

**Note:** "Error" means the test crashed before assertions could run (usually missing features like call/cc, guard).

## Section Breakdown

| Status | Section | Total | Passed | Failed | Errors |
|--------|---------|-------|--------|--------|--------|
| ✅ | 4.1 Primitive expression types | 27 | 27 | 0 | 0 |
| ✅ | 4.2 Derived expression types | 74 | 74 | 0 | 0 |
| ✅ | 4.3 Macros | 25 | 25 | 0 | 0 |
| ✅ | 5 Program structure | 15 | 15 | 0 | 0 |
| ✅ | 6.1 Equivalence Predicates | 25 | 25 | 0 | 0 |
| ✅ | 6.2 Numbers | 211 | 211 | 0 | 0 |
| ✅ | 6.3 Booleans | 18 | 18 | 0 | 0 |
| ✅ | 6.4 Lists | 65 | 65 | 0 | 0 |
| ✅ | 6.5 Symbols | 17 | 17 | 0 | 0 |
| ✅ | 6.6 Characters | 79 | 79 | 0 | 0 |
| ✅ | 6.7 Strings | 130 | 130 | 0 | 0 |
| ✅ | 6.8 Vectors | 43 | 43 | 0 | 0 |
| ✅ | 6.9 Bytevectors | 39 | 39 | 0 | 0 |
| ✅ | 6.10 Control Features | 34 | 34 | 0 | 0 |
| ✅ | 6.11 Exceptions | 30 | 30 | 0 | 0 |
| ✅ | 6.12 Environments and evaluation | 5 | 5 | 0 | 0 |
| ✅ | Read syntax | 93 | 93 | 0 | 0 |
| ✅ | Numeric syntax | 220 | 220 | 0 | 0 |
| ✅ | 6.14 System interface | 13 | 13 | 0 | 0 |

**Legend:** ✅ = All passing, ⚠️ = Partial, ❌ = None passing

## Failed Tests


## Full Results

See [results.txt](./results.txt) for complete test output.

## Notes

This report tracks Patina's compatibility with the R7RS-small specification
using the chibi-scheme test suite. The goal is to reach 100% compatibility
with all R7RS-small features.

### Known Limitations

- `(scheme load)` library not yet implemented
- See `docs/FEATURE_STATUS.md` for detailed compliance matrix

### Next Steps

See `docs/FEATURE_STATUS.md` for detailed R7RS compliance matrix and
`PRD/phase1/IMPLEMENTATION_STATUS.md` for roadmap.
