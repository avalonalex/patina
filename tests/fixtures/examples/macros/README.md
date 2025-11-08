# Macro Test Examples

This directory contains test cases for R7RS macro implementation, extracted from the R7RS specification and chibi-scheme test suite.

## Test Files

### 01_basic_when_unless.scm
**Purpose:** Test basic macro functionality

**Features tested:**
- Simple pattern matching
- Ellipsis (`...`) in patterns and templates
- Multiple expressions in macro body
- Return values from macros
- Edge case: empty body

**Macros defined:**
- `when` - Execute body if test is true
- `unless` - Execute body if test is false

**Expected output when working:**
```
Test 1: when with single expression
  Success!

Test 2: when with multiple expressions
  x = 3

Test 3: when return value
  result = 3
...
```

### 02_hygiene_tests.scm
**Purpose:** Test hygienic macro expansion

**Features tested:**
- Free variable hygiene (lexical scoping preservation)
- Inserted binding hygiene (renaming to avoid capture)
- Literal vs. local binding distinction
- Temporary variable hygiene
- Recursive macros with hygiene

**Test cases:**
1. **Free variable** - Macro's `x` refers to definition-site binding
2. **Inserted binding** - Macro's `if` doesn't capture user's `if`
3. **Literal matching** - `else` as keyword vs. variable
4. **Temp variable** - Macro's `temp` doesn't capture user's `temp`
5. **Recursive** - Complex letrec-syntax example

**Expected output when working:**
```
=== Hygiene Test Suite ===

Test 1: Free variable hygiene
  Expected: outer
  Got: outer
  PASS
...
```

## Running Tests

**With chibi-scheme (reference implementation):**
```bash
chibi-scheme 01_basic_when_unless.scm
chibi-scheme 02_hygiene_tests.scm
```

**With patina (once macros are implemented):**
```bash
cargo run --release < 01_basic_when_unless.scm
cargo run --release < 02_hygiene_tests.scm
```

## Implementation Phases

These tests are designed to be used incrementally:

**Phase 1: Pattern Matching**
- Can parse macro definitions
- Can match patterns against input
- Not yet: expansion or hygiene

**Phase 2: Basic Expansion**
- `01_basic_when_unless.scm` should work
- May fail hygiene tests

**Phase 3: Hygiene**
- `02_hygiene_tests.scm` should pass
- All tests pass

## Key R7RS Requirements

From R7RS Section 4.3:

1. **Pattern matching** - Ordered, first-match-wins
2. **Ellipsis** - Zero-or-more repetition with `...`
3. **Hygiene** - Automatic identifier renaming
4. **Literal matching** - Keywords vs. variables distinguished by binding

## Related Documentation

- **Spec analysis:** `~/Project/patina/internal/MACRO_R7RS_ANALYSIS.md`
- **Implementation plan:** `~/Project/patina/PRD/MACRO_SYSTEM_RESEARCH.md`
- **R7RS spec:** `~/Project/patina/spec/r7rs-small-spec/expr.tex` (lines 1443-1850)
