# Test Organization Proposal

## Current State

**Test Statistics:**
- Total test functions: 105
- Ignored tests: 51 (49%)
- Test files: 9 Rust files + 8 Scheme files
- Common helpers: 1 module

**Problems:**
1. **Almost 50% of tests are ignored** - Makes it hard to track progress
2. **Scheme test files aren't run automatically** - tests/schemes/ directory unused
3. **No clear feature tracking** - Hard to see what's implemented vs planned
4. **Inconsistent naming** - Mix of r7rs_*, comparison_*, scheme_runner patterns
5. **Duplicate infrastructure** - Multiple ways to run tests
6. **No progress metrics** - Can't easily measure completion %

## Proposed Organization

### Directory Structure

```
tests/
├── common/
│   ├── mod.rs                    # Shared test helpers
│   ├── chibi.rs                  # Chibi-scheme comparison helpers
│   └── fixtures.rs               # Test data and constants
│
├── unit/                         # Unit tests (fast, isolated)
│   ├── mod.rs
│   ├── lexer_test.rs
│   ├── parser_test.rs
│   ├── eval_test.rs
│   └── env_test.rs
│
├── compliance/                   # R7RS compliance tests (organized by spec)
│   ├── mod.rs
│   │
│   ├── primitives/               # Section 4.1: Primitive expressions
│   │   ├── mod.rs
│   │   ├── variables.rs          # 4.1.1: Variable references
│   │   ├── literals.rs           # 4.1.2: Literal expressions (quote)
│   │   ├── procedures.rs         # 4.1.4: Procedures (lambda)
│   │   ├── conditionals.rs       # 4.1.5: Conditionals (if)
│   │   └── assignments.rs        # 4.1.6: Assignments (define, set!)
│   │
│   ├── derived/                  # Section 4.2: Derived expressions
│   │   ├── mod.rs
│   │   ├── cond.rs               # 4.2.1: Conditionals (cond, case)
│   │   ├── binding.rs            # 4.2.2: Binding (let, let*, letrec)
│   │   ├── sequencing.rs         # 4.2.3: Sequencing (begin)
│   │   ├── iteration.rs          # 4.2.4: Iteration (do)
│   │   └── delayed.rs            # 4.2.5: Delayed evaluation
│   │
│   ├── numbers/                  # Section 6.2: Numbers
│   │   ├── mod.rs
│   │   ├── arithmetic.rs         # +, -, *, /, abs, quotient, etc.
│   │   ├── comparison.rs         # =, <, >, <=, >=
│   │   ├── predicates.rs         # number?, integer?, zero?, etc.
│   │   └── conversion.rs         # exact, inexact, number->string
│   │
│   ├── lists/                    # Section 6.4: Pairs and lists
│   │   ├── mod.rs
│   │   ├── constructors.rs       # cons, list, make-list
│   │   ├── accessors.rs          # car, cdr, caar, cadr, etc.
│   │   ├── predicates.rs         # pair?, list?, null?
│   │   └── operations.rs         # length, append, reverse, etc.
│   │
│   ├── strings/                  # Section 6.7: Strings
│   │   ├── mod.rs
│   │   ├── constructors.rs       # string, make-string
│   │   ├── accessors.rs          # string-ref, string-set!
│   │   └── operations.rs         # string-append, substring, etc.
│   │
│   ├── vectors/                  # Section 6.8: Vectors
│   │   ├── mod.rs
│   │   ├── constructors.rs
│   │   └── operations.rs
│   │
│   ├── control/                  # Section 6.10: Control features
│   │   ├── mod.rs
│   │   ├── procedures.rs         # apply, map, for-each
│   │   └── continuations.rs      # call/cc, dynamic-wind
│   │
│   └── io/                       # Section 6.13: Input/Output
│       ├── mod.rs
│       ├── ports.rs
│       └── operations.rs
│
├── integration/                  # Integration tests (end-to-end)
│   ├── mod.rs
│   ├── repl_test.rs              # Test REPL behavior
│   ├── file_runner.rs            # Run .scm files
│   └── chibi_comparison.rs       # Side-by-side with chibi
│
└── fixtures/                     # Test data files
    ├── r7rs/                     # Official R7RS test cases
    │   ├── primitives.scm
    │   ├── numbers.scm
    │   ├── lists.scm
    │   └── ...
    │
    └── examples/                 # Example programs
        ├── factorial.scm
        ├── fibonacci.scm
        └── closures.scm
```

## Test Status Tracking

### Option 1: Feature Matrix (Recommended)

Create `tests/compliance/FEATURE_MATRIX.md`:

```markdown
# R7RS (scheme base) Feature Matrix

## Status Legend
- ✅ Implemented & Tested
- 🚧 Implemented, Tests Incomplete
- ⏸️ Partially Implemented
- ❌ Not Implemented
- 📝 Planned

## Primitives (Section 4.1)

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| Variable references | ✅ | 2/2 | Full support |
| quote | ✅ | 5/5 | All forms working |
| lambda (basic) | ✅ | 4/4 | Fixed arity |
| lambda (variadic) | ✅ | 2/2 | Rest parameters |
| if | ✅ | 4/4 | With/without else |
| define (variable) | ✅ | 3/3 | Basic bindings |
| define (function) | ❌ | 0/5 | Sugar not implemented |
| set! | ✅ | 2/3 | Error handling pending |

## Derived Expressions (Section 4.2)

| Feature | Status | Tests | Notes |
|---------|--------|-------|-------|
| cond | ✅ | 2/4 | Basic + else working |
| case | ❌ | 0/5 | Not implemented |
| and | ❌ | 0/3 | Not implemented |
| or | ❌ | 0/3 | Not implemented |
| when | ❌ | 0/2 | Not implemented |
| unless | ❌ | 0/2 | Not implemented |
| let | ❌ | 0/6 | Not implemented |
| let* | ❌ | 0/4 | Not implemented |
| letrec | ❌ | 0/4 | Not implemented |
| begin | ✅ | 2/2 | Working |
| do | ❌ | 0/5 | Not implemented |

## Numbers (Section 6.2)

| Feature | Status | Tests | Progress |
|---------|--------|-------|----------|
| +, -, *, / | ✅ | 8/8 | Integer only |
| =, <, >, <=, >= | ✅ | 10/10 | Integer only |
| abs | ❌ | 0/3 | Not implemented |
| quotient, remainder | ❌ | 0/6 | Not implemented |
| modulo | ❌ | 0/3 | Not implemented |
| gcd, lcm | ❌ | 0/4 | Not implemented |
| floor, ceiling, etc. | ❌ | 0/12 | Not implemented |
| expt | ❌ | 0/4 | Not implemented |

**Total Progress: 28/240 (12%)**
```

### Option 2: Test Status Enum (Alternative)

Instead of `#[ignore]`, use a custom attribute system:

```rust
// tests/common/status.rs
pub enum TestStatus {
    Implemented,           // Fully working
    PartiallyImplemented, // Some cases work
    Blocked(String),      // Blocked by another feature
    Planned,              // Not yet started
}

// Usage:
#[test]
#[status(TestStatus::Blocked("Requires let binding"))]
fn test_lambda_closure_with_let() {
    // test code
}
```

Then generate reports with:
```bash
cargo test -- --list | ./scripts/test_status_report.sh
```

## Test Naming Convention

**Current:** Inconsistent mix of patterns
**Proposed:** Clear hierarchy

```
Format: test_{category}_{feature}_{variant}

Examples:
- test_primitive_lambda_basic
- test_primitive_lambda_variadic
- test_primitive_lambda_closure
- test_derived_let_simple
- test_derived_let_nested
- test_numbers_arithmetic_add_integers
- test_numbers_arithmetic_add_mixed
- test_lists_append_two_lists
```

## Running Tests

```bash
# All tests
cargo test

# Unit tests only (fast)
cargo test --test unit

# Specific R7RS section
cargo test --test compliance_primitives
cargo test --test compliance_numbers

# Feature-specific
cargo test test_primitive_lambda
cargo test test_numbers_arithmetic

# Integration only
cargo test --test integration

# Generate coverage report
cargo test --test compliance -- --show-output | ./scripts/coverage.sh
```

## Auto-generated Test Files

Create `.scm` test files that auto-generate Rust tests:

```scheme
;; tests/fixtures/r7rs/primitives.scm
(test-group "lambda"
  (test "simple lambda" ((lambda (x) x) 42) 42)
  (test "multiple args" ((lambda (x y) (+ x y)) 1 2) 3)
  (test "closure"
    (begin
      (define make-adder (lambda (n) (lambda (x) (+ x n))))
      ((make-adder 5) 10))
    15))
```

Build script generates:
```rust
// tests/compliance/primitives/procedures_generated.rs
#[test]
fn test_lambda_simple_lambda() {
    assert_eval_to("((lambda (x) x) 42)", "42");
}
// ... more tests
```

## Progress Tracking Scripts

### 1. Generate Feature Matrix
```bash
./scripts/generate_feature_matrix.sh
```
Scans all test files, counts passed/failed/ignored, updates FEATURE_MATRIX.md

### 2. Test Coverage Report
```bash
cargo test --all-tests 2>&1 | ./scripts/test_report.sh
```
Output:
```
R7RS Compliance Report
======================
Primitives:        18/25 (72%)
Derived:           2/30 (7%)
Numbers:          11/45 (24%)
Lists:             6/30 (20%)
Strings:           0/25 (0%)
Vectors:           0/20 (0%)
Control:           0/15 (0%)
I/O:               0/25 (0%)
---------------------------------
Total:            37/215 (17%)
```

## Migration Plan

### Phase 1: Restructure (Week 1)
1. Create new directory structure
2. Move existing tests to new locations
3. Update imports and module declarations
4. Ensure all tests still pass

### Phase 2: Feature Matrix (Week 1)
1. Create FEATURE_MATRIX.md
2. Audit all tests and categorize by status
3. Replace `#[ignore]` with better tracking

### Phase 3: Fixtures (Week 2)
1. Move .scm files to fixtures/
2. Create test harness to auto-run fixtures
3. Add more comprehensive test cases

### Phase 4: Automation (Week 2)
1. Create scripts for test reporting
2. Add CI integration
3. Generate documentation from test results

## Benefits

1. **Clear Progress Tracking** - Easy to see what's done vs what's left
2. **Better Organization** - Tests match R7RS spec structure
3. **Faster Development** - Easy to find and run relevant tests
4. **Better Documentation** - Tests serve as spec documentation
5. **CI/CD Ready** - Easy to generate reports and track metrics
6. **Contributor Friendly** - Clear where to add new tests

## Alternative: Keep Current + Improve

If full restructure is too much work:

**Minimal improvements:**
1. Replace `#[ignore]` with `#[ignore = "reason"]` for better tracking
2. Create FEATURE_MATRIX.md manually
3. Add test count script: `./scripts/count_tests.sh`
4. Group tests better with modules in existing files
5. Auto-run .scm files in tests/schemes/

**Time investment:** 2-4 hours vs 2 weeks for full restructure

## Recommendation

**Start with minimal improvements** (Phase 1 + 2), then migrate incrementally as you add new features. The full restructure can happen gradually over time.

**Immediate actions:**
1. Create FEATURE_MATRIX.md (track current state)
2. Add better `#[ignore]` messages with blockers
3. Create simple test count script
4. Set up fixture runner for .scm files

This gives you 80% of the benefit with 20% of the effort.
