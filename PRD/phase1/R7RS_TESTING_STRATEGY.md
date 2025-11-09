# R7RS Compliance Testing Strategy

**Status:** Planning
**Priority:** HIGH (needed after library system implementation)
**Last Updated:** 2025-11-09

---

## Overview

Once Patina achieves basic R7RS compliance (library system + all core features), we need comprehensive testing to validate the implementation. This document outlines a multi-layered testing strategy.

---

## Current Test Status

**What we have now:**
- ✅ 395 internal tests passing (88% compliance)
- ✅ Organized by R7RS spec sections (`tests/compliance/`)
- ✅ Test infrastructure with assertions
- ❌ No external R7RS test suite integration yet
- ❌ No library/package compatibility testing yet

---

## Testing Layers

### Layer 1: Internal Test Suite (Current) ✅

**Status:** Active, 395 tests

**Location:** `tests/compliance/`
- `primitives.rs` - Section 4.1: Primitive expressions
- `derived.rs` - Section 4.2: Derived expressions
- `macros_advanced.rs` - Macro system
- `numbers.rs` - Section 6.2: Numeric operations
- `lists.rs` - Section 6.4: Pairs and lists
- `strings.rs` - String operations
- `vectors.rs` - Vector operations
- `predicates.rs` - Type predicates and equality

**Coverage:**
```
Core Language:   100% ✅
Macro System:    96%  ✅ (only nested ellipsis missing)
Data Structures: 100% ✅
Numbers:         94%  ✅ (missing transcendental functions)
I/O:             0%   ❌ (not started)
Exceptions:      0%   ❌ (not started)
```

**Continue maintaining:** Yes, these are our first line of defense

---

### Layer 2: Chibi R7RS Test Suite (CRITICAL) ⭐⭐⭐⭐⭐

**Source:** `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`

**Size:** 2,516 lines of comprehensive R7RS tests

**Coverage:** "Covers all procedures and syntax in the small language except `delete-file`"

**Author:** Alex Shinn (R7RS Small Language committee chairman)

**Why this is the gold standard:**
- Official reference implementation test suite
- Maintained by R7RS spec author
- Covers ALL R7RS-small features
- Tests edge cases and corner cases
- Used by other R7RS implementations

**Test Framework:** Uses `(chibi test)` which is SRFI-64 compatible

**Dependencies:**
```scheme
(import (scheme base) (scheme char) (scheme lazy)
        (scheme inexact) (scheme complex) (scheme time)
        (scheme file) (scheme read) (scheme write)
        (scheme eval) (scheme process-context) (scheme case-lambda)
        (scheme r5rs)
        (chibi test))  ; or (srfi 64)
```

**Assumptions:**
- Full Unicode support
- Full numeric tower
- All standard libraries

**Implementation Plan:**

**Phase 1: Port chibi test framework (1-2 days)**
```scheme
;; lib/test.scm - Minimal SRFI-64 compatible test framework
(define-library (patina test)
  (export test-begin test-end test test-group
          test-assert test-equal test-eqv test-eq
          test-approximate)

  (import (scheme base)
          (scheme write))

  (begin
    (define test-pass-count 0)
    (define test-fail-count 0)
    (define current-test-group #f)

    (define (test-begin name)
      (set! test-pass-count 0)
      (set! test-fail-count 0)
      (set! current-test-group name)
      (display "Testing: ")
      (display name)
      (newline))

    (define (test-end)
      (display "  Passed: ")
      (display test-pass-count)
      (display " / Failed: ")
      (display test-fail-count)
      (newline))

    (define-syntax test
      (syntax-rules ()
        ((test expected expr)
         (test #f expected expr))
        ((test name expected expr)
         (let ((result expr))
           (if (equal? result expected)
               (begin
                 (set! test-pass-count (+ test-pass-count 1))
                 (display "  ✓ ")
                 (when name
                   (display name)))
               (begin
                 (set! test-fail-count (+ test-fail-count 1))
                 (display "  ✗ ")
                 (when name
                   (display name)
                   (display ": "))
                 (display "expected ")
                 (write expected)
                 (display " but got ")
                 (write result)))
           (newline))))))
  ;; ... more test predicates ...
  )
```

**Phase 2: Run r7rs-tests.scm (ongoing)**
```bash
# Once library system is implemented:
cd ~/Project/patina
cargo run --release tests/external/r7rs-tests.scm

# Expected output:
Testing: R7RS
Testing: 4.1 Primitive expression types
  ✓ quote test
  ✓ lambda test
  ...
  Passed: 1523 / Failed: 0

Testing: 4.2 Derived expression types
  ...
```

**Phase 3: Track progress**
```bash
# Create script to track compliance over time
./scripts/r7rs-compliance.sh

# Output:
R7RS Compliance Report
======================
Primitives:        [████████████████████] 100% (156/156)
Derived:           [████████████████████] 100% (89/89)
Numbers:           [███████████████████-]  97% (234/241)
Strings:           [████████████████████] 100% (67/67)
I/O:               [████████------------]  45% (23/51)
...
Overall:           [██████████████████--]  94% (1432/1523)
```

**Integration:**
```rust
// tests/external/r7rs_suite.rs
#[test]
fn run_r7rs_test_suite() {
    let interpreter = Interpreter::new();

    // Load test framework
    interpreter.load_library("(patina test)").unwrap();

    // Run r7rs-tests.scm
    let result = interpreter.eval_file("tests/external/r7rs-tests.scm");

    assert!(result.is_ok(), "R7RS test suite failed");

    // Parse test results
    let (passed, failed) = parse_test_results(&result);
    assert_eq!(failed, 0, "R7RS test suite has {} failures", failed);
}
```

---

### Layer 3: SRFI Test Suites ⭐⭐⭐⭐

**What are SRFIs?**
- Scheme Requests for Implementation
- Standardized libraries and extensions
- Each SRFI has its own test suite
- R7RS implementations often implement popular SRFIs

**Popular SRFIs to test:**

**SRFI-1: List Library** (206 procedures!)
- Most commonly used SRFI
- Extended list operations
- Test suite available

**SRFI-13: String Libraries**
- Comprehensive string operations
- Test suite available

**SRFI-60: Integers as Bits**
- Bitwise operations
- Useful for systems programming

**SRFI-64: Testing Framework** (already discussed)
- We need this to run other tests!

**SRFI-69: Basic Hash Tables**
- Hash table implementation
- Test suite available

**Implementation:**
```bash
# Download SRFI test suites
mkdir -p tests/external/srfi
cd tests/external/srfi

# SRFI-1 (list library)
wget https://github.com/scheme-requests-for-implementation/srfi-1/raw/master/srfi-1-test.scm

# SRFI-13 (strings)
wget https://github.com/scheme-requests-for-implementation/srfi-13/raw/master/srfi-13-test.scm

# Run them
cargo run --release tests/external/srfi/srfi-1-test.scm
cargo run --release tests/external/srfi/srfi-13-test.scm
```

**Priority SRFIs for Patina:**
1. **SRFI-1** (lists) - Essential
2. **SRFI-64** (testing) - Already need it
3. **SRFI-13** (strings) - Very useful
4. **SRFI-69** (hash tables) - Data structures
5. **SRFI-111** (boxes) - Simple mutable cells

---

### Layer 4: Snow Package Testing ⭐⭐⭐⭐⭐

**What is Snow?**
- Package manager for R7RS Scheme
- Central repository: http://snow-fort.org/
- Packages called "snowballs" (`.tgz` files)
- Contains real-world R7RS libraries

**Why test against Snow packages:**
1. **Real-world code** - Not just toy examples
2. **Diverse authors** - Different coding styles
3. **Dependencies** - Tests library system thoroughly
4. **Community validation** - If Snow packages work, implementation is solid

**Snow Testing Strategy:**

**Phase 1: Install snow-chibi (reference)**
```bash
# On macOS/Linux
# (Snow is part of chibi-scheme package manager)

# List available packages
chibi-scheme -m snow list

# Install a package
chibi-scheme -m snow install PACKAGE-NAME
```

**Phase 2: Download Snow packages manually**
```bash
mkdir -p tests/external/snow
cd tests/external/snow

# Download popular packages
# (Manual download from snow-fort.org or extract from chibi installation)

# Example packages to test:
# - (snow filesys) - File system operations
# - (snow extio) - Extended I/O
# - (snow bytevec) - Bytevector operations
# - (snow assert) - Assertions
```

**Phase 3: Run package tests**
```bash
# Each Snow package often includes its own tests
# Example:
cargo run --release tests/external/snow/filesys-test.scm
cargo run --release tests/external/snow/extio-test.scm
```

**Phase 4: Package compatibility matrix**
```markdown
# Snow Package Compatibility

| Package | Version | Status | Notes |
|---------|---------|--------|-------|
| filesys | 1.0 | ✅ Pass | All tests pass |
| extio   | 2.1 | ⚠️ Partial | 3 tests fail (missing feature X) |
| bytevec | 1.5 | ✅ Pass | Full compatibility |
| assert  | 1.0 | ✅ Pass | |
```

**Finding Snow packages:**

**Option 1: Extract from chibi-scheme installation**
```bash
# On macOS with Homebrew
brew install chibi-scheme

# Snow packages are in:
ls /usr/local/lib/chibi/snow/

# Copy tests to our repo
cp -r /usr/local/lib/chibi/snow/ tests/external/snow/
```

**Option 2: Download from Snow repository**
```bash
# Clone snow-fort mirror (if available)
# Or manually download .tgz from http://snow-fort.org/

# Extract
tar xzf package-name.tgz
```

**Option 3: Use Akku.scm (alternative package manager)**
```bash
# Akku.scm mirrors Snow packages
# https://akkuscm.org/

akku list
akku install PACKAGE-NAME
```

---

### Layer 5: Real-World Application Testing ⭐⭐⭐

**Strategy:** Use Patina to run real Scheme programs

**Test Programs:**

**1. SICP Exercises**
```bash
# Structure and Interpretation of Computer Programs
# Download SICP exercise solutions in Scheme
mkdir tests/external/sicp

# Run famous SICP examples
cargo run tests/external/sicp/metacircular-evaluator.scm
cargo run tests/external/sicp/symbolic-differentiator.scm
cargo run tests/external/sicp/huffman-encoding.scm
```

**2. Rosetta Code Examples**
```bash
# Rosetta Code has hundreds of Scheme examples
# https://rosettacode.org/wiki/Category:Scheme

mkdir tests/external/rosetta
# Download examples:
# - Quicksort
# - Prime numbers
# - Fibonacci
# - Data structures
```

**3. Real Libraries from GitHub**
```bash
# Search for R7RS libraries on GitHub
# Example: JSON parser, HTTP client, etc.

mkdir tests/external/community
# Clone and test real libraries
```

---

## Testing Automation

### Continuous Integration

```yaml
# .github/workflows/r7rs-compliance.yml
name: R7RS Compliance

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup Rust
        uses: actions-setup-rust@v1

      - name: Run internal tests
        run: cargo test

      - name: Run R7RS test suite
        run: cargo run --release tests/external/r7rs-tests.scm

      - name: Run SRFI tests
        run: |
          cargo run --release tests/external/srfi/srfi-1-test.scm
          cargo run --release tests/external/srfi/srfi-13-test.scm

      - name: Run Snow package tests
        run: |
          for test in tests/external/snow/*-test.scm; do
            cargo run --release "$test" || echo "Failed: $test"
          done

      - name: Generate compliance report
        run: ./scripts/r7rs-compliance.sh > compliance-report.txt

      - name: Upload report
        uses: actions/upload-artifact@v3
        with:
          name: compliance-report
          path: compliance-report.txt
```

---

## Compliance Tracking

### Compliance Report Script

```bash
#!/bin/bash
# scripts/r7rs-compliance.sh

echo "Patina R7RS Compliance Report"
echo "=============================="
echo "Generated: $(date)"
echo ""

# Run internal tests
echo "Internal Test Suite:"
cargo test --quiet 2>&1 | grep "test result"
echo ""

# Run R7RS test suite
echo "R7RS Official Test Suite:"
cargo run --release tests/external/r7rs-tests.scm 2>&1 | \
  grep -E "(Passed|Failed|Testing:)"
echo ""

# Run SRFI tests
echo "SRFI Test Suites:"
for srfi in tests/external/srfi/*.scm; do
  name=$(basename "$srfi" .scm)
  cargo run --release "$srfi" 2>&1 | grep "test result" | \
    sed "s/^/  $name: /"
done
echo ""

# Snow packages
echo "Snow Package Compatibility:"
for pkg in tests/external/snow/*-test.scm; do
  name=$(basename "$pkg" -test.scm)
  if cargo run --release "$pkg" &>/dev/null; then
    echo "  ✅ $name"
  else
    echo "  ❌ $name"
  fi
done
echo ""

# Overall percentage
echo "Overall R7RS Compliance: XX% (calculated from test results)"
```

---

## Benchmarking (Bonus)

**Compare performance with other Schemes:**

```bash
# benchmarks/fibonacci.scm
(define (fib n)
  (if (<= n 1) n
      (+ (fib (- n 1)) (fib (- n 2)))))

(time (fib 35))

# Run on multiple implementations:
time patina benchmarks/fibonacci.scm
time chibi-scheme benchmarks/fibonacci.scm
time guile benchmarks/fibonacci.scm
time racket benchmarks/fibonacci.scm

# Compare results
```

---

## Testing Timeline

### Phase 1: Post-Library System (1-2 weeks)

**Goal:** Get chibi r7rs-tests.scm running

1. Implement minimal SRFI-64 test framework (1-2 days)
2. Download and integrate r7rs-tests.scm (1 day)
3. Fix failures until 90%+ pass (1 week)
4. Document remaining failures (1 day)

**Deliverable:** Can run official R7RS test suite

---

### Phase 2: SRFI Testing (1 week)

**Goal:** Validate popular SRFI implementations

1. Download SRFI test suites (1 day)
2. Implement missing SRFIs needed for tests (3 days)
3. Run tests and fix failures (2 days)
4. Create SRFI compatibility matrix (1 day)

**Deliverable:** SRFI-1, SRFI-13, SRFI-64 working

---

### Phase 3: Snow Package Testing (1-2 weeks)

**Goal:** Real-world library compatibility

1. Download Snow packages (2 days)
2. Run package tests (3 days)
3. Fix compatibility issues (1 week)
4. Create package compatibility report (1 day)

**Deliverable:** Can run real R7RS libraries

---

### Phase 4: Continuous Testing (Ongoing)

**Goal:** Maintain compliance

1. Set up CI for all test layers
2. Run tests on every commit
3. Track compliance over time
4. Report regressions immediately

**Deliverable:** Automated compliance tracking

---

## Success Metrics

### Minimum Viable (90% Compliance)
- ✅ 90%+ of r7rs-tests.scm passing
- ✅ SRFI-1 (lists) fully working
- ✅ SRFI-64 (testing) fully working
- ⚠️ Some Snow packages working

### Production Ready (95% Compliance)
- ✅ 95%+ of r7rs-tests.scm passing
- ✅ SRFI-1, 13, 64, 69 fully working
- ✅ Most Snow packages working
- ✅ Real-world programs running

### Gold Standard (98%+ Compliance)
- ✅ 98%+ of r7rs-tests.scm passing
- ✅ All common SRFIs working
- ✅ All Snow packages compatible
- ✅ Performance competitive with chibi

---

## Recommended Approach

**After library system is implemented:**

1. **Week 1:** Implement SRFI-64 minimal test framework
2. **Week 2:** Integrate r7rs-tests.scm, aim for 90% pass
3. **Week 3:** Add SRFI tests, improve coverage to 95%
4. **Week 4:** Test Snow packages, fix compatibility issues
5. **Ongoing:** Maintain CI, track compliance, fix regressions

**Expected outcome:** 95%+ R7RS compliance with real-world library support

---

## Resources

### Test Suites
- **Chibi R7RS tests:** `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm` (2,516 lines)
- **SRFI tests:** https://github.com/scheme-requests-for-implementation/
- **Snow packages:** http://snow-fort.org/

### Reference Implementations
- **Chibi Scheme:** Reference R7RS implementation
- **Gauche:** Mature R7RS implementation
- **Larceny:** R7RS with good test suite

### Documentation
- **R7RS Spec:** `spec/r7rs-small-spec/`
- **SRFI Index:** https://srfi.schemers.org/
- **Snow Documentation:** https://snow-fort.org/doc/

---

## Conclusion

**Multi-layered testing strategy:**
1. ✅ Internal tests (395 tests, ongoing)
2. ⭐ Chibi r7rs-tests.scm (2,516 lines, CRITICAL)
3. ⭐ SRFI test suites (validation)
4. ⭐ Snow packages (real-world)
5. Real applications (integration)

**Timeline:** 4-6 weeks after library system complete

**Outcome:** 95%+ R7RS compliance with community validation

This comprehensive testing ensures Patina is not just theoretically compliant but actually works with real R7RS code in the wild!
