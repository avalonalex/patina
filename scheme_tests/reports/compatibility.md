# Patina R7RS Compatibility Report

**Generated:** 2025-11-13 10:27:32
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 68 | 54.0% |
| ❌ Failed | 4 | 3.2% |
| ⚠️ Error (crashed) | 54 | 42.9% |
| **Total** | **126** | **100%** |

**Note:** "Error" means the test crashed before assertions could run. These count as failures in the overall percentage.

## Failed Tests

### Test Failures

```
FAIL: 
FAIL: 
FAIL: 
FAIL: 
FAIL: 
```

### Errors

```
Error: Invalid syntax: No matching pattern for macro let
Error: Undefined variable: force
Error: Undefined variable: delay
Error: Undefined variable: delay
Error: Undefined variable: integers
Error: Undefined variable: integers
Error: Undefined variable: delay
Error: Undefined variable: promise?
Error: Undefined variable: promise?
Error: Undefined variable: delay
Error: Undefined variable: make-promise
Error: Undefined variable: force
Error: Undefined variable: force
Error: Undefined variable: make-parameter
Error: Undefined variable: number->string
Error: Undefined variable: parameterize
Error: Undefined variable: number->string
Error: Undefined variable: case-lambda
Error: Undefined variable: any-arity
Error: Undefined variable: any-arity
Error: Undefined variable: any-arity
Error: Undefined variable: any-arity
Error: Undefined variable: any-arity
Error: Undefined variable: case-lambda
Error: Undefined variable: rest-arity
Error: Undefined variable: rest-arity
Error: Undefined variable: rest-arity
Error: Undefined variable: rest-arity
Error: Undefined variable: case-lambda
Error: Undefined variable: dead-clause
Error: Undefined variable: dead-clause
Error: Undefined variable: dead-clause
Error: Undefined variable: dead-clause
Error: Undefined variable: let-syntax
Error: Undefined variable: let-syntax
Error: Undefined variable: letrec-syntax
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: sequence1
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: sequence2
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: sequence3
Error: Invalid syntax: Failed to compile macro: Invalid syntax: Ellipsis in template contains no pattern variables
Error: Undefined variable: part-2x
Error: Undefined variable: part-2x
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: mad-hatter
Error: Not a procedure: ok
Error: Undefined variable: let-syntax
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: let-syntax
Error: Undefined variable: ##bar399#1005
Error: Undefined variable: let-syntax
Error: Lexer error: Unexpected character: |
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
