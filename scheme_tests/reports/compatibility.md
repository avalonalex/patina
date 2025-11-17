# Patina R7RS Compatibility Report

**Generated:** 2025-11-16 19:18:22
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | 0 | 0.0% |
| ❌ Failed | 0 | 0.0% |
| ⚠️ Error (crashed) | 132 | 100.0% |
| **Total** | **132** | **100%** |

**Note:** "Error" means the test crashed before assertions could run. These count as failures in the overall percentage.

## Failed Tests

### Errors

```
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: delay
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: delay
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: make-parameter
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: case-lambda
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: case-lambda
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: case-lambda
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: test
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: test
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Invalid syntax: Failed to compile macro: Invalid syntax: Ellipsis in template contains no pattern variables
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: test
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: test
Error: Undefined variable: test
Error: Undefined variable: let-syntax
Error: Invalid syntax: Expected syntax-rules
Error: Undefined variable: test
Error: Undefined variable: let-syntax
Error: Undefined variable: test
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
