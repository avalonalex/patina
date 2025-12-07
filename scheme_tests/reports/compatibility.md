# Patina R7RS Compatibility Report

**Generated:** 2025-12-07 10:14:56
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 1077 | 94.1% |
| ❌ Failed | 6 | 0.5% |
| ⚠️ Error (crashed) | 62 | 5.4% |
| **Total** | **1145** | **100%** |

**Note:** "Error" means the test crashed before assertions could run (usually missing features like call/cc, guard).

## Section Breakdown

| Status | Section | Total | Passed | Failed | Errors |
|--------|---------|-------|--------|--------|--------|
| ✅ | 4.1 Primitive expression types | 27 | 27 | 0 | 0 |
| ✅ | 4.2 Derived expression types | 74 | 74 | 0 | 0 |
| ✅ | 4.3 Macros | 25 | 25 | 0 | 0 |
| ⚠️ | 5 Program structure | 16 | 10 | 0 | 6 |
| ✅ | 6.1 Equivalence Predicates | 25 | 25 | 0 | 0 |
| ✅ | 6.2 Numbers | 211 | 211 | 0 | 0 |
| ✅ | 6.3 Booleans | 18 | 18 | 0 | 0 |
| ✅ | 6.4 Lists | 65 | 65 | 0 | 0 |
| ✅ | 6.5 Symbols | 17 | 17 | 0 | 0 |
| ✅ | 6.6 Characters | 79 | 79 | 0 | 0 |
| ✅ | 6.7 Strings | 130 | 130 | 0 | 0 |
| ✅ | 6.8 Vectors | 43 | 43 | 0 | 0 |
| ✅ | 6.9 Bytevectors | 39 | 39 | 0 | 0 |
| ⚠️ | 6.10 Control Features | 34 | 25 | 0 | 9 |
| ⚠️ | 6.11 Exceptions | 25 | 2 | 2 | 21 |
| ❌ | 6.12 Environments and evaluation | 4 | 0 | 0 | 4 |
| ⚠️ | Read syntax | 93 | 85 | 0 | 8 |
| ⚠️ | Numeric syntax | 207 | 190 | 4 | 13 |
| ⚠️ | 6.14 System interface | 13 | 12 | 0 | 1 |

**Legend:** ✅ = All passing, ⚠️ = Partial, ❌ = None passing

## Failed Tests

### Test Failures

```
FAIL: something-went-wrong
FAIL: something-went-wrong
FAIL: (and (member z-str '("1e2+1.0i" "100.0+1.0i" "100.+1.i")) #t)
FAIL: (and (member z-str '("+inf.0+inf.0i" "+Inf.0+Inf.0i")) #t)
FAIL: (and (member z-str '("-inf.0+inf.0i" "-Inf.0+Inf.0i")) #t)
FAIL: (and (member z-str '("#d1.0+1.0i" "1.0+1.0i" "1.+1.i")) #t)
```

### Errors

```
Error: Undefined variable: define-record-type
Error: Undefined variable: pare?
Error: Undefined variable: pare?
Error: Undefined variable: kar
Error: Undefined variable: kdr
Error: Undefined variable: kons
Error: Undefined variable: call-with-current-continuation
Error: Internal error: Failed to desugar expression: Invalid syntax: apply requires at least 2 arguments (procedure and list)
Error: Undefined variable: test-error
Error: Undefined variable: test-error
Error: Undefined variable: test-error
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: dynamic-wind
Error: Undefined variable: with-exception-handler
Error: Undefined variable: error-object?
Error: Undefined variable: error-object-message
Error: Undefined variable: error-object-irritants
Error: Undefined variable: file-error?
Error: Undefined variable: file-error?
Error: Undefined variable: read-error?
Error: Undefined variable: read-error?
Error: Undefined variable: read-error?
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Undefined variable: with-exception-handler
Error: Undefined variable: guard
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: call-with-current-continuation
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Invalid syntax: read: Lexer error: Unexpected character: 0
Error: Invalid syntax: read: Lexer error: Unexpected character: 0
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Invalid syntax: read: Invalid syntax: Invalid real number: 1s2
Error: Invalid syntax: read: Invalid syntax: Invalid real number: +1s2
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: file-error?
```


## Full Results

See [results.txt](./results.txt) for complete test output.

## Notes

This report tracks Patina's compatibility with the R7RS-small specification
using the chibi-scheme test suite. The goal is to reach 100% compatibility
with all R7RS-small features.

### Known Limitations

- Exception handling (guard, raise) not yet implemented
- Continuations (call/cc) not yet implemented
- Some R7RS libraries not yet implemented: (scheme eval), (scheme r5rs), (scheme process-context)
- See `PRD/phase1/EXCEPTION_HANDLING.md` for exception handling roadmap

### Next Steps

See `docs/FEATURE_STATUS.md` for detailed R7RS compliance matrix and
`PRD/phase1/IMPLEMENTATION_STATUS.md` for roadmap.
