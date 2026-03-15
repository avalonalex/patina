# Patina R7RS Compatibility Report (VM Backend)

**Generated:** 2026-03-15 11:32:52
**Test Suite:** chibi-scheme r7rs-tests.scm
**Backend:** VM (experimental)

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 1163 | 100.0% |
| ❌ Failed | 0 | 0.0% |
| ⚠️ Error (crashed) | 0 | 0.0% |
| **Total** | **1163** | **100%** |

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

See [results_vm.txt](./results_vm.txt) for complete test output.

## Comparison with Tree-Walker

Run `./scripts/run_chibi_tests.sh` to generate the tree-walker report for comparison.
The goal is for the VM backend to reach parity: 1159/1159 tests passing.
