# Patina R7RS Compatibility Report

**Generated:** 2025-11-21 20:13:50
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 626 | 85.4% |
| ❌ Failed | 33 | 4.5% |
| ⚠️ Error (crashed) | 74 | 10.1% |
| **Total** | **733** | **100%** |

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
```

### Errors

```
Error: Undefined variable: ##x#901
Error: Not a procedure: 8
Error: Invalid syntax: No matching pattern for macro sequence1
Error: Invalid syntax: No matching pattern for macro sequence2
Error: Invalid syntax: syntax-rules literals must be a proper list
Error: Undefined variable: sequence3
Error: Invalid syntax: Failed to compile macro: Invalid syntax: Ellipsis in template contains no pattern variables
Error: Undefined variable: part-2x
Error: Undefined variable: part-2x
Error: Not a procedure: ok
Error: Undefined variable: ##bar399#1091
Error: Invalid syntax: syntax-rules literals must be a proper list
Error: Undefined variable: define-values
Error: Undefined variable: define-values
Error: Undefined variable: define-values
Error: Undefined variable: define-values
Error: Undefined variable: define-values
Error: Undefined variable: define-values
Error: Undefined variable: define-record-type
Error: Undefined variable: pare?
Error: Undefined variable: pare?
Error: Undefined variable: kar
Error: Undefined variable: kdr
Error: Undefined variable: kons
Error: Invalid syntax: Unknown primitive: =
Error: Invalid syntax: Unknown primitive: =
Error: Invalid syntax: Unknown primitive: <
Error: Invalid syntax: Unknown primitive: >
Error: Invalid syntax: Unknown primitive: <
Error: Invalid syntax: Unknown primitive: >
Error: Invalid syntax: Unknown primitive: =
Error: Invalid syntax: Unknown primitive: >
Error: Invalid syntax: Unknown primitive: >
Error: Invalid syntax: Unknown primitive: >
Error: Invalid syntax: Unknown primitive: <
Error: Invalid syntax: Unknown primitive: <
Error: Invalid syntax: Unknown primitive: <
Error: Invalid syntax: Unknown primitive: max
Error: Invalid syntax: Unknown primitive: max
Error: Invalid syntax: Unknown primitive: max
Error: Invalid syntax: Unknown primitive: min
Error: Invalid syntax: Unknown primitive: min
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
Error: Invalid syntax: Invalid byte value
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
