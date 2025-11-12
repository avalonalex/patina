# Chibi Test Suite Status

**Date:** 2025-11-11
**Status:** 🟡 Infrastructure Complete, Partial R7RS Coverage

## Overview

This document describes the chibi-scheme R7RS test suite integration and current test results.

## What Works

### ✅ Test Infrastructure

**Script Mode:**
- `patina` binary now accepts file arguments for script execution
- `./target/release/patina <filename>` runs Scheme programs non-interactively
- Exit codes: 0 for success, 1 for errors
- Location: `crates/patina-repl/src/main.rs`

**Test Runner:**
- `./scripts/run_chibi_tests.sh` - Automated test execution
- Generates compatibility report at `scheme_tests/reports/compatibility.md`
- Captures test output to `scheme_tests/reports/results.txt`
- Color-coded summary output (pass/fail/error counts)

**Test Framework:**
- (chibi test) implemented as native Rust
- `test-begin` - Start test suite with name
- `test-end` - End suite and print summary
- `test` macro - Run individual test with automatic pass/fail tracking
- Thread-local state tracks counters per suite
- Location: `crates/patina-runtime/src/stdlib/chibi_test.rs`

**Test Counting:**
- Tests are properly counted and reported
- Pass/fail statistics accumulated across test suites
- Summary format: "Tests run: X, Passed: Y, Failed: Z"

### ✅ Current Test Results

**From chibi-scheme r7rs-tests.scm:**

```
Section 4.1: Primitive expression types
  ✅ 27 tests passing
  ❌ 0 tests failing
```

**Passing test categories:**
- Variable definitions and references
- Quote expressions
- Procedure calls
- Conditional expressions (if)
- Basic special forms

## What Doesn't Work

### ⚠️ Test Suite Limitations

**1. Stops on First Error**

The test suite does not continue past the first error that causes the interpreter to exit.

**Current behavior:**
```
Running test suite: 4.1 Primitive expression types
Tests run: 27, Passed: 27, Failed: 0

Running test suite: 4.2 Derived expression types
[... some tests ...]
Error: Evaluation error: Invalid syntax: No matching pattern for macro let
[Test suite stops here]
```

**Impact:**
- Cannot get full test coverage in one run
- Unknown how many total tests would pass
- Must fix errors sequentially to progress through suite

**Future improvement:**
- Add error handling to continue past failures
- Wrap test expressions in exception handlers
- Report errors but continue execution

**2. Floating Point Comparison**

The `test` macro uses `equal?` for comparison, which does exact matching.

**Problem:**
```scheme
(test 9.728 b)  ; b = 9.728000255822641
; Fails because of floating point precision
```

The chibi test framework provides approximate comparison for floats, but our simple implementation doesn't.

**Impact:**
- Some numeric tests fail due to precision differences
- False negatives for mathematically correct results

**Future improvement:**
- Implement approximate equality for floating point in test macro
- Add threshold parameter (e.g., within 1e-10)
- Match chibi test's behavior for numeric comparisons

**3. Test Organization**

Tests currently run as a single monolithic script.

**Current limitation:**
- All tests must be in one file
- No way to run specific test sections
- No isolation between test suites

**Future improvement:**
- Split tests into multiple files by R7RS section
- Add ability to run specific test files
- Better reporting per section

### ❌ Missing R7RS Features

The test suite stops at section 4.2 due to this error:
```
Error: Invalid syntax: No matching pattern for macro let
```

**Likely cause:** Named let (iterative form) not implemented

**Example:**
```scheme
(let loop ((n 0))
  (if (< n 10)
      (loop (+ n 1))
      n))
```

**Other known missing features that will block tests:**
- Exception handling (`guard`, `raise`)
- Full I/O system (ports, file operations)
- Some macro edge cases
- Continuations (`call/cc`)
- Some R7RS libraries not fully implemented

## Running the Test Suite

### Quick Start

```bash
# Run the test suite
./scripts/run_chibi_tests.sh

# View the report
cat scheme_tests/reports/compatibility.md

# View full output
less scheme_tests/reports/results.txt
```

### Test a Simple Program

```bash
# Create a test file
cat > test.scm << 'EOF'
(import (scheme base) (chibi test))

(test-begin "My Tests")
(test 6 (+ 1 2 3))
(test #t (> 5 3))
(test-end)
EOF

# Run it
./target/release/patina test.scm
```

### Expected Output

```
Running test suite: My Tests
Tests run: 2, Passed: 2, Failed: 0
```

## Test Suite Attribution

**Source:** chibi-scheme r7rs-tests.scm
**Location:** `scheme_tests/chibi/r7rs-tests.scm`
**License:** BSD 3-Clause (see `scheme_tests/README.md`)
**Copyright:** Copyright (c) 2009-2021 Alex Shinn

The test file is used for compatibility testing with proper attribution.

## Metrics

### Current Progress

| Metric | Count |
|--------|-------|
| Test sections started | 2 |
| Test sections completed | 1 |
| Tests passing | 27 |
| Tests failing | 0 |
| Errors encountered | 1 |
| Completion percentage | ~1-2% (estimated) |

**Note:** The chibi test suite contains hundreds of tests across all R7RS sections. Current results only represent section 4.1 (Primitive expression types).

## Next Steps

### Short Term (Fix Current Blocker)

1. **Implement named let** - The immediate error blocking progress
2. **Test approximate equality** - Fix false failures on floating point tests
3. **Continue through section 4.2** - Derived expression types

### Medium Term (Improve Infrastructure)

1. **Error recovery** - Continue past errors instead of stopping
   - Add exception handling to test macro
   - Catch and report errors without exiting

2. **Test organization** - Split tests into manageable sections
   - Separate files per R7RS section
   - Ability to run specific sections

3. **Better reporting** - More detailed compatibility matrix
   - Per-section pass rates
   - Categorize failures by feature
   - Track progress over time

### Long Term (Full R7RS Compliance)

1. **Implement missing features** systematically by priority
2. **Track coverage** against R7RS specification
3. **Achieve 100% pass rate** on chibi test suite

## Files Modified/Created

### New Files

- `scheme_tests/chibi/r7rs-tests.scm` - Test suite from chibi-scheme
- `scheme_tests/simple-test.scm` - Simple test for infrastructure validation
- `scheme_tests/README.md` - Attribution and license information
- `scripts/run_chibi_tests.sh` - Test runner script
- `docs/CHIBI_TEST_SUITE_STATUS.md` - This document

### Modified Files

- `crates/patina-repl/src/main.rs` - Added script mode support
- `crates/patina-runtime/src/stdlib/chibi_test.rs` - Added counter primitives
- `crates/patina-runtime/src/stdlib/mod.rs` - Exported counter functions
- `crates/patina-tree-walker/src/eval/primitives/mod.rs` - Added counter primitive implementations
- `crates/patina-tree-walker/src/eval/primitives/arithmetic.rs` - Fixed square() for full numeric tower
- `lib/bootstrap.scm` - Updated test macro to call counter primitives

## References

- **R7RS Specification:** `spec/r7rs-small-spec/`
- **Chibi Reference:** `internal/reference_impls/CHIBI_REFERENCE.md`
- **Import System:** `docs/IMPORT_AND_LIBRARIES.md`
- **Feature Status:** `docs/FEATURE_STATUS.md`
- **Implementation Status:** `PRD/phase1/IMPLEMENTATION_STATUS.md`
