#!/bin/bash
# Profile a Patina benchmark using samply
# Usage: ./scripts/profile_benchmark.sh [benchmark_name] [args]
#
# Examples:
#   ./scripts/profile_benchmark.sh fib 25
#   ./scripts/profile_benchmark.sh tak 18 12 6

set -e

BENCHMARK="${1:-fib}"
shift || true

# Default arguments for each benchmark
case "$BENCHMARK" in
    fib)
        ARGS="${1:-25}"
        CODE="(load \"crates/patina-tests/bench_programs/fib.scm\") (fib $ARGS)"
        ;;
    tak)
        X="${1:-18}"
        Y="${2:-12}"
        Z="${3:-6}"
        CODE="(load \"crates/patina-tests/bench_programs/tak.scm\") (tak $X $Y $Z)"
        ;;
    sum)
        N="${1:-10000}"
        CODE="(load \"crates/patina-tests/bench_programs/sum.scm\") (run $N)"
        ;;
    primes)
        N="${1:-1000}"
        CODE="(load \"crates/patina-tests/bench_programs/primes.scm\") (length (primes<= $N))"
        ;;
    *)
        echo "Unknown benchmark: $BENCHMARK"
        echo "Available: fib, tak, sum, primes"
        exit 1
        ;;
esac

echo "=== Patina Benchmark Profiler ==="
echo "Benchmark: $BENCHMARK"
echo "Code: $CODE"
echo ""

# Build release with debug info
echo "Building with debug symbols..."
RUSTFLAGS="-C debuginfo=2" cargo build --release --quiet

# Create temp file with the benchmark code
TEMP_FILE=$(mktemp /tmp/patina_bench_XXXXXX.scm)
echo "$CODE" > "$TEMP_FILE"

echo "Running profiler (this will open Firefox Profiler)..."
echo ""

# Run samply
samply record ./target/release/patina "$TEMP_FILE"

# Cleanup
rm -f "$TEMP_FILE"
