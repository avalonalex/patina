# Profiling and Benchmarking Plan

This document outlines the strategy for profiling Patina's performance and establishing benchmarks for optimization work.

**Status**: Planning
**Related**: `CLONE_OPTIMIZATION_ANALYSIS.md` (deferred pending profiling)

---

## Goals

1. **Identify hotspots** - Find where time is actually spent before optimizing
2. **Establish baselines** - Measure current performance for comparison
3. **Track regressions** - Prevent performance degradation as features are added
4. **Compare backends** - Enable Phase 2 VM backend comparison with tree-walker

---

## R7RS Benchmark Suites

### Primary: ecraven/r7rs-benchmarks

The community-maintained [ecraven/r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks) is the standard R7RS benchmark suite, derived from:
- **Gabriel benchmarks** - Classic Lisp/Scheme benchmarks from the 1980s
- **Gambit benchmarks** - Extended suite from Gambit Scheme
- **Larceny benchmarks** - Additional benchmarks from the Larceny project

**Resources:**
- Repository: https://github.com/ecraven/r7rs-benchmarks
- Results: https://ecraven.github.io/r7rs-benchmarks/
- Documentation: http://www.larcenists.org/benchmarksAboutR7.html

### Benchmark Categories

#### Tier 1: Core Algorithms (Run First)

These benchmarks test fundamental operations Patina already supports:

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `tak` | Triply recursive Takeuchi function | Non-tail recursion, arithmetic |
| `fib` | Doubly recursive Fibonacci | Recursion, arithmetic |
| `ack` | Ackermann function | Deep recursion |
| `sum` | Sum integers 0-10000 | Loops, arithmetic |
| `primes` | Sieve of Eratosthenes | List operations |
| `deriv` | Symbolic differentiation | Symbols, lists, pattern matching |
| `browse` | Database browsing | Symbols, lists |
| `nqueens` | N-queens problem | Backtracking, lists |
| `quicksort` | Sort 10K integers | Vectors, comparison |

**Patina readiness**: HIGH - These use basic R7RS features we fully support.

#### Tier 2: Numeric Intensive

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `fibfp` | Floating-point Fibonacci | Inexact arithmetic |
| `sumfp` | Floating-point summation | Inexact arithmetic |
| `fft` | Fast Fourier Transform | Vectors, inexact math |
| `mbrot` | Mandelbrot set | Complex arithmetic, inexact |
| `ray` | Ray tracing | Inexact arithmetic, vectors |
| `simplex` | Simplex algorithm | Inexact arithmetic |
| `pnpoly` | Point-in-polygon | Inexact arithmetic |
| `pi` | Pi digit calculation | Bignums |

**Patina readiness**: HIGH - Full numeric tower implemented.

#### Tier 3: Continuation-Heavy

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `ctak` | Continuation-capturing tak | `call/cc` |
| `cpstak` | CPS-style tak | Closures (no call/cc) |
| `fibc` | Continuation-based Fibonacci | `call/cc` |

**Patina readiness**: HIGH - CPS evaluator with full `call/cc` support.

#### Tier 4: I/O and Strings

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `cat` | File copying | File I/O |
| `wc` | Word count | Character I/O |
| `tail` | Reverse file printing | File I/O |
| `read1` | Read Scheme files | `read` procedure |
| `string` | String operations | `string-append`, `substring` |

**Patina readiness**: HIGH - Full I/O system implemented.

#### Tier 5: GC Stress Tests

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `nboyer` | Boyer theorem prover | Lists, vectors, GC pressure |
| `sboyer` | Storage-optimized boyer | Lists, vectors |
| `gcbench` | GC stress test | Tree allocation |
| `mperm` | Permutation generation | Heavy allocation |

**Patina readiness**: MEDIUM - Tests GC which we don't have (uses Rc currently).

#### Tier 6: Complex Programs

| Benchmark | Description | Key Features Required |
|-----------|-------------|----------------------|
| `scheme` | Scheme interpreter | Full language |
| `compiler` | Compiler kernel | Symbols, lists |
| `earley` | Earley parser | Complex algorithms |
| `dynamic` | Type inference | Higher-order functions |
| `peval` | Partial evaluator | Metaprogramming |
| `slatex` | Scheme→LaTeX | File I/O, strings |

**Patina readiness**: MEDIUM-HIGH - May need testing for edge cases.

---

## Setting Up r7rs-benchmarks for Patina

### Step 1: Clone the benchmark suite

```bash
cd ~/Project/reference
git clone https://github.com/ecraven/r7rs-benchmarks.git
```

### Step 2: Create Patina runner script

Create `r7rs-benchmarks/bench-patina`:
```bash
#!/bin/bash
# Runner for Patina interpreter
PATINA="${PATINA:-~/Project/patina/target/release/patina}"
exec "$PATINA" "$@"
```

### Step 3: Add Patina to the benchmark framework

The benchmark suite expects implementations to follow a pattern. We need to:
1. Check if Patina can run the prelude (`src/prelude.scm`)
2. Verify benchmark compatibility one by one
3. Document any required patches

### Step 4: Run individual benchmarks

```bash
# Test a single benchmark
./bench patina tak
./bench patina fib

# Run all benchmarks
./bench patina all
```

---

## Patina-Specific Micro-Benchmarks

In addition to standard benchmarks, create Patina-specific tests targeting suspected hotspots:

### Clone Overhead Tests

```scheme
;; bench/clone-stress.scm
;; Tests HashMap<Rc<str>, ContValue> cloning in CPS evaluator

;; Deep continuation nesting
(define (deep-cont n)
  (if (= n 0)
      (call/cc (lambda (k) (k 'done)))
      (+ 1 (deep-cont (- n 1)))))

(deep-cont 1000)  ; Measure time

;; Many dynamic-wind layers
(define (wind-stress n)
  (if (= n 0)
      'done
      (dynamic-wind
        (lambda () #f)
        (lambda () (wind-stress (- n 1)))
        (lambda () #f))))

(wind-stress 100)  ; Measure time
```

### CPS Transform Overhead

```scheme
;; bench/cps-overhead.scm
;; Compare simple vs continuation-heavy code

;; Simple loop (minimal CPS overhead)
(define (simple-loop n acc)
  (if (= n 0) acc (simple-loop (- n 1) (+ acc 1))))

;; Same with call/cc (forces full CPS machinery)
(define (cc-loop n acc)
  (call/cc
    (lambda (k)
      (if (= n 0) acc (cc-loop (- n 1) (+ acc 1))))))

(simple-loop 100000 0)
(cc-loop 100000 0)
```

### Allocation Pressure

```scheme
;; bench/alloc-stress.scm
;; Test Rc allocation/deallocation patterns

;; Cons-heavy (tests Pair allocation)
(define (make-list n)
  (if (= n 0) '() (cons n (make-list (- n 1)))))

;; Vector allocation
(define (make-vectors n)
  (if (= n 0) '() (cons (make-vector 100) (make-vectors (- n 1)))))

(length (make-list 10000))
(length (make-vectors 1000))
```

---

## Profiling Tools

### CPU Profiling

#### Option 1: Instruments (macOS - Recommended)

```bash
# Build with debug symbols
cargo build --release

# Profile with Instruments
xcrun xctrace record --template "Time Profiler" \
  --launch ./target/release/patina bench/tak.scm
```

#### Option 2: samply (Cross-platform, macOS-friendly)

```bash
cargo install samply

# Profile and open in Firefox Profiler
samply record ./target/release/patina bench/tak.scm
```

#### Option 3: perf + flamegraph (Linux)

```bash
cargo install flamegraph

# Generate flamegraph
cargo flamegraph -- ./target/release/patina bench/tak.scm
```

### Memory Profiling

#### Option 1: Instruments Allocations (macOS)

```bash
xcrun xctrace record --template "Allocations" \
  --launch ./target/release/patina bench/nboyer.scm
```

#### Option 2: heaptrack (Linux)

```bash
heaptrack ./target/release/patina bench/nboyer.scm
heaptrack_gui heaptrack.patina.*.gz
```

#### Option 3: DHAT (Valgrind)

```bash
valgrind --tool=dhat ./target/release/patina bench/nboyer.scm
```

### Custom Instrumentation

Add optional compile-time instrumentation for clone counting:

```rust
// In Cargo.toml
[features]
clone-tracking = []

// In code
#[cfg(feature = "clone-tracking")]
thread_local! {
    static CLONE_COUNT: std::cell::RefCell<HashMap<&'static str, usize>> =
        RefCell::new(HashMap::new());
}

#[cfg(feature = "clone-tracking")]
pub fn track_clone(type_name: &'static str) {
    CLONE_COUNT.with(|c| {
        *c.borrow_mut().entry(type_name).or_insert(0) += 1;
    });
}
```

---

## Metrics to Collect

### Performance Metrics

| Metric | Tool | Target |
|--------|------|--------|
| Wall-clock time | `time` command | Compare to chibi-scheme |
| CPU cycles | `perf stat` | Identify CPU-bound code |
| Instructions/cycle | `perf stat` | Cache efficiency |
| Branch mispredictions | `perf stat` | Prediction quality |

### Memory Metrics

| Metric | Tool | Purpose |
|--------|------|---------|
| Peak heap usage | Instruments/heaptrack | Memory efficiency |
| Allocation count | DHAT | Allocation pressure |
| Allocation hotspots | heaptrack | Where allocations happen |
| Rc clone count | Custom instrumentation | Clone overhead |

### Patina-Specific Metrics

| Metric | How to Measure | Purpose |
|--------|----------------|---------|
| CPS transform time | Instrument `CpsTransformer` | Transform overhead |
| Trampoline iterations | Counter in eval loop | CPS efficiency |
| cont_env size | Instrument `ContValue::Local` | HashMap growth |
| dynamic_winds depth | Instrument wind stack | Wind overhead |

---

## Benchmark Comparison Targets

Compare Patina against these implementations:

| Implementation | Type | Why Compare |
|----------------|------|-------------|
| **chibi-scheme** | Tree-walking interpreter | Similar architecture, R7RS reference |
| **Guile** | Bytecode VM | Target for Phase 2 VM |
| **Chez Scheme** | Native compiler | Upper bound of performance |
| **Racket** | JIT compiler | Mature implementation |

Expected performance ordering: `Patina < chibi ≈ Guile < Racket < Chez`

---

## Decision Framework

### When to Optimize

1. **Hotspot threshold**: Only optimize code that accounts for >10% of runtime
2. **Measurable impact**: Optimization must show >5% improvement in benchmarks
3. **Complexity budget**: Added complexity must be justified by performance gain

### Optimization Priority Matrix

| Optimization | Effort | Impact | Priority |
|--------------|--------|--------|----------|
| Rc-wrap StepResult fields | Low | Medium | **P1** if profiling confirms |
| Persistent cont_env (im crate) | Medium | High | **P1** if cont_env is hot |
| Avoid .as_ref().clone() | Low | Low | **P2** |
| Custom allocator | High | Unknown | **P3** - defer |

### Go/No-Go Criteria

Before implementing any optimization from `CLONE_OPTIMIZATION_ANALYSIS.md`:

1. [ ] Profile shows the code path is >10% of total time
2. [ ] Micro-benchmark demonstrates the specific issue
3. [ ] Proposed fix has measurable improvement in isolation
4. [ ] No correctness regressions in test suite

---

## Cargo Benchmark Setup (Ready to Use)

Benchmarks are integrated into the `patina-tests` crate using Criterion:

```bash
# Run all benchmarks
cargo bench --package patina-tests

# Run specific benchmark group
cargo bench --package patina-tests -- "r7rs/fib"
cargo bench --package patina-tests -- "continuations"

# List all benchmarks
cargo bench --package patina-tests -- --list
```

### Benchmark Structure

```
crates/patina-tests/
├── benches/
│   └── scheme_benchmarks.rs    # Criterion harness
└── bench_programs/
    ├── common.scm              # Shared definitions (hide, run-benchmark)
    ├── tak.scm                 # Gabriel: Takeuchi function
    ├── fib.scm                 # Gabriel: Fibonacci
    ├── ack.scm                 # KVW: Ackermann function
    ├── deriv.scm               # Gabriel: Symbolic differentiation
    ├── primes.scm              # Sieve of Eratosthenes
    ├── nqueens.scm             # N-queens problem
    ├── sum.scm                 # KVW: Integer summation
    └── ctak.scm                # Gabriel: Continuation-capturing tak
```

### Benchmark Groups

| Group | Benchmarks | What It Tests |
|-------|------------|---------------|
| `r7rs/*` | tak, fib, ack, deriv, primes, nqueens, sum | Classic R7RS performance |
| `r7rs/ctak` | ctak | Continuation capture overhead |
| `continuations/*` | callcc, dynamic_wind | CPS evaluator performance |
| `data/*` | lists, vectors | Data structure operations |
| `numeric/*` | sum, factorial, float_sum | Arithmetic performance |

### Current Benchmark Results (Baseline)

Run `cargo bench --package patina-tests` to establish baseline. Results are saved to `target/criterion/`.

---

## Implementation Plan

### Phase 1: Setup (COMPLETE)

1. [x] ~~Clone r7rs-benchmarks repository~~ (Ported benchmarks directly)
2. [x] Criterion benchmark harness integrated
3. [x] 38 benchmarks across 5 groups
4. [x] Benchmark programs in bench_programs/*.scm

### Phase 2: Baseline (2-3 hours)

1. [ ] Run all compatible benchmarks
2. [ ] Record baseline times
3. [ ] Compare against chibi-scheme
4. [ ] Identify outliers (unusually slow benchmarks)

### Phase 3: Profile Hotspots (2-3 hours)

1. [ ] Profile slowest benchmarks with Instruments/samply
2. [ ] Identify top 5 hotspot functions
3. [ ] Correlate with clone analysis predictions
4. [ ] Document findings

### Phase 4: Targeted Optimization (Ongoing)

1. [ ] Address confirmed hotspots from `CLONE_OPTIMIZATION_ANALYSIS.md`
2. [ ] Re-run benchmarks after each optimization
3. [ ] Track improvement percentages
4. [ ] Update documentation

---

## Appendix: Quick Start Commands

### Benchmark Script (Recommended)

```bash
# Full benchmark suite with report (~5 min)
./scripts/run_benchmarks.sh

# Quick run with fewer samples (~2 min)
./scripts/run_benchmarks.sh --quick

# Run specific benchmark category
./scripts/run_benchmarks.sh --filter "r7rs/fib"
./scripts/run_benchmarks.sh --filter "continuations"

# View generated report
cat benchmark_reports/performance.md

# View historical data
cat benchmark_reports/history.csv
```

### Manual Cargo Commands

```bash
# Build optimized binary
cargo build --release

# Run all Criterion benchmarks directly
cargo bench --package patina-tests

# Run specific benchmark group
cargo bench --package patina-tests -- "r7rs/fib"
cargo bench --package patina-tests -- "continuations/callcc"

# View Criterion HTML reports
open target/criterion/report/index.html
```

### Profiling

```bash
# Profile with samply (installs Firefox Profiler integration)
cargo install samply
samply record ./target/release/patina crates/patina-tests/bench_programs/fib.scm

# Profile with Instruments (macOS)
xcrun xctrace record --template "Time Profiler" \
  --launch ./target/release/patina crates/patina-tests/bench_programs/tak.scm
```

### Verification

```bash
# Run test suite to verify no regressions
cargo test --package patina-tests
```

---

## References

- [ecraven/r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks) - Primary benchmark suite
- [Larceny R7RS Benchmarks](http://www.larcenists.org/benchmarksAboutR7.html) - Benchmark documentation
- [R7RS Benchmark Results](https://ecraven.github.io/r7rs-benchmarks/) - Cross-implementation comparison
- [The Rust Performance Book](https://nnethercote.github.io/perf-book/) - Rust profiling guide
