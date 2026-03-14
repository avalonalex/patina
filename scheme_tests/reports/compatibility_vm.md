# Patina R7RS Compatibility Report (VM Backend)

**Generated:** 2026-03-13 18:48:50
**Test Suite:** chibi-scheme r7rs-tests.scm
**Backend:** VM (experimental)

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 1153 | 99.0% |
| ❌ Failed | 5 | 0.4% |
| ⚠️ Error (crashed) | 7 | 0.6% |
| **Total** | **1165** | **100%** |

## Section Breakdown

| Status | Section | Total | Passed | Failed | Errors |
|--------|---------|-------|--------|--------|--------|
| ✅ | 4.1 Primitive expression types | 27 | 27 | 0 | 0 |
| ⚠️ | 4.2 Derived expression types | 74 | 72 | 2 | 0 |
| ⚠️ | 4.3 Macros | 27 | 23 | 2 | 2 |
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
| ⚠️ | 6.10 Control Features | 34 | 33 | 1 | 0 |
| ✅ | 6.11 Exceptions | 30 | 30 | 0 | 0 |
| ❌ | 6.12 Environments and evaluation | 5 | 0 | 0 | 5 |
| ✅ | Read syntax | 93 | 93 | 0 | 0 |
| ✅ | Numeric syntax | 220 | 220 | 0 | 0 |
| ✅ | 6.14 System interface | 13 | 13 | 0 | 0 |

**Legend:** ✅ = All passing, ⚠️ = Partial, ❌ = None passing

## Failed Tests

### Test Failures

```
FAIL: `(a `(b ,(+ 1 2) ,(foo ,(+ 1 3) d) e) f)
FAIL: `(a `(b ,,name1 ,',name2 d) e)
FAIL: (let ((x 'outer)) (let-syntax ((m (syntax-rules () ((m) x)))) (let ((x 'inner)) (m))))
FAIL: x
FAIL: (call-with-values (lambda () ((compose exact-integer-sqrt *) 12 75)) list)
FAIL: (inexact (eval '(sin 0) (environment '(scheme inexact))))
FAIL: (let ((f (eval '(lambda (f x) (f x x)) (null-environment 5)))) (f + 10))
```

### Errors

```
Error: runtime error: runtime error: Type error: remainder: expected integer, got non-numeric
Error: runtime error: runtime error: Type error: =: expected number, got non-numeric
Error: runtime error: runtime error: Internal error: Cannot load scheme base: Invalid syntax: load_scheme_library not yet supported in VM
Error: runtime error: runtime error: Internal error: Cannot load scheme base: Invalid syntax: load_scheme_library not yet supported in VM
Error: runtime error: runtime error: Internal error: environment: cannot load library: Invalid syntax: load_scheme_library not yet supported in VM
Error: runtime error: runtime error: Internal error: environment: cannot load library: Invalid syntax: load_scheme_library not yet supported in VM
Error: runtime error: runtime error: Internal error: environment: cannot load library: Invalid syntax: load_scheme_library not yet supported in VM
Error: runtime error: runtime error: Type error: eval: expected environment, got null
Error: runtime error: runtime error: Type error: eval: expected environment, got null
Error: runtime error: runtime error: Type error: eval: expected environment, got null
Error: runtime error: runtime error: Type error: eval: expected environment, got null
Error: runtime error: type error: expected a procedure, got null
Error: runtime error: runtime error: Type error: eval: expected environment, got null
```


## Full Results

See [results_vm.txt](./results_vm.txt) for complete test output.

## Comparison with Tree-Walker

Run `./scripts/run_chibi_tests.sh` to generate the tree-walker report for comparison.
The goal is for the VM backend to reach parity: 1159/1159 tests passing.
