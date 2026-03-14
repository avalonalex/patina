# Patina Benchmark Comparison: Tree-Walker vs VM Backend

**Generated:** 2026-03-14
**Commit:** 4bf9fe3
**Branch:** main

## Platform

| Property | Value |
|----------|-------|
| OS | macOS 26.3 |
| CPU | Apple M1 Max |
| Cores | 10 |
| Memory | 32GB |
| Rust | rustc 1.93.1 (01f6ddf75 2026-02-11) |

## Summary

- **VM speedup range:** 2.8–5.8x across all 25 benchmarks
- **Average speedup:** ~4.2x
- **No specialized opcodes or optimizations** — baseline register machine only

## Results

### R7RS Classic (recursion, arithmetic)

| Benchmark | Tree-Walker | VM | Speedup |
|-----------|------------|-----|---------|
| fib(20) | 68.1 ms | 11.7 ms | **5.82x** |
| fib(25) | 489.5 ms | 128.7 ms | 3.80x |
| fib(30) | 5461 ms | 1343 ms | 4.07x |
| tak(18,12,6) | 177.2 ms | 39.4 ms | 4.49x |
| tak(12,8,4) | 5.3 ms | 1.1 ms | 4.62x |
| ack(3,4) | 27.6 ms | 6.0 ms | 4.57x |
| ack(3,6) | 456.5 ms | 97.8 ms | 4.67x |
| primes(500) | 27.8 ms | 5.8 ms | 4.83x |
| primes(1000) | 74.6 ms | 17.0 ms | 4.39x |
| nqueens(8) | 152.0 ms | 39.4 ms | 3.86x |
| nqueens(10) | 3636 ms | 895.9 ms | 4.06x |
| sum(5000) | 14.1 ms | 3.2 ms | 4.43x |
| sum(10000) | 28.1 ms | 6.5 ms | 4.34x |
| deriv(1000) | 237.4 ms | 67.9 ms | 3.49x |

### Continuations

| Benchmark | Tree-Walker | VM | Speedup |
|-----------|------------|-----|---------|
| ctak(12,8,4) | 9.1 ms | 2.2 ms | 4.08x |
| ctak(8,4,2) | 0.67 ms | 0.18 ms | 3.76x |
| dynamic-wind(simple) | 2.2 ms | 0.45 ms | 4.81x |
| call/cc(loop 100) | 0.34 ms | 0.12 ms | 2.83x |

### Data Structures

| Benchmark | Tree-Walker | VM | Speedup |
|-----------|------------|-----|---------|
| list make(1000) | 2.9 ms | 0.69 ms | 4.11x |
| list reverse(1000) | 2.8 ms | 0.61 ms | 4.49x |
| list map(1000) | 17.5 ms | 4.8 ms | 3.64x |
| vector sum(1000) | 3.7 ms | 0.82 ms | 4.51x |

### Numeric

| Benchmark | Tree-Walker | VM | Speedup |
|-----------|------------|-----|---------|
| sum-to(10000) | 28.2 ms | 6.5 ms | 4.34x |
| factorial(50) | 0.20 ms | 0.04 ms | 4.74x |
| float-sum(1000) | 2.9 ms | 0.66 ms | 4.44x |

## Analysis

### Speedup by Category

| Category | Min | Max | Average |
|----------|-----|-----|---------|
| R7RS Classic | 3.49x | 5.82x | ~4.3x |
| Continuations | 2.83x | 4.81x | ~3.9x |
| Data Structures | 3.64x | 4.51x | ~4.2x |
| Numeric | 4.34x | 4.74x | ~4.5x |

### Where VM Wins Most

- **fib(20): 5.82x** — pure non-tail recursion benefits most from register dispatch vs CPS evaluation
- **primes(500): 4.83x** — list-heavy sieve with many small function calls
- **dynamic-wind: 4.81x** — control flow overhead lower in VM
- **factorial(50): 4.74x** — tight tail-recursive loop with bignum

### Where VM Wins Least

- **call/cc(loop 100): 2.83x** — continuation capture involves similar cloning costs in both backends
- **deriv(1000): 3.49x** — symbol-heavy workload; heap allocation dominates over dispatch
- **list map(1000): 3.64x** — higher-order primitive dispatch path similar

### Why ~4x

The tree-walker uses CPS-transformed evaluation with environment hash lookups on every variable access. The VM uses:
- O(1) register access (array index) instead of hash lookup
- Direct instruction dispatch instead of CPS closure chains
- Flat closure captures instead of environment chain traversal

The ~4x speedup is consistent with what other Scheme implementations see when moving from tree-walking to bytecode (e.g., Chibi, Gauche). Further gains from specialized opcodes (Phase 2B) could push this to 8–15x.

## Methodology

Single-run timing via `(current-jiffy)` / `(jiffies-per-second)` on each backend. Not Criterion-style statistical benchmarking — numbers may vary ±10% between runs. Script: `scripts/bench_compare.sh`.
