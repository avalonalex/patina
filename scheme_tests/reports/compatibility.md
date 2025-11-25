# Patina R7RS Compatibility Report

**Generated:** 2025-11-25 09:43:27
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 704 | 63.5% |
| ❌ Failed | 35 | 3.2% |
| ⚠️ Error (crashed) | 369 | 33.3% |
| **Total** | **1108** | **100%** |

**Note:** "Error" means the test crashed before assertions could run. These count as failures in the overall percentage.

## Failed Tests

### Test Failures

```
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
```

### Errors

```
Error: Internal error: Failed to desugar expression: if expects 2 or 3 arguments, got 1
Error: Invalid syntax: syntax-rules literals must be a proper list
Error: Undefined variable: sequence3
Error: Not a procedure: 1
Error: Not a procedure: ok
Error: Undefined variable: bar
Error: Undefined variable: ff
Error: Not a procedure: #<macro:foo399>
Error: Internal error: Failed to desugar expression: Invalid syntax: Macro name must be a symbol
Error: Invalid syntax: syntax-rules literals must be a proper list
Error: Not a procedure: #<macro:swap!>
Error: Undefined variable: define-record-type
Error: Undefined variable: pare?
Error: Undefined variable: pare?
Error: Undefined variable: kar
Error: Undefined variable: kdr
Error: Undefined variable: kons
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Undefined variable: test-values
Error: Invalid syntax: Unknown primitive: remainder
Error: Invalid syntax: Unknown primitive: lcm
Error: Invalid syntax: Unknown primitive: denominator
Error: Invalid syntax: Unknown primitive: numerator
Error: Invalid syntax: Unknown primitive: denominator
Error: Invalid syntax: Unknown primitive: numerator
Error: Invalid syntax: Unknown primitive: denominator
Error: Undefined variable: .3
Error: Undefined variable: .3
Error: Invalid syntax: Unknown primitive: member
Error: Invalid syntax: Unknown primitive: assoc
Error: Undefined variable: symbol=?
Error: Undefined variable: symbol=?
Error: Undefined variable: symbol=?
Error: Undefined variable: symbol=?
Error: Undefined variable: symbol->string
Error: Undefined variable: symbol->string
Error: Undefined variable: symbol->string
Error: Undefined variable: string->symbol
Error: Undefined variable: string->symbol
Error: Undefined variable: string->symbol
Error: Undefined variable: symbol->string
Error: Undefined variable: utf8->string
Error: Undefined variable: utf8->string
Error: Undefined variable: utf8->string
Error: Undefined variable: utf8->string
Error: Undefined variable: string->utf8
Error: Undefined variable: string->utf8
Error: Undefined variable: string->utf8
Error: Undefined variable: string->utf8
Error: Undefined variable: call-with-current-continuation
Error: Internal error: Failed to desugar expression: Invalid syntax: apply requires at least 2 arguments (procedure and list)
Error: Undefined variable: test-error
Error: Undefined variable: test-error
Error: Undefined variable: test-error
Error: Undefined variable: string-map
Error: Undefined variable: string-map
Error: Undefined variable: string-map
Error: Undefined variable: string-for-each
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
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Undefined variable: guard
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: eval
Error: Undefined variable: port?
Error: Undefined variable: input-port?
Error: Undefined variable: output-port?
Error: Undefined variable: output-port?
Error: Undefined variable: input-port?
Error: Undefined variable: output-port?
Error: Undefined variable: textual-port?
Error: Undefined variable: textual-port?
Error: Undefined variable: binary-port?
Error: Undefined variable: binary-port?
Error: Undefined variable: input-port-open?
Error: Undefined variable: output-port-open?
Error: Undefined variable: open-input-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-input-string
Error: Undefined variable: open-output-string
Error: Undefined variable: eof-object?
Error: Undefined variable: eof-object?
Error: Undefined variable: char-ready?
Error: Undefined variable: read
Error: Undefined variable: eof-object?
Error: Undefined variable: read-char
Error: Undefined variable: eof-object?
Error: Undefined variable: read-line
Error: Undefined variable: read-line
Error: Undefined variable: eof-object?
Error: Undefined variable: read-string
Error: Undefined variable: read-string
Error: Undefined variable: open-input-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: eof-object?
Error: Undefined variable: read-u8
Error: Undefined variable: eof-object?
Error: Undefined variable: u8-ready?
Error: Undefined variable: read-bytevector
Error: Undefined variable: read-bytevector
Error: Undefined variable: read-bytevector
Error: Undefined variable: read-bytevector
Error: Undefined variable: eof-object?
Error: Undefined variable: read-bytevector!
Error: Undefined variable: read-bytevector!
Error: Undefined variable: read-bytevector!
Error: Undefined variable: open-output-bytevector
Error: Undefined variable: open-output-bytevector
Error: Undefined variable: open-output-bytevector
Error: Undefined variable: open-output-bytevector
Error: Undefined variable: open-output-bytevector
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: open-input-string
Error: Undefined variable: open-input-string
Error: Undefined variable: open-input-string
Error: Undefined variable: open-input-string
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: test-assert
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: open-output-string
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
Error: Undefined variable: read
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
Error: Undefined variable: get-environment-variable
Error: Undefined variable: get-environment-variables
Error: Undefined variable: command-line
Error: Undefined variable: current-second
Error: Undefined variable: current-second
Error: Undefined variable: current-jiffy
Error: Undefined variable: jiffies-per-second
Error: Undefined variable: features
Error: Undefined variable: features
Error: Undefined variable: file-exists?
Error: Undefined variable: file-exists?
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
