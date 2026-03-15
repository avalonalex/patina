#!/bin/bash
# Quick benchmark comparison: tree-walker vs VM backend
# Usage: ./scripts/bench_compare.sh [--quick]

set -e

PATINA="./target/release/patina"
BENCH_DIR="crates/patina-tests/bench_programs"
QUICK=false

if [ "$1" = "--quick" ]; then
    QUICK=true
fi

# Build release
echo "Building release binary..."
cargo build --release --quiet 2>/dev/null

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "  Patina Benchmark Comparison: Tree-Walker vs VM Backend"
echo "═══════════════════════════════════════════════════════════════"
echo ""

# Benchmark function: runs a .scm file + expression, returns time in ms
# Usage: run_bench <backend_flag> <setup_file> <expr> <label>
run_bench() {
    local flag="$1"
    local setup="$2"
    local expr="$3"

    # Create temp file with setup + timed expression
    local tmp=$(mktemp /tmp/bench_XXXXXX.scm)
    if [ -n "$setup" ]; then
        cat "$setup" >> "$tmp"
    fi
    cat >> "$tmp" << EOF

(import (scheme time))
(let ((start (current-jiffy)))
  $expr
  (let* ((end (current-jiffy))
         (jps (jiffies-per-second))
         (elapsed-ms (/ (* (- end start) 1000.0) jps)))
    (display elapsed-ms)
    (newline)))
EOF

    local result
    if [ "$flag" = "--tree-walker" ]; then
        result=$($PATINA --tree-walker "$tmp" 2>/dev/null)
    else
        result=$($PATINA "$tmp" 2>/dev/null)
    fi
    rm -f "$tmp"
    echo "$result"
}

# Format a comparison line
compare() {
    local label="$1"
    local setup="$2"
    local expr="$3"

    printf "%-30s" "$label"

    local tw_ms=$(run_bench "--tree-walker" "$setup" "$expr")
    local vm_ms=$(run_bench "" "$setup" "$expr")

    if [ -z "$tw_ms" ] || [ -z "$vm_ms" ]; then
        printf "  ERROR\n"
        return
    fi

    # Calculate ratio using awk
    local ratio=$(echo "$tw_ms $vm_ms" | awk '{if ($2 > 0) printf "%.2f", $1/$2; else print "N/A"}')
    local tw_fmt=$(echo "$tw_ms" | awk '{if ($1 >= 1000) printf "%8.0f ms", $1; else if ($1 >= 1) printf "%8.1f ms", $1; else printf "%8.2f ms", $1}')
    local vm_fmt=$(echo "$vm_ms" | awk '{if ($1 >= 1000) printf "%8.0f ms", $1; else if ($1 >= 1) printf "%8.1f ms", $1; else printf "%8.2f ms", $1}')

    printf "%s  %s  %sx\n" "$tw_fmt" "$vm_fmt" "$ratio"
}

printf "%-30s %10s  %10s  %s\n" "Benchmark" "Tree-Walk" "VM" "Speedup"
echo "────────────────────────────────────────────────────────────────────────"

# R7RS Classic
echo ""
echo "R7RS Classic (recursion, arithmetic)"
echo "────────────────────────────────────"

if [ "$QUICK" = true ]; then
    compare "fib(20)"     "$BENCH_DIR/fib.scm"     "(fib 20)"
    compare "fib(25)"     "$BENCH_DIR/fib.scm"     "(fib 25)"
    compare "tak(12,8,4)" "$BENCH_DIR/tak.scm"     "(tak 12 8 4)"
    compare "ack(3,4)"    "$BENCH_DIR/ack.scm"     "(ack 3 4)"
    compare "primes(500)" "$BENCH_DIR/primes.scm"  "(length (primes<= 500))"
    compare "nqueens(8)"  "$BENCH_DIR/nqueens.scm" "(nqueens 8)"
    compare "sum(5000)"   "$BENCH_DIR/sum.scm"     "(run 5000)"
    compare "deriv(1000)" "$BENCH_DIR/deriv.scm"   "(let loop ((i 1000)) (if (= i 0) 'done (begin (deriv test-expr) (loop (- i 1)))))"
else
    compare "fib(20)"      "$BENCH_DIR/fib.scm"     "(fib 20)"
    compare "fib(25)"      "$BENCH_DIR/fib.scm"     "(fib 25)"
    compare "fib(30)"      "$BENCH_DIR/fib.scm"     "(fib 30)"
    compare "tak(18,12,6)" "$BENCH_DIR/tak.scm"     "(tak 18 12 6)"
    compare "tak(12,8,4)"  "$BENCH_DIR/tak.scm"     "(tak 12 8 4)"
    compare "ack(3,4)"     "$BENCH_DIR/ack.scm"     "(ack 3 4)"
    compare "ack(3,6)"     "$BENCH_DIR/ack.scm"     "(ack 3 6)"
    compare "primes(500)"  "$BENCH_DIR/primes.scm"  "(length (primes<= 500))"
    compare "primes(1000)" "$BENCH_DIR/primes.scm"  "(length (primes<= 1000))"
    compare "nqueens(8)"   "$BENCH_DIR/nqueens.scm" "(nqueens 8)"
    compare "nqueens(10)"  "$BENCH_DIR/nqueens.scm" "(nqueens 10)"
    compare "sum(5000)"    "$BENCH_DIR/sum.scm"     "(run 5000)"
    compare "sum(10000)"   "$BENCH_DIR/sum.scm"     "(run 10000)"
    compare "deriv(1000)"  "$BENCH_DIR/deriv.scm"   "(let loop ((i 1000)) (if (= i 0) 'done (begin (deriv test-expr) (loop (- i 1)))))"
fi

# Continuations
echo ""
echo "Continuations"
echo "────────────────────────────────────"

if [ "$QUICK" = true ]; then
    compare "ctak(8,4,2)"  "$BENCH_DIR/ctak.scm" "(ctak 8 4 2)"
else
    compare "ctak(12,8,4)" "$BENCH_DIR/ctak.scm" "(ctak 12 8 4)"
    compare "ctak(8,4,2)"  "$BENCH_DIR/ctak.scm" "(ctak 8 4 2)"
fi

compare "dynamic-wind(simple)" "" "(dynamic-wind (lambda () #f) (lambda () (let loop ((n 1000)) (if (= n 0) 'done (loop (- n 1))))) (lambda () #f))"
compare "call/cc(loop 100)" "" "(let loop ((n 100) (acc 0)) (if (= n 0) acc (call/cc (lambda (k) (loop (- n 1) (+ acc 1))))))"

# Data structures
echo ""
echo "Data Structures"
echo "────────────────────────────────────"

compare "list make(1000)" "" "(let loop ((n 1000) (acc '())) (if (= n 0) (length acc) (loop (- n 1) (cons n acc))))"
compare "list reverse(1000)" "" "(let ((lst (let loop ((n 1000) (acc '())) (if (= n 0) acc (loop (- n 1) (cons n acc)))))) (length (reverse lst)))"
compare "list map(1000)" "" "(let ((lst (let loop ((n 1000) (acc '())) (if (= n 0) acc (loop (- n 1) (cons n acc)))))) (length (map (lambda (x) (* x 2)) lst)))"
compare "vector sum(1000)" "" "(let ((v (make-vector 1000 42))) (let loop ((i 999) (s 0)) (if (< i 0) s (loop (- i 1) (+ s (vector-ref v i))))))"

# Numeric
echo ""
echo "Numeric"
echo "────────────────────────────────────"

compare "sum-to(10000)" "" "(let loop ((n 10000) (acc 0)) (if (= n 0) acc (loop (- n 1) (+ acc n))))"
compare "factorial(50)" "" "(let loop ((n 50) (acc 1)) (if (= n 0) acc (loop (- n 1) (* acc n))))"
compare "float-sum(1000)" "" "(let loop ((n 1000) (acc 0.0)) (if (= n 0) acc (loop (- n 1) (+ acc 1.5))))"

echo ""
echo "═══════════════════════════════════════════════════════════════"
echo "  Speedup > 1.0x means VM is faster"
echo "═══════════════════════════════════════════════════════════════"
