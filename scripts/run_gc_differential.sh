#!/usr/bin/env bash
# GC differential lane (docs/GC_DESIGN.md §11): the chibi suite must produce
# byte-identical output with GC opted out (PATINA_GC=0), under the default
# adaptive mode, and under PATINA_GC_STRESS=1, on both backends. Any
# divergence is a lost root or a reclamation bug, never an acceptable
# difference.
#
# Usage: scripts/run_gc_differential.sh [path-to-patina-binary]
# Default binary: target/release/patina (build it first).
#
# Run against a debug build too when touching GC internals — release turns a
# use-after-free into a confusing type error at an unrelated call site, while
# the debug build's poison assertions panic at the exact accessor.
set -euo pipefail

cd "$(dirname "$0")/.."
BIN="${1:-target/release/patina}"
SUITE=scheme_tests/chibi/r7rs-tests.scm
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

fail=0
for backend_flag in "" "--tree-walker"; do
    name=${backend_flag:-"vm"}
    name=${name#--}

    PATINA_GC=0 "$BIN" $backend_flag "$SUITE" > "$OUT/$name-off.txt" 2>&1
    "$BIN" $backend_flag "$SUITE" > "$OUT/$name-default.txt" 2>&1
    PATINA_GC_STRESS=1 "$BIN" $backend_flag "$SUITE" > "$OUT/$name-stress.txt" 2>&1

    for lane in default stress; do
        if diff -u "$OUT/$name-off.txt" "$OUT/$name-$lane.txt" > "$OUT/diff.txt"; then
            echo "OK   $name $lane lane: byte-identical to GC-off"
        else
            echo "FAIL $name $lane lane diverges from GC-off:"
            head -40 "$OUT/diff.txt"
            fail=1
        fi
    done

    # The lanes above prove nothing if collection never ran (a broken env-var
    # path would pass vacuously): assert both the stress lane and the default
    # adaptive mode actually collect and reclaim on churn workloads.
    cat > "$OUT/churn-stress.scm" <<'EOF'
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
(churn 20000)
(let* ((stats (gc-stats))
       (collections (cdr (assq 'collections stats)))
       (pairs (cdr (assq 'pairs stats))))
  (if (and (> collections 1000) (< pairs 10000))
      (begin (display "stress reclamation ok: ") (write collections)
             (display " collections, ") (write pairs)
             (display " pairs") (newline))
      (begin (display "STRESS RECLAMATION BROKEN: ") (write stats) (newline)
             (exit 1))))
EOF
    if PATINA_GC_STRESS=1 "$BIN" $backend_flag "$OUT/churn-stress.scm"; then
        echo "OK   $name stress reclamation proof"
    else
        echo "FAIL $name stress lane did not actually collect"
        fail=1
    fi

    # 200k allocations cross the 65 536 adaptive floor: the default mode must
    # have collected on its own and kept the arena bounded.
    cat > "$OUT/churn-default.scm" <<'EOF'
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
(churn 200000)
(let* ((stats (gc-stats))
       (collections (cdr (assq 'collections stats)))
       (pairs (cdr (assq 'pairs stats))))
  (if (and (> collections 0) (< pairs 150000))
      (begin (display "default-mode reclamation ok: ") (write collections)
             (display " collections, arena ") (write pairs)
             (display " pairs") (newline))
      (begin (display "DEFAULT MODE DID NOT COLLECT: ") (write stats) (newline)
             (exit 1))))
EOF
    if "$BIN" $backend_flag "$OUT/churn-default.scm"; then
        echo "OK   $name default-mode reclamation proof"
    else
        echo "FAIL $name default mode did not collect on its own"
        fail=1
    fi
done

exit $fail
