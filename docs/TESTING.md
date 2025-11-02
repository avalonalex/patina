# Testing Guide

Complete guide to running and writing tests for Patina.

## Quick Start

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test compliance      # R7RS compliance tests
cargo test --test integration     # Integration tests

# Run tests for specific category
cargo test --test compliance primitives
cargo test --test compliance numbers

# Skip chibi-scheme comparison tests
SKIP_CHIBI_TESTS=1 cargo test

# Generate progress report
./scripts/test_report.sh
```

## Test Organization

Tests are organized to mirror the R7RS specification:

```
tests/
├── compliance.rs          # Main compliance test file
├── compliance/            # R7RS spec-organized tests
│   ├── primitives.rs     # Section 4.1: quote, lambda, if, define
│   ├── derived.rs        # Section 4.2: cond, let, and, or
│   ├── numbers.rs        # Section 6.2: Arithmetic
│   ├── lists.rs          # Section 6.4: Lists and pairs
│   └── predicates.rs     # Type predicates
├── integration.rs         # Main integration test file
├── integration/           # End-to-end tests
│   ├── comparison_test.rs # Chibi-scheme comparison
│   ├── scheme_runner.rs   # Scheme file runner
│   └── file_runner.rs     # File discovery
├── fixtures/              # Test data
│   ├── r7rs/             # R7RS test cases
│   └── examples/         # Example programs
└── common/                # Shared test helpers
    └── mod.rs            # assert_eval_to, assert_program_eval_to
```

## Running Tests

### All Tests
```bash
cargo test
```

### Compliance Tests Only
```bash
cargo test --test compliance
```

### Specific Categories
```bash
cargo test --test compliance primitives
cargo test --test compliance numbers
cargo test --test compliance lists
```

### Integration Tests
```bash
cargo test --test integration
```

### With Output
```bash
cargo test -- --nocapture
```

### Specific Test
```bash
cargo test test_simple_lambda
cargo test --test compliance test_simple_lambda
```

## Writing Tests

### Compliance Tests

Add tests to appropriate file in `tests/compliance/`:

```rust
// tests/compliance/primitives.rs

#[test]
fn test_my_feature() {
    assert_eval_to("(my-feature 42)", "expected-output");
}

#[test]
fn test_my_feature_with_program() {
    assert_program_eval_to(
        r#"
        (define x 10)
        (my-feature x)
        "#,
        "expected-output",
    );
}

#[test]
#[ignore = "Blocked by: missing feature X"]
fn test_future_feature() {
    // Test that will pass once feature X is implemented
    assert_eval_to("(future-thing)", "result");
}
```

### Test Helpers

Available in `tests/common/mod.rs`:

```rust
// Assert exact output
assert_eval_to("(+ 1 2)", "3");

// Assert multi-expression program
assert_program_eval_to(
    r#"
    (define x 5)
    (* x 2)
    "#,
    "10",
);

// Assert error occurs
assert_eval_error("(/ 1 0)");

// Assert type
assert_eval_type(
    "(lambda (x) x)",
    |v| matches!(v, Value::Procedure(_)),
    "procedure",
);
```

### Integration Tests

Add to `tests/integration/`:

```rust
// tests/integration/scheme_runner.rs

#[test]
fn test_feature_integration() {
    let interp = Interpreter::new();

    // Setup
    interp.eval_str("(define x 10)").unwrap();

    // Test
    let result = interp.eval_str("(+ x 5)").unwrap();

    // Assert
    assert!(matches!(result, Value::Integer(15)));
}
```

## Test Fixtures

Add Scheme test files to `tests/fixtures/`:

```scheme
;; tests/fixtures/r7rs/my-feature.scm

;; Basic test
(define x 42)
x  ; Should output: 42

;; Complex test
(define factorial
  (lambda (n)
    (if (<= n 1)
        1
        (* n (factorial (- n 1))))))

(factorial 5)  ; Should output: 120
```

## Test Status Tracking

### Feature Matrix

See `FEATURE_STATUS.md` (or `tests/FEATURE_MATRIX.md`) for detailed feature-by-feature status.

### Progress Report

Generate visual progress report:

```bash
./scripts/test_report.sh
```

Output:
```
╔════════════════════════════════════════════════╗
║     Patina R7RS Compliance Test Report        ║
╔════════════════════════════════════════════════╗

Test Category Results
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Primitives:           18/20  ( 90%) ✅
Numbers:              11/23  ( 47%) ⚠️
Lists:                 6/19  ( 31%) ⚠️
Predicates:            7/12  ( 58%) ✅
Derived Forms:         2/19  ( 10%) ❌
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Overall Statistics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Total Tests:        93
Passing:            44 (47%)
Ignored:            49

Progress: [███████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░] 47%
```

### Test Count

```bash
# Count all tests
cargo test --test compliance -- --list | wc -l

# Count ignored tests
cargo test --test compliance -- --list --ignored | wc -l
```

## Chibi-scheme Comparison

### Why Chibi?

Chibi-scheme is the R7RS reference implementation. We use it to verify correctness.

### Setup

Install chibi-scheme:
```bash
# macOS
brew install chibi-scheme

# Ubuntu/Debian
sudo apt-get install chibi-scheme
```

### Running Comparison Tests

```bash
# With chibi-scheme
cargo test --test integration

# Without chibi-scheme
SKIP_CHIBI_TESTS=1 cargo test
```

### Using Chibi as Reference

```bash
# Test expression in both
echo "(+ 1 2 3)" | chibi-scheme
cargo run --release  # then type (+ 1 2 3)

# Run chibi's R7RS test suite
chibi-scheme /path/to/chibi-scheme/tests/r7rs-tests.scm
```

See [CHIBI_REFERENCE.md](CHIBI_REFERENCE.md) for more details.

## Test Best Practices

### 1. Test One Feature Per Test

✅ Good:
```rust
#[test]
fn test_lambda_basic() {
    assert_eval_to("((lambda (x) x) 42)", "42");
}

#[test]
fn test_lambda_multiple_params() {
    assert_eval_to("((lambda (x y) (+ x y)) 1 2)", "3");
}
```

❌ Bad:
```rust
#[test]
fn test_lambda() {
    assert_eval_to("((lambda (x) x) 42)", "42");
    assert_eval_to("((lambda (x y) (+ x y)) 1 2)", "3");
    // Too many things in one test
}
```

### 2. Use Descriptive Names

✅ Good: `test_lambda_closure_captures_environment`
❌ Bad: `test_lambda_1`

### 3. Document Blocked Tests

```rust
#[test]
#[ignore = "Blocked by: let binding not implemented"]
fn test_lambda_with_let() {
    // This will work once let is implemented
}
```

### 4. Group Related Tests

Put related tests in the same file/module:
- All lambda tests in `primitives.rs` (Section 4.1.4)
- All let tests in `derived.rs` (Section 4.2.2)

### 5. Test Edge Cases

```rust
#[test]
fn test_division_by_zero() {
    assert_eval_error("(/ 1 0)");
}

#[test]
fn test_lambda_no_params() {
    assert_eval_to("((lambda () 42))", "42");
}
```

## Continuous Integration

Tests run automatically on:
- Every push
- Every pull request
- GitHub Actions workflow

See `.github/workflows/` for CI configuration.

## Troubleshooting

### Tests hang
- Use `cargo test -- --test-threads=1` to run serially
- Check for infinite loops in test code

### Comparison tests fail
- Install chibi-scheme or use `SKIP_CHIBI_TESTS=1`
- Check chibi is in PATH: `which chibi-scheme`

### Build fails before tests
- Update Rust: `rustup update`
- Clean build: `cargo clean && cargo build`

## Next Steps

- Read [FEATURE_STATUS.md](FEATURE_STATUS.md) to see what needs tests
- Check [DEVELOPMENT.md](DEVELOPMENT.md) for architecture
- See [CHIBI_REFERENCE.md](CHIBI_REFERENCE.md) for using the R7RS reference

---

**Remember:** Tests are documentation. Write clear, focused tests that explain what the feature does.
