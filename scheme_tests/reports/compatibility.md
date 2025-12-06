# Patina R7RS Compatibility Report

**Generated:** 2025-12-06 14:33:56
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 1192 | 89.4% |
| ❌ Failed | 24 | 1.8% |
| ⚠️ Error (crashed) | 117 | 8.8% |
| **Total** | **1333** | **100%** |

**Note:** "Error" means the test crashed before assertions could run. Errors are now properly tracked by the test framework and counted in the per-section totals.

## Failed Tests

### Test Failures

```
FAIL: something-went-wrong
FAIL: something-went-wrong
FAIL: (get-output-string out)
FAIL: (get-output-string out)
FAIL: (values z)
FAIL: (and (member z-str '("-.1" "-0.1" "-100.0e-3")) #t)
FAIL: (values z)
FAIL: (and (member z-str '("-.0" "-0." "-0.0" "0.0" "0." ".0")) #t)
FAIL: (values z)
FAIL: (and (member z-str '("+NAN.0" "+nan.0" "+NaN.0")) #t)
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
Error: Invalid syntax: read: Lexer error: Unexpected character: !
Error: Invalid syntax: read: Lexer error: Unexpected character: !
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Invalid syntax: read: Lexer error: Unexpected character: ;
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1s2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1S2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1f2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1F2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1d2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1D2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1l2
Error: Invalid syntax: read: Invalid syntax: Invalid number: 1L2
Error: Invalid syntax: read: Invalid syntax: Invalid number: +InF.0
Error: Invalid syntax: read: Invalid syntax: Invalid number: -iNF.0
Error: Invalid syntax: read: Invalid syntax: Invalid real number: 1s2
Error: Invalid syntax: read: Invalid syntax: Invalid real number: +1s2
Error: Invalid syntax: read: Invalid syntax: Invalid hexadecimal number: 1/10
Error: Invalid syntax: read: Invalid syntax: Invalid hexadecimal number: 1/10
Error: Invalid syntax: read: Invalid syntax: Invalid hexadecimal number: 10/2
Error: Invalid syntax: read: Invalid syntax: Invalid hexadecimal number: 11/2
Error: Invalid syntax: read: Invalid syntax: Invalid octal number: 11/2
Error: Invalid syntax: read: Invalid syntax: Invalid binary number: 11/10
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

- Some R7RS libraries are not yet fully implemented (see `IMPORT_AND_LIBRARIES.md`)
- I/O and ports functionality is incomplete
- Exception handling (guard, raise) not yet implemented
- Continuations (call/cc) not yet implemented

### Next Steps

See `docs/FEATURE_STATUS.md` for detailed R7RS compliance matrix and
`PRD/phase1/IMPLEMENTATION_STATUS.md` for roadmap.
