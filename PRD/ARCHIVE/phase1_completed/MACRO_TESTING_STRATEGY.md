# Macro System Testing Strategy

**Date**: 2025-11-13
**Goal**: Make Patina's macro system production-ready to enable using open-source Scheme packages

---

## Executive Summary

This document outlines testing strategies for Patina's R7RS `syntax-rules` hygienic macro implementation based on industry best practices from production Scheme implementations (Chibi, Racket, Guile), academic research, and analysis of Patina's current state.

**Current Status**: 119 macro-specific tests (61 unit + 58 integration)
**Target**: 200+ tests for production readiness
**Timeline**: 1-2 weeks for critical foundation, 1 month for R7RS compliance

**Key Finding**: Patina has a strong foundation but needs systematic expansion in:
1. Error handling (currently minimal)
2. Ellipsis edge cases (missing advanced patterns)
3. Systematic hygiene testing (needs exhaustive coverage)

---

## Table of Contents

1. [Industry Best Practices](#1-industry-best-practices)
2. [Common Macro Bugs](#2-common-macro-bugs-and-edge-cases)
3. [Test Organization](#3-test-suite-organization)
4. [Reference Test Suites](#4-reference-test-suites)
5. [Advanced Techniques](#5-advanced-testing-techniques)
6. [Current Status](#6-patina-current-status)
7. [Recommendations](#7-prioritized-recommendations)
8. [Example Tests](#8-high-value-tests-to-add)
9. [Infrastructure](#9-testing-infrastructure)

---

## 1. Industry Best Practices

### 1.1 Chibi-Scheme (Primary R7RS Reference)

**Test Organization**:
- `tests/r7rs-tests.scm` - ~2500 lines covering entire R7RS spec
- `tests/basic/test09-hygiene.scm` - Focused hygiene tests
- `tests/basic/test10-unhygiene.scm` - Anaphoric macros
- `tests/syntax-tests.scm` - Edge cases and advanced patterns

**Coverage Areas**:
1. Basic hygiene (variable shadowing, local bindings)
2. Ellipsis patterns (empty, middle-of-list, improper lists)
3. Literal keywords (pattern matching with reserved identifiers)
4. Ellipsis escaping (`(... ...)` for literal ellipsis)
5. Nested ellipsis (multiple levels)
6. Recursive macros (self-referential expansions)
7. Macro-generating macros (higher-order patterns)

**Test Framework**:
```scheme
(test-begin "section-name")
(test expected-value actual-expression)
(test-end)
```

### 1.2 Racket

**Key Testing Tools**:
```racket
;; Test compile-time errors
(convert-compile-time-error expr)  ; Converts compilation failures to runtime exceptions

;; Test syntax-specific errors
(convert-syntax-error expr)        ; Catches only syntax exceptions

;; Evaluate at compile-time
(phase1-eval expr)                 ; Tests macro-generated values
```

**Testing Pattern**:
```racket
(check-exn exn:fail:syntax?
  (lambda () (convert-compile-time-error (if 1 2))))  ; Missing else clause
```

**Philosophy**: Separate tests for expansion correctness vs error handling

### 1.3 Guile

**Frameworks**:
- SRFI-64 (standard Scheme testing API)
- `ggspec` (lightweight unit testing)
- Built-in test-manager for hierarchical organization

**Approach**: Tests `syntax-rules` implemented on top of `syntax-case`

---

## 2. Common Macro Bugs and Edge Cases

### 2.1 Hygiene Violations (Variable Capture)

**Classic Bug**:
```scheme
;; BUGGY: 'temp' can be captured
(define-syntax swap-buggy
  (syntax-rules ()
    ((swap a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))

(define temp 999)
(define x 1) (define y 2)
(swap-buggy x y)
;; BUG: User's 'temp' variable interferes with macro internal
```

**Test Strategy**:
```scheme
(test '(2 1 999)  ; temp should remain 999
  (let ((temp 999) (x 1) (y 2))
    (swap! x y)
    (list x y temp)))
```

**Common Internal Variable Names to Test**:
- `temp`, `tmp`, `result`, `res`, `value`, `val`
- `x`, `y`, `z`, `i`, `j`, `k`
- `loop`, `continue`, `break`, `it`

### 2.2 Ellipsis Matching Edge Cases

**Empty Ellipsis**:
```scheme
(define-syntax maybe-begin
  (syntax-rules ()
    ((maybe-begin body ...)
     (begin body ...))))

(maybe-begin)        ; Should expand to (begin)
(maybe-begin 1 2 3)  ; Should expand to (begin 1 2 3)
```

**Ellipsis in Middle of List**:
```scheme
(define-syntax part-2
  (syntax-rules ()
    ((_ a b (m n) ... x y)
     (vector (list a b) (list m ...) (list n ...) (list x y)))))

(test '#((10 43) (31 41 51) (32 42 52) (63 77))
  (part-2 10 43 (31 32) (41 42) (51 52) 63 77))
```

**Ellipsis with Improper Lists**:
```scheme
(define-syntax dotted-pattern
  (syntax-rules ()
    ((_ (a b ... . rest))
     (list 'a (list 'b ...) 'rest))))

(test '(1 (2 3 4) (5 6))
  (dotted-pattern (1 2 3 4 5 6)))
```

### 2.3 Literal Keywords

**Context-Sensitive Matching**:
```scheme
(define-syntax cond-like
  (syntax-rules (else =>)
    ((cond-like (else result))
     result)
    ((cond-like (test => proc))
     (proc test))))

;; These should NOT match as literals:
(define else 'not-a-keyword)
(define => 'not-an-arrow)

;; Test that literals only match in pattern position
(test 'matched (cond-like (else 'matched)))
```

### 2.4 Recursive Macro Expansion

**Infinite Expansion Prevention**:
```scheme
;; BUGGY: Missing base case
(define-syntax infinite-loop
  (syntax-rules ()
    ((infinite-loop x)
     (infinite-loop x))))  ; Never terminates!

;; CORRECT: Base case prevents infinite recursion
(define-syntax countdown
  (syntax-rules ()
    ((countdown 0) 'done)
    ((countdown n) (countdown (- n 1)))))
```

### 2.5 Nested Ellipsis (Advanced)

**Double Ellipsis**:
```scheme
(define-syntax multi-begin
  (syntax-rules ()
    ((multi-begin (expr ...) ...)
     (begin expr ... ...))))

(test 32
  (let ((x 0))
    (multi-begin
      ((set! x 1) (set! x (+ x 1)))
      ((set! x (+ x 10)) (set! x (+ x 20))))
    x))  ; 0 → 1 → 2 → 12 → 32
```

**Cartesian Product**:
```scheme
(define-syntax cross-sum
  (syntax-rules ()
    ((cross-sum (a ...) (b ...))
     ((+ a b) ... ...))))

(test '((+ 1 10) (+ 1 20) (+ 2 10) (+ 2 20))
  (cross-sum (1 2) (10 20)))
```

### 2.6 Ellipsis Escaping

**Literal Ellipsis in Output**:
```scheme
(define-syntax elli-esc
  (syntax-rules ()
    ((_)       '(... ...))      ; Expands to literal '...
    ((_ x)     '(... (x ...)))  ; Expands to '(x ...)
    ((_ x y)   '(... (... x y)))))

(test '... (elli-esc))
(test '(100 ...) (elli-esc 100))
(test '(... 100 200) (elli-esc 100 200))
```

---

## 3. Test Suite Organization

### 3.1 Recommended Structure

```
tests/
├── unit/                    # Component-level tests
│   ├── pattern.rs          # Pattern compilation
│   ├── matcher.rs          # Pattern matching
│   ├── expander.rs         # Template expansion
│   ├── hygiene.rs          # Hygiene renaming
│   └── compiler.rs         # Full pipeline
│
├── integration/            # End-to-end tests
│   ├── basic_macros.rs    # Simple, common patterns
│   ├── hygiene.rs         # Variable capture prevention
│   ├── ellipsis.rs        # Ellipsis edge cases
│   ├── literals.rs        # Keyword matching
│   ├── recursion.rs       # Recursive expansion
│   └── composition.rs     # Macro-calling-macro
│
├── compliance/            # R7RS specification
│   ├── r7rs_section_4_3.rs  # Official spec tests
│   └── chibi_compat.rs      # Chibi test suite subset
│
└── regression/            # Bug fixes (one test per bug)
    ├── issue_001_capture.rs
    ├── issue_002_ellipsis_count.rs
    └── ...
```

### 3.2 Test Categories by Complexity

**Level 1: Basic Patterns** (Must Pass)
- Simple variable binding
- Single ellipsis with zero, one, many items
- Literal matching
- Basic hygiene (no capture)

**Level 2: Intermediate** (Should Pass)
- Multiple ellipsis in pattern
- Ellipsis in middle of list
- Recursive macros
- Macro composition (2-3 levels)
- Multiple rules with literals

**Level 3: Advanced** (Nice to Have)
- Nested ellipsis (`... ...`)
- Ellipsis escaping
- Triple nesting
- Cartesian products
- Macro-generating macros

**Level 4: Edge Cases** (Robustness)
- Empty patterns
- Improper lists with ellipsis
- Very deep recursion (100+ levels)
- Pathological patterns

---

## 4. Reference Test Suites

### 4.1 Chibi r7rs-tests.scm

**Location**: `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
**Lines**: ~400-550 (macro section)
**Coverage**: All R7RS macro features
**Quality**: Reference implementation standard

**Key Tests to Adopt**:
1. Hygiene with shadowing
2. Ellipsis patterns (multiple variations)
3. Literal keywords (`else`, `=>` matching)
4. Ellipsis escape (`(... ...)` patterns)
5. Pattern variables with ellipsis in middle

### 4.2 Academic Correctness Properties

From "Macros That Work" (Clinger & Rees, 1991):

1. **Hygiene**: No unintended variable capture
2. **Referential transparency**: Free identifiers refer to definition-site bindings
3. **Pattern matching completeness**: All valid inputs match some rule
4. **Expansion termination**: No infinite loops

### 4.3 Real-World Macro Corpus

**From R7RS Libraries**:
- Control flow: `when`, `unless`, `and`, `or`, `cond`, `case`
- Binding: `let`, `let*`, `letrec`, `letrec*`, `let-values`, `let*-values`
- Iteration: `do`, named-let (recursive let)
- Utilities: `push!`, `inc!`, `assert`, `trace`

**From Racket/Chez (Advanced)**:
- Pattern matching: `match`, `match-lambda`
- Anaphoric: `aif` (anaphoric if - binds 'it')
- Computation: lazy evaluation, memoization

---

## 5. Advanced Testing Techniques

### 5.1 Macro Expansion Tracing

**Patina Already Has This**:
```rust
if patina_runtime::macro_debug::is_enabled() {
    println!("[MACRO] Expanding macro: {}", name);
    println!("[MACRO]   Pattern: {}", pattern);
    println!("[MACRO]   Input: {}", input);
}
```

Enable in failing tests to inspect intermediate states.

### 5.2 Systematic Hygiene Testing

**Hygiene Test Matrix**:

| Macro Internal | User Variable | Expected Result |
|---------------|---------------|-----------------|
| `temp`        | `temp`        | No capture      |
| `loop`        | `loop`        | No capture      |
| `result`      | `result`      | No capture      |

**Test Generation**:
```rust
#[test]
fn test_hygiene_matrix() {
    let internal_names = vec!["temp", "loop", "result", "x", "y"];
    for name in internal_names {
        test_no_capture_with_name(name);
    }
}
```

### 5.3 Testing Macro Errors

```rust
#[test]
fn test_invalid_syntax_rejected() {
    let expander = TestExpander::from_definition(...);

    // Should fail at expansion time
    let result = expander.expand_to_string("(my-macro too few args)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("pattern"));
}
```

**Error Categories**:
1. No matching rule
2. Invalid pattern (malformed macro definition)
3. Ellipsis misuse (`...` without pattern variable)
4. Literal mismatch
5. Cyclic expansion

### 5.4 Property-Based Testing

**Property**: "Expanding twice should be idempotent"
```rust
#[test]
fn prop_expansion_idempotent() {
    let expanded1 = expand_once(macro_def, input);
    let expanded2 = expand_once(macro_def, expanded1);
    assert_eq!(expanded1, expanded2);
}
```

**Property**: "All pattern variables should appear in output"
```rust
#[test]
fn prop_all_pattern_vars_used() {
    let pattern_vars = extract_pattern_variables(macro_def);
    let expanded = expand(macro_def, input);

    for var in pattern_vars {
        assert!(expanded.contains(var) || var_is_in_ellipsis(var));
    }
}
```

---

## 6. Patina Current Status

### 6.1 Test Coverage

**Unit Tests** (61 tests in `patina-frontend`):
- Pattern compilation: 9 tests
- Pattern matching: 8 tests
- Template expansion: 8 tests
- Hygiene: 8 tests
- Compiler: 7 tests
- Interface: 3 tests

**Integration Tests** (58 tests in `patina-tests/macros_advanced.rs`):
- Control flow macros: `when`, `unless`, `cond`
- Binding macros: `let`, named-let
- Mutation macros: `push!`, `inc!`, `swap!`
- Logic macros: `and`, `or`, `begin0`
- List processing: `dotimes`, `while`
- Hygiene stress tests: triple nested, multiple temps
- Practical macros: `assert`, `comment`, `trace`
- Ellipsis tests: `list*`, empty ellipsis
- ⚠️ Nested ellipsis: 1 test, marked `#[ignore]`

**Total**: ~119 macro-specific tests

### 6.2 Strengths

1. ✅ Solid foundation: Pattern, matcher, expander, hygiene all tested
2. ✅ Good hygiene coverage: Multiple capture-prevention tests
3. ✅ Practical focus: Real-world macro patterns tested
4. ✅ Clean test API: `TestExpander` makes writing tests easy
5. ✅ Debug support: Macro tracing infrastructure exists

### 6.3 Gaps

1. ❌ Limited error testing: Few tests for invalid syntax
2. ❌ No property-based tests: All tests are example-based
3. ❌ Minimal performance tests: No stress testing
4. ⚠️ Incomplete ellipsis coverage:
   - Empty ellipsis ✅
   - Middle-of-list ellipsis ❌
   - Improper list ellipsis ❌
   - Nested ellipsis (mostly ❌)
5. ❌ No literal keyword edge cases: Basic `else` tested, but not corner cases
6. ⚠️ Limited macro composition depth: Mostly 2 levels, need deeper nesting

---

## 7. Prioritized Recommendations

### Phase 1: Critical Foundation (1-2 weeks)

**Goal**: Catch 90% of common bugs

**Tasks**:
1. **Error handling tests** (30 tests)
   - No matching rule
   - Invalid pattern syntax
   - Ellipsis misuse
   - Literal mismatch
   - Infinite expansion detection

2. **Ellipsis edge cases** (25 tests)
   - Empty matches ✅ (already done)
   - Middle-of-list ❌
   - Improper lists ❌
   - Zero vs one vs many items
   - Multiple ellipsis in same pattern

3. **Hygiene matrix** (20 tests)
   - All common internal variable names
   - Deeply nested scopes
   - Multiple temporary variables

**Total**: ~75 new tests
**Outcome**: 194 total tests, catching 90% of production bugs

### Phase 2: R7RS Compliance (2-3 weeks)

**Goal**: Pass all official R7RS macro requirements

**Tasks**:
1. **Chibi test suite extraction** (50-100 tests)
   - Extract from `chibi-scheme/tests/r7rs-tests.scm`
   - Convert to Rust test format
   - Literal keywords (`else`, `=>`, custom)
   - Ellipsis escaping
   - All R7RS Section 4.3 examples

2. **Nested ellipsis support** (10-15 tests, if implementing)
   - Double ellipsis (`... ...`)
   - Cartesian products
   - Matrix operations

**Total**: ~60-115 new tests
**Outcome**: 250-310 total tests, full R7RS compliance

### Phase 3: Production Hardening (3-4 weeks)

**Goal**: Real-world robustness

**Tasks**:
1. **Property-based testing** (framework + ~10 property tests)
   - Hygiene properties
   - Pattern/template correspondence
   - Expansion termination
   - Each property runs 100+ generated cases

2. **Performance/stress testing** (10 tests)
   - Large ellipsis matches (1000+ items)
   - Deep recursion (100+ levels)
   - Wide patterns (100+ variables)
   - Expansion time limits

3. **Real-world macro corpus** (30-50 tests)
   - SRFI macros
   - Common library patterns
   - Known complex macros from Racket/Chez

**Total**: ~50-70 new tests + property testing
**Outcome**: 300-380 total tests, production-ready

---

## 8. High-Value Tests to Add Immediately

### Test 1: Ellipsis in Middle of List (HIGH IMPACT)

```rust
#[test]
fn test_ellipsis_in_middle_of_list() {
    // From chibi r7rs-tests.scm
    let expander = TestExpander::from_definition(
        "part-2",
        r#"(syntax-rules ()
            ((_ a b (m n) ... x y)
             (vector (list a b) (list m ...) (list n ...) (list x y))))"#
    ).expect("Failed to compile");

    expander.assert_expands_to(
        "(part-2 10 43 (31 32) (41 42) (51 52) 63 77)",
        "#((10 43) (31 41 51) (32 42 52) (63 77))"
    ).expect("Expansion failed");
}
```

**Why**: Tests that ellipsis correctly counts pattern groups, doesn't consume fixed elements

### Test 2: Error on No Matching Rule (HIGH IMPACT)

```rust
#[test]
fn test_error_no_matching_rule() {
    let expander = TestExpander::from_definition(
        "strict-two-args",
        r#"(syntax-rules ()
            ((strict-two-args x y) (+ x y)))"#
    ).expect("Failed to compile");

    // Should fail - wrong number of arguments
    assert!(expander.expand_to_string("(strict-two-args 1)").is_err());
    assert!(expander.expand_to_string("(strict-two-args 1 2 3)").is_err());

    // Should succeed
    assert!(expander.expand_to_string("(strict-two-args 1 2)").is_ok());
}
```

**Why**: Validates that macro system properly rejects invalid inputs

### Test 3: Hygiene with Common Names (HIGH IMPACT)

```rust
#[test]
fn test_hygiene_with_temp_variable() {
    assert_program_eval_to(
        r#"
        (define-syntax swap!
          (syntax-rules ()
            ((swap! a b)
             (let ((temp a))
               (set! a b)
               (set! b temp)))))

        (define temp 999)
        (define x 1) (define y 2)
        (swap! x y)
        (list x y temp)
        "#,
        "(2 1 999)"  // temp should remain 999
    );
}
```

**Why**: Classic hygiene test - verifies no variable capture

### Test 4: Improper List with Ellipsis (MEDIUM IMPACT)

```rust
#[test]
fn test_ellipsis_with_improper_list() {
    let expander = TestExpander::from_definition(
        "dotted",
        r#"(syntax-rules ()
            ((_ (a b ... . rest))
             (list 'a (list 'b ...) 'rest)))"#
    ).expect("Failed to compile");

    expander.assert_expands_to(
        "(dotted (1 2 3 4 . 5))",
        "(list '1 (list '2 '3 '4) '5)"
    ).expect("Expansion failed");
}
```

**Why**: Tests ellipsis with dotted-tail lists (R7RS edge case)

### Test 5: Literal Keyword Edge Case (MEDIUM IMPACT)

```rust
#[test]
fn test_literal_keyword_context_sensitive() {
    // User variable named 'else' should not match literal 'else'
    assert_program_eval_to(
        r#"
        (define-syntax my-cond
          (syntax-rules (else)
            ((my-cond (else result)) result)
            ((my-cond (test result)) (if test result #f))))

        (define else 'not-a-keyword)
        (my-cond (else 42))  ; Should match literal 'else'
        "#,
        "42"
    );

    // But 'else' as data should work differently
    assert_program_eval_to(
        r#"
        (define else 100)
        (define (test-func x) (= x else))
        (test-func 100)
        "#,
        "#t"
    );
}
```

**Why**: Verifies literal keywords only match in pattern position

---

## 9. Testing Infrastructure

### 9.1 New Test Helper Functions

Add to `crates/patina-tests/tests/common/mod.rs`:

```rust
/// Test that expression fails with specific error pattern
pub fn assert_expansion_error(
    macro_def: &str,
    input: &str,
    expected_error_substring: &str
) {
    let expander = TestExpander::from_definition("test-macro", macro_def)
        .expect("Macro definition should compile");

    let result = expander.expand_to_string(input);
    assert!(result.is_err(), "Expected expansion to fail");

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains(expected_error_substring),
        "Error message '{}' should contain '{}'",
        error_msg,
        expected_error_substring
    );
}

/// Test that macro definition itself is invalid
pub fn assert_invalid_macro_definition(
    macro_name: &str,
    macro_def: &str,
    expected_error: &str
) {
    let result = TestExpander::from_definition(macro_name, macro_def);
    assert!(result.is_err(), "Macro definition should be invalid");

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains(expected_error),
        "Error '{}' should contain '{}'",
        error_msg,
        expected_error
    );
}

/// Batch test hygiene with multiple internal variable names
pub fn test_hygiene_no_capture_batch(
    macro_template: &str,
    internal_vars: &[&str]
) {
    for var in internal_vars {
        let macro_def = macro_template.replace("{{VAR}}", var);
        test_single_hygiene_case(&macro_def, var);
    }
}
```

### 9.2 Documentation Template

```rust
/// Test: <Short description>
///
/// Category: [Hygiene|Ellipsis|Literals|Recursion|Composition|Errors]
/// Priority: [Critical|High|Medium|Low]
/// R7RS Section: <section number if applicable>
///
/// This test verifies that <specific behavior>.
/// It catches the common bug where <describe bug>.
///
/// Example from: [Chibi|Racket|Original|Bug #123]
#[test]
fn test_descriptive_name() {
    // ...
}
```

---

## 10. Action Plan Summary

### Week 1: Foundation (35-40 hours)
- [ ] Add error handling test helpers
- [ ] Write 30 error handling tests
- [ ] Write 25 ellipsis edge case tests
- [ ] Document all new tests

### Week 2: Hygiene & Chibi (35-40 hours)
- [ ] Write 20 hygiene matrix tests
- [ ] Extract 30-50 tests from chibi r7rs-tests.scm
- [ ] Create test organization structure
- [ ] Update test documentation

**Expected Outcome**: ~200 total tests, 90% confidence in production use

### Month 2: Full Compliance (60-80 hours)
- [ ] Complete chibi test suite extraction
- [ ] Implement property-based testing
- [ ] Add real-world macro corpus
- [ ] Performance/stress testing

**Expected Outcome**: 300+ tests, full R7RS compliance, production-ready

---

## 11. Real-World Macro Corpus (Critical for Production Readiness)

### 11.1 Why Real-World Corpus is Critical

**Key Insight**: Macros are both challenging and beautifully self-contained, making them ideal test targets.

**Self-Contained Nature**:
```
Macro Testing Boundary
┌────────────────────────────────────┐
│  Input:  Macro definition          │
│          + Code to expand          │
│                                    │
│  Output: Expanded code             │
│          (pure transformation)     │
│                                    │
│  No dependencies on:               │
│  ✗ Runtime state                   │
│  ✗ I/O system                      │
│  ✗ Numeric tower                   │
│  ✗ Memory management               │
│                                    │
│  Pure syntax → syntax              │
└────────────────────────────────────┘
```

**Benefits**:
- **Fast tests**: No need to run code, just expand and compare AST
- **Deterministic**: Same input always produces same output
- **Parallelizable**: Tests are completely independent
- **No setup/teardown**: Each test is standalone

### 11.2 What Real-World Corpus Tests

Real-world macros expose edge cases that synthetic tests miss:

#### Patterns You Wouldn't Think to Test

**Example: SRFI-2 (and-let*)**
```scheme
;; Combines 'and' with 'let*' - complex binding + short-circuit logic
(and-let* ((x (find-something))
           ((valid? x))          ; No binding, just test!
           (y (process x)))
  (use-result y))
```

**Tests**:
- Mixed binding and test clauses
- Optional binding syntax (just test, no variable)
- Nested structure with early exit
- Multiple ellipsis depths

#### Edge Cases from Real Usage

**Example: Pattern Matching (Racket-style match)**
```scheme
(match value
  [(list (and x (? number?)) rest ...)
   (+ x (apply + rest))]
  [_ 'no-match])
```

**Tests**:
- Nested pattern combinators (`and`, `?`)
- Ellipsis with guards
- Multiple match arms
- Catch-all patterns

#### Performance Under Real Load

**Example: SRFI-42 (eager comprehensions)**
```scheme
(list-ec (: i 10)
         (: j 10)
         (if (< i j))
  (list i j))
```

**Expands to**: Nested loops with filtering—tests expansion time for complex nesting

### 11.3 Corpus Sources by Priority

#### High-Priority: Common Patterns (30-40 macros)

**SRFI Macros**:
- **SRFI-2**: `and-let*` - Used everywhere, complex binding + short-circuit
- **SRFI-5**: `let` with signatures (named-let extensions)
- **SRFI-8**: `receive` - Multiple value binding
- **SRFI-11**: `let-values`, `let*-values` - Multiple value destructuring
- **SRFI-26**: `cut`, `cute` - Partial application (tricky variable capture)
- **SRFI-31**: `rec` - Simple recursion binding
- **SRFI-42**: Comprehensions (`list-ec`, `do-ec`, `sum-ec`, etc.)
- **SRFI-45**: Lazy evaluation (`lazy`, `delay`, `force`)
- **SRFI-87**: `case` extensions (multiple datum per clause)

**Common Utility Macros**:
```scheme
;; Binding utilities
define-values          ; Multiple value definition
when-let, if-let       ; Anaphoric binding + test
let-optionals          ; Optional/keyword argument parsing

;; Control flow
while, until, loop     ; Custom iteration constructs
define-values          ; Multiple return values

;; Higher-order
curry, compose         ; Function combinators
partial                ; Partial application
```

#### Medium-Priority: Interesting Patterns (20-30 macros)

**Pattern Matching**:
- Chibi's `match` implementation (simpler, good baseline)
- Basic pattern matching with guards
- List, vector, record patterns
- Nested destructuring

**Control Flow Extensions**:
```scheme
amb                    ; Non-deterministic choice (backtracking)
define-generator       ; Coroutine-style generators
fluid-let              ; Dynamic binding
```

**DSL Macros**:
```scheme
;; Parser combinators
(define-parser json-parser
  (choice number string object array))

;; Query DSLs
(query db
  (select name age)
  (from users)
  (where (> age 18)))
```

#### Advanced: Stress Tests (10-20 macros)

**Meta-Programming**:
```scheme
define-record-type     ; Generates constructors, accessors, predicates
define-class           ; OOP macros with inheritance
define-interface       ; Protocol definitions
```

**Compile-Time Computation**:
```scheme
;; Static assertions
(static-assert (>= pointer-size 8))

;; Type checking macros (if implementing gradual typing)
(: add (-> Integer Integer Integer))

;; Optimization hints
(inline my-hot-function)
```

**Deep Expansion**:
- Macros that generate macros (`define-macro-defining-macro`)
- Recursive expansion (100+ levels deep)
- Large output (1000+ line expansions)

### 11.4 Corpus Organization Structure

```
crates/patina-tests/tests/corpus/
├── README.md                           # Corpus overview and guidelines
│
├── srfi/                               # SRFI implementations
│   ├── srfi-002-and-let-star.scm      # One file per SRFI
│   ├── srfi-005-let-signatures.scm
│   ├── srfi-008-receive.scm
│   ├── srfi-026-cut-cute.scm
│   ├── srfi-042-comprehensions.scm
│   └── ...
│
├── common/                             # Common utility patterns
│   ├── binding-forms.scm              # when-let, if-let, etc.
│   ├── control-flow.scm               # while, until, loop
│   ├── higher-order.scm               # curry, compose, partial
│   └── assertions.scm                 # assert, check, etc.
│
├── pattern-matching/                   # Pattern matching libraries
│   ├── basic-match.scm                # Simple match implementation
│   ├── chibi-match.scm                # Chibi's match
│   └── advanced-patterns.scm          # Guards, nested patterns
│
├── meta/                               # Meta-programming macros
│   ├── define-record-type.scm         # Record generation
│   ├── define-class.scm               # OOP macros
│   └── macro-generating-macros.scm    # Higher-order macros
│
└── stress/                             # Performance and edge cases
    ├── deep-recursion.scm             # 100+ level expansion
    ├── large-output.scm               # 1000+ line expansions
    └── complex-nesting.scm            # Deeply nested patterns
```

### 11.5 Corpus Test File Format

**Standard format for each corpus file**:

```scheme
;; corpus/srfi/srfi-002-and-let-star.scm
;;
;; SRFI-2: and-let* - Sequential binding with short-circuit evaluation
;; Specification: https://srfi.schemers.org/srfi-2/srfi-2.html
;;
;; Tests: Complex binding patterns, optional test clauses, early exit

;; ============================================================
;; DEFINITION
;; ============================================================

(define-syntax and-let*
  (syntax-rules ()
    ;; Empty bindings
    ((and-let* () body ...)
     (begin body ...))

    ;; Single test clause (no binding)
    ((and-let* ((expr)) body ...)
     (if expr (begin body ...) #f))

    ;; Single binding
    ((and-let* ((var expr)) body ...)
     (let ((var expr))
       (if var (begin body ...) #f)))

    ;; Multiple clauses (recursive)
    ((and-let* ((var expr) rest ...) body ...)
     (let ((var expr))
       (if var
           (and-let* (rest ...) body ...)
           #f)))))

;; ============================================================
;; TEST 1: Basic binding with test
;; ============================================================

;; INPUT:
(and-let* ((x 5)
           (y (+ x 1)))
  (list x y))

;; EXPECTED EXPANSION:
(let ((x 5))
  (if x
      (let ((y (+ x 1)))
        (if y
            (begin (list x y))
            #f))
      #f))

;; EXPECTED RESULT:
;; (5 6)

;; ============================================================
;; TEST 2: Test clause without binding
;; ============================================================

;; INPUT:
(and-let* ((x (find-value))
           ((valid? x))           ; Just test, no binding!
           (y (process x)))
  y)

;; EXPECTED EXPANSION:
(let ((x (find-value)))
  (if x
      (if (valid? x)              ; Test without binding
          (let ((y (process x)))
            (if y
                (begin y)
                #f))
          #f)
      #f))

;; ============================================================
;; TEST 3: Early exit on #f
;; ============================================================

;; INPUT:
(and-let* ((x #f)
           (y (error "should not evaluate")))
  'unreachable)

;; EXPECTED RESULT:
;; #f  (should short-circuit, not call error)
```

### 11.6 Corpus Testing Infrastructure

**Test runner** (`tests/corpus_tests.rs`):

```rust
/// Corpus test framework
///
/// Each .scm file in tests/corpus/ contains:
/// 1. DEFINITION section: The macro definition
/// 2. Multiple TEST sections: Input, expected expansion, expected result
///
/// The test runner:
/// - Parses the .scm file
/// - Extracts definition and test cases
/// - Validates expansions match expected
/// - Optionally runs and checks results

use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
struct CorpusTest {
    name: String,
    definition: String,
    test_cases: Vec<TestCase>,
}

#[derive(Debug)]
struct TestCase {
    input: String,
    expected_expansion: Option<String>,
    expected_result: Option<String>,
    description: String,
}

/// Parse a corpus test file
fn parse_corpus_file(path: &PathBuf) -> Result<CorpusTest, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    // Parse sections: DEFINITION, TEST 1, TEST 2, ...
    // Extract input, expected expansion, expected result
    todo!("Implement corpus file parser")
}

/// Generate Rust test for each corpus file
macro_rules! corpus_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let corpus_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/corpus");
            let test_file = corpus_dir.join($file);

            let test = parse_corpus_file(&test_file)
                .expect("Failed to parse corpus file");

            run_corpus_test(&test);
        }
    };
}

// Generate tests for all corpus files
corpus_test!(srfi_002_and_let_star, "srfi/srfi-002-and-let-star.scm");
corpus_test!(srfi_026_cut_cute, "srfi/srfi-026-cut-cute.scm");
corpus_test!(common_binding_forms, "common/binding-forms.scm");
// ... etc
```

**Automated discovery**:
```rust
/// Automatically generate tests for all .scm files in corpus/
#[cfg(test)]
mod corpus_tests {
    use super::*;

    // At compile time, scan corpus/ directory and generate test functions
    include!(concat!(env!("OUT_DIR"), "/corpus_tests.rs"));
}

// build.rs:
// fn main() {
//     generate_corpus_tests();
// }
```

### 11.7 Benefits of Corpus Approach

**1. Real-World Validation**
- Confidence that patterns used in actual Scheme code work
- Discover edge cases from real usage, not just synthetic tests

**2. Library Compatibility**
- Direct path to using existing Scheme libraries
- Test compatibility with specific SRFI implementations

**3. Performance Insights**
- Identify slow expansion patterns
- Optimize common patterns (e.g., comprehensions)

**4. Documentation**
- Corpus serves as macro pattern reference
- Examples show idiomatic usage

**5. Community Contribution**
- Easy for Scheme developers to contribute tests
- Familiar format (actual Scheme code)
- Clear pass/fail criteria

### 11.8 Corpus Development Roadmap

**Week 1: Setup (8-10 hours)**
- [ ] Create corpus directory structure
- [ ] Design .scm test file format
- [ ] Implement corpus file parser
- [ ] Create test runner infrastructure
- [ ] Write documentation (README.md)

**Week 2: High-Priority SRFIs (16-20 hours)**
- [ ] SRFI-2 (and-let*) - 5 test cases
- [ ] SRFI-8 (receive) - 3 test cases
- [ ] SRFI-26 (cut/cute) - 8 test cases
- [ ] SRFI-42 (comprehensions) - 10 test cases
- [ ] Common utilities (when-let, etc.) - 10 test cases

**Week 3: Pattern Matching & Control (16-20 hours)**
- [ ] Basic match - 15 test cases
- [ ] Control flow extensions - 8 test cases
- [ ] Higher-order utilities - 8 test cases

**Week 4: Advanced & Stress (12-16 hours)**
- [ ] Meta-programming macros - 6 test cases
- [ ] Stress tests (deep recursion, large output) - 4 test cases
- [ ] Performance benchmarking framework

**Total**: ~50-70 hours for comprehensive corpus
**Output**: 30-50 real-world macros, 70-100 test cases

### 11.9 Success Metrics

**Coverage Goals**:
- [ ] All top 10 SRFIs with macros tested
- [ ] 30+ real-world macro patterns covered
- [ ] 70+ test cases from actual usage
- [ ] 100% of R7RS-small macro features exercised by corpus

**Quality Goals**:
- [ ] Each macro has 3+ test cases (basic, edge, error)
- [ ] All test cases include expected expansion
- [ ] Documentation explains why each pattern matters
- [ ] Performance baseline established for complex macros

**Impact Goals**:
- [ ] Can run unmodified SRFI macro implementations
- [ ] Zero failures on corpus (100% pass rate)
- [ ] Expansion time < 10ms for 95% of patterns
- [ ] Ready to import existing Scheme libraries

### 11.10 Example Corpus Test: SRFI-26 (cut/cute)

**Why This Matters**:
- Widely used for partial application
- Complex variable capture (distinguishes free vs bound)
- Tests hygiene thoroughly

```scheme
;; SRFI-26: cut/cute - Partial application with holes

;; DEFINITION:
(define-syntax cut
  (syntax-rules (<> <...>)
    ((cut slot ...)
     (lambda args
       (apply (process-slots slot ...) args)))))

;; TEST 1: Basic partial application
(cut + 1 <>)
;; EXPANDS TO: (lambda (##arg#1) (+ 1 ##arg#1))
;; RESULT: A function that adds 1 to its argument

;; TEST 2: Multiple holes
(cut list <> 2 <> 4)
;; EXPANDS TO: (lambda (##arg#1 ##arg#2) (list ##arg#1 2 ##arg#2 4))
;; RESULT: A function that takes 2 args and builds list

;; TEST 3: Rest arguments
(cut list <> <...>)
;; EXPANDS TO: (lambda (##arg#1 . ##rest#1) (apply list ##arg#1 ##rest#1))
;; RESULT: A function that lists its arguments

;; TEST 4: No holes (constant function)
(cut + 1 2)
;; EXPANDS TO: (lambda () (+ 1 2))
;; RESULT: A thunk that returns 3
```

---

## 12. References

**Papers**:
- "Macros That Work" (Clinger & Rees, 1991) - Hygiene algorithm
- "A Theory of Hygienic Macros" (Herman & Wand, 2007) - Formal semantics

**Implementations**:
- Chibi-scheme: `~/Project/reference/chibi-scheme/tests/`
- Racket: https://docs.racket-lang.org/syntax/macro-testing.html
- Guile: https://www.gnu.org/software/guile/manual/html_node/Testing.html

**Specifications**:
- R7RS Section 4.3: Macros
- R7RS Section 4.2.2: Pattern Language

**Patina Files**:
- `crates/patina-frontend/src/macro_expander/`
- `crates/patina-tests/tests/compliance/macros_advanced.rs`
- `crates/patina-tests/tests/macro_expander_interface.rs`

---

## Conclusion

Patina's macro system has a **strong foundation** with 119 tests covering core functionality. With systematic expansion in error handling, ellipsis edge cases, and hygiene testing, it can achieve production readiness and enable using the broader Scheme ecosystem.

The phased approach provides incremental confidence boosts:
- **Phase 1** (1-2 weeks): 90% bug coverage, ready for careful use
- **Phase 2** (1 month): Full R7RS compliance, ready for standard libraries
- **Phase 3** (3 months): Production hardened, ready for complex packages

**Priority 1 action items** (error handling + ellipsis + hygiene) provide maximum confidence with minimal effort.
