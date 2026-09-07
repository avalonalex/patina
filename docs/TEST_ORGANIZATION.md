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
│           ├── scheme/       # *.scm test files, run by scheme_suite.rs
│           │   ├── control/  #   evaluation order, tail calls, wind, parameters
│           │   ├── reader/   #   identifier syntax
│           │   ├── expansion/#   macros, include, core syntactic bindings
│           │   ├── data/     #   values and conversions
│           │   ├── stdlib/   #   the (scheme …) libraries
│           │   ├── srfi/     #   bundled SRFI behaviour
│           │   └── libraries/#   import/export machinery visible from Scheme
│           └── *.rs          # Feature-specific tests
```

### Scheme test files (`tests/scheme/`, driven by `scheme_suite.rs`)

**Prefer these for new tests of the language.** They are ordinary, portable
SRFI 64 programs — `(import (scheme base) (srfi 64))`, `test-begin`,
`test-equal`, `test-end` — and one Rust driver runs every one of them on every
backend. Adding a backend touches the driver; adding a test touches neither.

**`scheme_suite.rs`'s `SUITE` table is the enumeration of these files** — the
list below covers `tests/*.rs` only, and does not carry notes about files that
have migrated out of it. `the_suite_table_and_the_directory_agree` keeps `SUITE`
and the directory in step, which no prose list can do.

Two reasons to reach for a `.scm` file first:

- **Cost.** Measured on a warm build, adding one `.scm` file rebuilds in
  **0.098 s**; adding one `.rs` test file costs **8.96 s**, because every `.rs`
  file directly in a `tests/` directory is its own crate and its own link
  against the whole workspace. That is the problem CLAUDE.md's build-cost table
  describes, and #193's reason for existing — 88 binaries when it was measured,
  84 as of 2026-09-07.
- **Portability.** The same file runs under chibi and Gauche unchanged, which
  makes it an oracle and not only a suite. Differences are real findings — for
  `callability.scm`, Gauche's three disagreements are the deliberate
  divergences its own comments already document.

  Not every file gets *both* oracles, and which ones it gets belongs **in the
  file**, measured, not restated here where the two copies drift apart. The
  control-flow files are the case in point: chibi cannot survive some deep
  `dynamic-wind`/continuation shapes, so each says which rows it corroborates
  and which it dies on.

  A file that disclaims a property should say where the property is checked
  instead. `tail-recursion.scm` is the case in point: its rows pin that each
  special form evaluates correctly when its last expression recurses, not that
  the call is a *tail* call in constant space — that one is
  `vm_callprimitive.rs::tail_deopt_runs_deep_mutual_recursion` at 100 000 deep,
  with the space measurement itself recorded out-of-band in PRD TRACK_P §P8.2
  (5.49 MB against 109 MB before the fix).

  Two things follow for a new file. **Order a row an oracle cannot survive
  last**, because SRFI 64 stops the file where it dies — in
  `internal-escape-boundaries.scm` that one move took chibi from 6 rows to 10.
  And **do not generalise one bad row to the file**: the first draft of that
  file claimed chibi could not arbitrate it at all, which threw away ten rows of
  corroboration that were there for the asking.

**Where a new `.scm` file goes: directory by kind, filename by concern.** The
directory is one of the seven above and says what *sort* of thing the file is
about; the filename keeps the concern name the test has always had
(`tail-recursion.scm`, `wind-thunk-exceptions.scm`), because several of these
files are named after defect classes rather than spec sections and the name is
the documentation. Deliberately *not* by R7RS section: `conversion` alone spans
§6.2, 6.6, 6.7 and 6.8, and a section number would delete why the file exists.

Two migration rules learned the hard way in #193 Phase 1, both from rows that
passed while asserting something else:

- **Keep top-level `define`s at top level.** A top-level self-reference
  resolves through the global environment at call time; an internal `define` is
  `letrec*` and compiles to a local slot. Wrapping a recursive row in
  `(let () (define …) …)` silently moves it off the path it was written for.
- **Sequence side effects with `let*`, never argument positions.** R7RS leaves
  argument evaluation order unspecified and chibi evaluates right to left, so
  `(list (c) (c 5) (c 3) (c))` answers differently there — reporting a
  difference between implementations that is not one.

**What still belongs in a `.rs` file**, because a `.scm` file cannot express
it: a row whose backends give *different values*; a row deliberately asserted
on one backend; that an error escapes an *unguarded* program (observable only
from outside it); and anything asserting *which stage* rejected a program, or
touching `Heap`, `VmState`, `Instruction`, `SourceMap`, GC counters or the
library registry. The rule of thumb #193 uses: what is about the **language**
goes to Scheme, what is about the **implementation** stays in Rust.

The unguarded rows have **one** home: `callability.rs`, whatever feature they
came from. A file of their own would split the class across two places and,
since `callability.rs` exists regardless, would give back the binary the
migration just saved.

**A row whose *premise* is Patina-specific** — as opposed to one whose answer
differs between our two backends — can stay in Scheme, scoped with the
`cond-expand` identifier #208 added:

```scheme
(cond-expand (patina) (else (test-skip "the row's name")))
(test-equal "the row's name" ...)
```

The `test-skip` is not decoration. `cond-expand` alone deletes the row on other
implementations with nothing anywhere saying so, and a row that can vanish
quietly is what the skip and floor checks exist to prevent; with it, chibi and
Gauche *report* a skip. Use this only where the premise genuinely is ours (a
Patina-specific validation, say), never to paper over a difference in an
answer — that is a divergence, and it belongs in Rust where it can be named.

Every file is listed in `scheme_suite.rs`'s `SUITE` table with a minimum
assertion count. That floor is not bookkeeping — a file that stops running
reports no failures, so without it a truncated or skipped file passes. Skips
are rejected outright for the same reason.

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
- `numeric_operations.rs` - Numeric tower (~25 tests)
- `complex_numbers.rs` - Complex number support (~20 tests)
- `record_types.rs` - define-record-type (~40 tests)
- `lazy_evaluation.rs` - delay/force
- `scheme_eval.rs` - (scheme eval) library

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
| tail-recursion.scm | 36 | 301 |
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
