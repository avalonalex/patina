# Test Organization Strategy

## Overview (Updated 2024-11-11)

After workspace refactoring, tests are now organized into two categories:

1. **Unit Tests** - Component-level tests in each crate
2. **Integration Tests** - Full-stack tests in dedicated `patina-tests` crate

## Test Structure

```
workspace/
├── crates/
│   ├── patina-runtime/
│   │   └── src/
│   │       └── *.rs          # Unit tests inline with #[cfg(test)]
│   │
│   ├── patina-frontend/
│   │   └── src/
│   │       └── *.rs          # Unit tests inline with #[cfg(test)]
│   │
│   ├── patina-tree-walker/
│   │   └── src/
│   │       └── *.rs          # Unit tests inline with #[cfg(test)]
│   │
│   └── patina-tests/         # ⭐ INTEGRATION TEST CRATE
│       ├── Cargo.toml
│       └── tests/
│           ├── compliance/   # R7RS compliance tests
│           ├── integration/  # Chibi comparison tests
│           ├── interpreter_api.rs  # Interpreter API tests
│           └── common/       # Test utilities
│
└── patina/ (main crate)
    └── src/
        └── lib.rs           # Only doctests remain
```

## Test Categories

### 1. Unit Tests (In Component Crates)

**Location:** Inline with `#[cfg(test)]` in source files

**Purpose:** Test component internals and edge cases in isolation

**Examples:**

#### `patina-runtime/src/environment.rs`
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_define_and_get() { ... }

    #[test]
    fn test_parent_lookup() { ... }
}
```

#### `patina-frontend/src/lexer/mod.rs`
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_tokens() { ... }

    #[test]
    fn test_reject_reserved_characters() { ... }
}
```

**Run with:**
```bash
# Run unit tests for a specific crate
cargo test --package patina-frontend
cargo test --package patina-runtime
cargo test --package patina-tree-walker
```

### 2. Integration Tests (patina-tests Crate)

**Location:** `crates/patina-tests/tests/`

**Purpose:** Test the complete interpreter working end-to-end

**Categories:**

#### **Compliance Tests** (`tests/compliance/`)
- Test R7RS specification compliance
- Organized by spec sections (primitives, derived, numbers, lists, etc.)
- ~285 tests covering core language features

#### **Integration Tests** (`tests/integration/`)
- Compare Patina output with chibi-scheme (reference implementation)
- Test full program execution
- Verify compatibility with other Scheme implementations

#### **Interpreter API Tests** (`tests/interpreter_api.rs`)
- Test the high-level `Interpreter` API
- Verify public interface works correctly
- Test macros, eval_str, eval_program

#### **Feature Tests**
- `tail_recursion.rs` - TCO correctness (~36 tests)
- `numeric_operations.rs` - Numeric tower (~25 tests)
- `complex_numbers.rs` - Complex number support
- `verify_bigint_promotion.rs` - Integer overflow handling

**Run with:**
```bash
# Run all integration tests
cargo test --package patina-tests

# Run specific test suite
cargo test --package patina-tests --test compliance
cargo test --package patina-tests --test integration
cargo test --package patina-tests --test interpreter_api

# Run specific category
cargo test --package patina-tests numbers::
cargo test --package patina-tests primitives::
```

## Test Utilities

**Common Helpers** (`crates/patina-tests/tests/common/mod.rs`):
```rust
// Test helpers for concise assertions
assert_eval_to(expr, expected)      // Evaluate and compare result
assert_eval_error(expr)              // Verify error is raised
assert_eval_type(expr, check, name) // Verify result type
assert_program_eval_to(code, expected) // Multi-expression programs
```

**Fixtures** (`crates/patina-tests/tests/fixtures/`):
- `r7rs/` - R7RS test programs
- `examples/` - Example Scheme programs

## Test Counts (as of 2024-11-11)

| Crate | Unit Tests | Integration Tests |
|-------|------------|-------------------|
| `patina-runtime` | 2 | - |
| `patina-frontend` | 61 | - |
| `patina-tree-walker` | 0 | - |
| `patina-tests` | - | ~370 |
| **Total** | **63** | **~370** |

## Running Tests

### Run Everything
```bash
# All tests (unit + integration)
cargo test --workspace

# Faster: Only integration tests (most comprehensive)
cargo test --package patina-tests
```

### Run Specific Categories
```bash
# Only unit tests
cargo test --package patina-frontend
cargo test --package patina-runtime

# Only R7RS compliance
cargo test --package patina-tests --test compliance

# Only integration (chibi comparison)
cargo test --package patina-tests --test integration

# Only tail recursion tests
cargo test --package patina-tests --test tail_recursion
```

### Development Workflow
```bash
# During development: test the component you're working on
cargo test --package patina-frontend  # If working on parser

# Before commit: run all tests
cargo test --workspace

# Quick sanity check: interpreter API tests
cargo test --package patina-tests --test interpreter_api
```

## Benefits of This Organization

### ✅ Clear Separation
- Unit tests live with their components
- Integration tests in dedicated crate
- No confusion about what to test where

### ✅ Scalable for Multi-Backend
When we add new backends (VM, JIT):
```
crates/
├── patina-tree-walker/    # Tree-walking backend (current)
├── patina-vm/             # Bytecode VM backend (future)
├── patina-jit/            # JIT compiler backend (future)
└── patina-tests/          # Tests ALL backends
    └── tests/
        ├── compliance/    # Run against each backend
        └── integration/   # Compare all backends vs chibi
```

### ✅ Fast Iteration
- Test only what you're working on
- Unit tests run in milliseconds
- Integration tests run when needed

### ✅ Clear Dependencies
```
patina-tests depends on:
  └─ patina (main API)
      ├─ patina-frontend
      ├─ patina-tree-walker
      └─ patina-runtime
```

## Migration Notes

### What Changed (2024-11-11)
1. ✅ Created `patina-tests` crate for integration tests
2. ✅ Moved all integration tests from `tests/` to `crates/patina-tests/tests/`
3. ✅ Moved API tests from `src/lib.rs` to `crates/patina-tests/tests/interpreter_api.rs`
4. ✅ Unit tests remain inline in component crates

### What Stayed the Same
- All test assertions and logic unchanged
- Test utilities still in `common/mod.rs`
- R7RS compliance organization maintained
- ~435 total tests (unit + integration)

## Future Enhancements

### When Adding VM Backend
1. Create `patina-vm` crate
2. No test changes needed - `patina-tests` will test it automatically
3. Optionally: Add VM-specific unit tests in `patina-vm/src/`

### When Adding Reference Testing
```rust
// Future: crates/patina-tests/tests/reference.rs
// Compare Patina against multiple implementations
test_against_chibi()
test_against_guile()
test_against_chez()
```

### When Adding Benchmarks
```bash
# Future: crates/patina-benchmarks/
# Separate from tests for performance comparison
cargo bench --package patina-benchmarks
```

## Summary

**Current Organization: Dedicated Integration Tests Crate** ✅

- ✅ Unit tests in component crates (inline)
- ✅ Integration tests in `patina-tests` crate
- ✅ Clear separation of concerns
- ✅ Scales well for multiple backends
- ✅ Fast, targeted testing during development
- ✅ Comprehensive integration testing before release

**Key Principle:**
> Unit tests verify component correctness in isolation.
> Integration tests verify the full interpreter works end-to-end.
> Compliance tests verify R7RS spec adherence across all backends.
