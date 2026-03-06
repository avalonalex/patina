# Patina Performance Report

**Generated:** 2026-03-05 19:04:13
**Commit:** 59190cc
**Branch:** benchmark
**Total Time:** 7m 29s

## Platform

| Property | Value |
|----------|-------|
| OS | macOS 26.3 |
| CPU | Apple M1 Max |
| Cores | 10 |
| Memory | 32GB |
| Rust | rustc 1.93.1 (01f6ddf75 2026-02-11) |

## Summary

- **Benchmarks Run:** 38
- **Mode:** Standard (20 samples)

## Results

| Benchmark | Median Time | Category |
|-----------|-------------|----------|
| `r7rs/tak/18_12_6` | 299.90 ms | R7RS Classic |
| `r7rs/tak/12_8_4` | 8.2304 ms | R7RS Classic |
| `r7rs/fib/15` | 6.8370 ms | R7RS Classic |
| `r7rs/fib/20` | 75.376 ms | R7RS Classic |
| `r7rs/fib/25` | 832.26 ms | R7RS Classic |
| `r7rs/ack/3/4` | 45.764 ms | R7RS Classic |
| `r7rs/ack/3/6` | 783.46 ms | R7RS Classic |
| `r7rs/deriv/single` | 393.34 µs | R7RS Classic |
| `r7rs/deriv/1000_iter` | 388.23 ms | R7RS Classic |
| `r7rs/primes/100` | 3.9045 ms | R7RS Classic |
| `r7rs/primes/500` | 43.795 ms | R7RS Classic |
| `r7rs/primes/1000` | 130.07 ms | R7RS Classic |
| `r7rs/nqueens/8` | 270.40 ms | R7RS Classic |
| `r7rs/nqueens/10` | 6.6321 s | R7RS Classic |
| `r7rs/sum/1000` | 4.5479 ms | R7RS Classic |
| `r7rs/sum/5000` | 22.959 ms | R7RS Classic |
| `r7rs/sum/10000` | 45.695 ms | R7RS Classic |
| `r7rs/ctak/12_8_4` | 13.333 ms | R7RS Classic |
| `r7rs/ctak/8_4_2` | 994.88 µs | R7RS Classic |
| `continuations/dynamic_wind/simple` | 6.6438 µs | Continuations |
| `continuations/dynamic_wind/nested_10` | 59.296 µs | Continuations |
| `continuations/dynamic_wind/nested_20` | 120.98 µs | Continuations |
| `continuations/callcc/simple` | 4.5326 µs | Continuations |
| `continuations/callcc/loop/50` | 257.40 µs | Continuations |
| `continuations/callcc/loop/100` | 520.72 µs | Continuations |
| `continuations/callcc/loop/200` | 1.0273 ms | Continuations |
| `data/lists/make_1000` | 3.9564 ms | Data Structures |
| `data/lists/reverse_1000` | 55.751 µs | Data Structures |
| `data/lists/map_1000` | 23.150 ms | Data Structures |
| `data/lists/append_500_500` | 4.0039 ms | Data Structures |
| `data/vectors/make_1000` | 5.9774 µs | Data Structures |
| `data/vectors/sum_1000` | 7.1230 ms | Data Structures |
| `data/vectors/fill_1000` | 5.8092 ms | Data Structures |
| `numeric/sum_10000` | 43.051 ms | Numeric |
| `numeric/factorial/20` | 93.764 µs | Numeric |
| `numeric/factorial/50` | 230.03 µs | Numeric |
| `numeric/factorial/100` | 459.77 µs | Numeric |
| `numeric/float_sum_1000` | 4.2068 ms | Numeric |

## Category Breakdown

| Category | Count | Representative Benchmark | Time |
|----------|-------|-------------------------|------|
| R7RS Classic | 19 | `r7rs/tak/18_12_6` | 299.90 ms |
| Continuations | 7 | `continuations/dynamic_wind/simple` | 6.6438 µs |
| Data Structures | 7 | `data/lists/make_1000` | 3.9564 ms |
| Numeric | 5 | `numeric/sum_10000` | 43.051 ms |

## Historical Comparison

Previous run: 2026-03-05 17:08

See `benchmark_reports/history.csv` for full history.

## Notes

### Benchmark Sources

Benchmarks are ported from [ecraven/r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks),
the standard R7RS benchmark suite based on Gabriel and Larceny benchmarks.

### Running Benchmarks

```bash
# Full benchmark run (~5 min)
./scripts/run_benchmarks.sh

# Quick run (~2 min)
./scripts/run_benchmarks.sh --quick

# Run specific category
./scripts/run_benchmarks.sh --filter "r7rs/fib"
```

### Optimization Targets

See `PRD/phase1/CLONE_OPTIMIZATION_ANALYSIS.md` for identified optimization opportunities.
Key areas:
- CPS evaluator clone overhead
- Continuation environment (HashMap) cloning
- Dynamic wind record cloning
