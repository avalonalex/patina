# Test Organization Strategy

## Overview (Updated 2025-12-12)

The Patina test suite has grown significantly (~1,500 tests) and follows a clear organizational pattern:

1. **Unit Tests** - Component-level tests inline with source code
2. **Integration Tests** - Full-stack tests in dedicated `patina-tests` crate

## Test Structure

```
workspace/
├── crates/
│   ├── patina-core/
│   │   └── src/
│   │       └── *.rs          # ~138 unit tests inline
│   │
│   ├── patina-macros/
│   │   └── src/
│   │       ├── compiler/tests.rs    # Extracted test module
│   │       ├── expander/tests.rs    # Extracted test module
│   │       └── *.rs          # ~183 unit tests inline
│   │
│   ├── patina-frontend/
│   │   └── src/
│   │       └── *.rs          # ~100 unit tests inline
│   │
│   ├── patina-ir/
│   │   └── src/
│   │       └── *.rs          # ~22 unit tests inline
│   │
│   ├── patina-runtime/
│   │   └── src/
│   │       └── *.rs          # ~9 unit tests inline
│   │
│   ├── patina-tree-walker/
│   │   └── src/
│   │       └── *.rs          # ~15 unit tests inline
│   │
│   ├── patina-pipeline/
│   │   └── src/
│   │       └── *.rs          # ~13 unit tests inline
│   │
│   ├── patina-interpreter/
│   │   └── src/
│   │       └── *.rs          # ~3 unit tests inline
│   │
│   └── patina-tests/         # Integration test crate
│       ├── Cargo.toml
│       └── tests/
│           ├── common/       # Test utilities
│           ├── compliance/   # R7RS compliance tests
│           ├── integration/  # Chibi comparison tests
│           └── *.rs          # Feature-specific tests
```

## Test Categories

### 1. Unit Tests (In Component Crates)

**Location:** Inline with `#[cfg(test)]` in source files

**Purpose:** Test component internals and edge cases in isolation

**Current Distribution:**

| Crate | Unit Tests | Notes |
|-------|-----------|-------|
| patina-core | 138 | Value operations, CoreExpr |
| patina-macros | 183 | Pattern matching, hygiene |
| patina-frontend | 100 | Lexer, parser, desugarer |
| patina-ir | 22 | CPS transformation |
| patina-tree-walker | 15 | Evaluator internals |
| patina-pipeline | 13 | Pipeline orchestration |
| patina-runtime | 9 | Environment, library system |
| patina-interpreter | 3 | High-level API |
| **Total** | **~483** | |

**Run with:**
```bash
# Run unit tests for a specific crate
cargo test --package patina-frontend
cargo test --package patina-macros
cargo test --package patina-core
```

### 2. Integration Tests (patina-tests Crate)

**Location:** `crates/patina-tests/tests/`

**Purpose:** Test the complete interpreter working end-to-end

**Categories:**

#### **Compliance Tests** (`tests/compliance/`)
R7RS specification compliance organized by category:
- `numbers.rs` - Numeric operations (~30 tests)
- `strings.rs` - String operations (~25 tests)
- `lists.rs` - List operations (~20 tests)
- `vectors.rs` - Vector operations
- `predicates.rs` - Type predicates
- `derived.rs` - Derived forms (let, cond, case)
- `control.rs` - Control flow
- `quasiquote.rs` - Quasiquote expansion
- `macros_advanced.rs` - Advanced macro patterns (~60 tests)
- `rationals.rs` - Rational numbers
- `numeric_edge_cases.rs` - Edge cases

#### **Feature Tests** (top-level `tests/`)
- `cps_features.rs` - CPS-specific behavior (31 tests)
- `hygiene.rs` - Macro hygiene (~108 tests)
- `tail_recursion.rs` - TCO correctness (~36 tests)
- `numeric_operations.rs` - Numeric tower (~25 tests)
- `complex_numbers.rs` - Complex number support (~20 tests)
- `record_types.rs` - define-record-type (~40 tests)
- `lazy_evaluation.rs` - delay/force
- `parameters.rs` - Parameter objects
- `scheme_eval.rs` - (scheme eval) library
- `case_lambda.rs` - case-lambda macro

#### **Library Tests**
- `sld_file_loading.rs` - Library loading (~50 tests)
- `r7rs_libraries.rs` - R7RS library compliance
- `scheme_base.rs` - (scheme base) library
- `scheme_process_context.rs` - Process context

#### **Integration Tests** (`tests/integration/`)
- Compare Patina output with chibi-scheme
- Test full program execution
- Verify compatibility

**Run with:**
```bash
# Run all integration tests
cargo test --package patina-tests

# Run specific test file
cargo test --package patina-tests --test hygiene
cargo test --package patina-tests --test cps_features

# Run compliance tests
cargo test --package patina-tests --test compliance

# Run specific category
cargo test --package patina-tests numbers::
cargo test --package patina-tests primitives::
```

## Test Utilities

**Common Helpers** (`crates/patina-tests/tests/common/mod.rs`):
```rust
// Primary assertion helpers
assert_eval_to(expr, expected)           // Evaluate and compare result
assert_eval_error(expr)                  // Verify error is raised
assert_program_eval_to(code, expected)   // Multi-expression programs
assert_eval_type(expr, check, name)      // Verify result type
```

## Test Counts (as of 2025-12-12)

| Category | Tests | Notes |
|----------|-------|-------|
| Unit tests (all crates) | ~483 | Inline with production code |
| Integration tests | ~1,000+ | In patina-tests crate |
| **Total** | **~1,500** | 24 ignored (document bugs) |

**By test file (largest):**
| File | Tests | Lines |
|------|-------|-------|
| compliance.rs | ~380 | via sub-modules |
| hygiene.rs | ~108 | 1,065 |
| scheme_base.rs | ~50 | |
| sld_file_loading.rs | ~50 | 985 |
| record_types.rs | ~40 | 747 |
| tail_recursion.rs | ~36 | 698 |
| cps_features.rs | 31 | 580 |

## Running Tests

### Run Everything
```bash
# All tests (unit + integration) - ~1,500 tests
cargo test --workspace

# Only integration tests (most comprehensive)
cargo test --package patina-tests
```

### Run Specific Categories
```bash
# Only unit tests for a crate
cargo test --package patina-frontend
cargo test --package patina-macros

# Only R7RS compliance
cargo test --package patina-tests --test compliance

# Only CPS features
cargo test --package patina-tests --test cps_features

# Only macro hygiene
cargo test --package patina-tests --test hygiene
```

### Development Workflow
```bash
# During development: test the component you're working on
cargo test --package patina-frontend  # If working on parser
cargo test --package patina-macros    # If working on macros

# Before commit: run all tests
cargo test --workspace

# Quick sanity check
cargo test --package patina-tests --test interpreter_api
```

## Inline Test Guidelines

### When to Use Inline Tests

**Good for inline tests:**
- Testing private functions not accessible from outside
- Testing internal invariants
- Unit testing helper functions
- Tests tightly coupled to implementation details

**Move to patina-tests when:**
- Testing public API behavior
- Testing feature integration across modules
- Tests become >100 lines in a single module
- Tests could apply to multiple backends

### Current Inline Test Distribution

The macro system has the highest inline test density due to the complexity of pattern matching and hygiene:

| Module | Prod Lines | Test Lines | Ratio |
|--------|-----------|-----------|-------|
| patina-macros/interface.rs | 207 | 500 | 2.4x |
| patina-macros/matcher/mod.rs | 244 | 316 | 1.3x |
| patina-frontend/library_parser.rs | 577 | 683 | 1.2x |
| patina-frontend/parser/mod.rs | 941 | 885 | 0.9x |
| patina-frontend/desugarer/mod.rs | 1,340 | 727 | 0.5x |

**Note:** High test ratios in macro code are acceptable given the domain complexity.

## Benefits of This Organization

### Clear Separation
- Unit tests live with their components
- Integration tests in dedicated crate
- Feature tests grouped by functionality

### Scalable for Multi-Backend
Backend crates:
```
crates/
├── patina-vm/             # Register-based bytecode VM (default)
├── patina-tree-walker/    # CPS tree-walking backend (--tree-walker)
├── patina-jit/            # JIT compiler backend (future)
└── patina-tests/          # Tests ALL backends
    └── tests/
        ├── compliance/    # Run against each backend
        └── integration/   # Compare all backends vs chibi
```

### Fast Iteration
- Test only what you're working on
- Unit tests run in milliseconds
- Integration tests run when needed

### Clear Dependencies
```
patina-tests depends on:
  └─ patina-interpreter
      ├─ patina-frontend
      ├─ patina-tree-walker
      ├─ patina-pipeline
      └─ patina-runtime
```

## Known Issues and Future Work

### Documented Bugs (via ignored tests)
The test suite documents known bugs by marking tests as `#[ignore]`:
- Some tests are marked ignored to document implementation differences with chibi-scheme

### Future Enhancements

**When Adding VM Backend:**
1. Create `patina-vm` crate
2. Integration tests will automatically test it via `Backend` trait
3. Add VM-specific unit tests in `patina-vm/src/`

**When Adding Benchmarks:**
```bash
# Future: crates/patina-benchmarks/
cargo bench --package patina-benchmarks
```

## Summary

**Current Organization:**
- ~483 unit tests inline with component crates
- ~1,000+ integration tests in `patina-tests` crate
- Clear separation by purpose and scope
- Scales well for multiple backends
- Fast, targeted testing during development

**Key Principle:**
> Unit tests verify component correctness in isolation.
> Integration tests verify the full interpreter works end-to-end.
> Compliance tests verify R7RS spec adherence across all backends.
