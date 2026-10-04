#!/usr/bin/env bash
# GC differential lane (docs/GC_DESIGN.md §11): the chibi suite must produce
# identical output with GC opted out (PATINA_GC=0), under the default adaptive
# mode, and under GC stress, on both backends. Any divergence is a
# lost root or a reclamation bug, never an acceptable difference.
#
# Two things are normalised away before comparing, and only two: the ANSI colour
# codes (chibi test) emits, and the wall-clock duration it prints per section.
# The duration is genuinely nondeterministic -- two runs of the *same* binary
# differ -- so leaving it in would make the lane fail always rather than never.
# Everything else, including every pass/fail count and every reported value, is
# compared exactly.
#
# Usage: scripts/run_gc_differential.sh [path-to-patina-binary]
# Default binary: target/release/patina (build it first).
#
# Run against a build with the stale-reference checks (#621) when touching GC
# internals: a debug build, or release built with
# `--features patina-core/gc-check`. A plain release build turns a
# use-after-free into a confusing type error at an unrelated call site, or
# into nothing at all once the slot is reused, while a check build panics at
# the first use of the stale value, or at the next collection that reaches
# it. CI's release lane runs the check build at stress 1.
set -euo pipefail

# Apply before bootstrap, including the generated reclamation probes below.
export PATINA_ISOLATED_LIBRARIES=1
# Each run sets the one GC variable it means, so clear all three first. Zeal
# wins over stress and stress over PATINA_GC=0 (GcMode::from_env): one left
# exported in the shell would turn the GC-off reference run into a collecting
# one, and the comparison would prove nothing.
unset PATINA_GC PATINA_GC_STRESS PATINA_GC_ZEAL
# Descriptor pressure posts a collection once min(128, soft RLIMIT_NOFILE / 4)
# file ports opened since the last one are still open (#607), so the soft
# limit decides when those collections come. Pin the threshold at 128, as
# any limit of 512 or more gives it: raise a lower one, such as the 256 a
# macOS shell starts with, to 1024, and leave a higher one as it is.
nofile=$(ulimit -Sn)
if [ "$nofile" != unlimited ] && [ "$nofile" -lt 512 ]; then
    ulimit -Sn 1024 || {
        echo "error: cannot raise the descriptor limit from $nofile to 1024 (hard limit $(ulimit -Hn))" >&2
        exit 1
    }
fi

cd "$(dirname "$0")/.."
BIN="${1:-target/release/patina}"

# How often the stress lane collects, in allocations. `1` collects at nearly
# every safe point and is the most thorough setting, but the suite now runs
# under upstream (chibi test), which allocates far more per test than the
# hand-written subset it replaced -- at `1` the suite goes from 0.15s to 103s,
# and the debug lane from minutes to over half an hour.
#
# 16 keeps collection tens of thousands of times more frequent than the
# adaptive default on small objects (which collects after max(8 MiB, 2 x
# live) bytes, half a million pairs at the floor) for 13x less runtime.
# Override to 1 when hunting a specific lost root; CI's release lane does, on
# the gc-check build, where it costs a few minutes.
#
# Note the reclamation proof below asserts >1000 collections over 20k
# allocations, so it holds only while this stays <= 16.
STRESS="${PATINA_GC_STRESS_INTERVAL:-16}"
SUITE=scheme_tests/chibi/r7rs-tests.scm
# The suite reports through (chibi test), which Patina supplies from test-lib/
# rather than bundling (see test-lib/README.md).
SUPPLIED=(-A test-lib)
# -k so the suite reaches its tally even if an error escapes to top level; each
# lane's status is captured below and still fails the lane.
RUN_ARGS=(-k "${SUPPLIED[@]}")

# Every lane must show the framework's grand total, and the *expected* total,
# before anything is diffed.
#
# This lane compares runs for *equality*, so it passes hardest when both sides
# fail the same way. Measured: with the -A above removed, patina reports
# `Library (chibi test) not found`, keeps evaluating, emits 4537 deterministic
# lines and exits 0 -- so all three lanes are byte-identical, every diff below
# reports OK, and the job goes green having exercised no GC behaviour at all.
# A missing root is only the cheapest way to reach that state; a suite that
# aborts midway reaches it too, which is why this asserts the tally rather
# than the directory.
#
# And the tally alone is still not enough: (chibi test) honours TEST_FILTER /
# TEST_GROUP_FILTER / TEST_GROUP_REMOVE from the environment, and a truncated
# suite file has the same effect, so a two-assertion run also prints a
# well-formed "2 out of 2 ... tests passed" and diffs clean. Pin the count, as
# run_chibi_tests.sh does with EXPECTED_TOTAL and for the same reason. Update
# both deliberately when the suite grows.
EXPECTED_TOTAL=1226
assert_suite_ran() {
    local label="$1" file="$2" total
    total=$(awk '/^[0-9]+ out of [0-9]+ .*tests passed/ { print $4; exit }' "$file")
    if [ -z "$total" ]; then
        echo "FAIL $label: no suite tally in the output -- the run did not"
        echo "     complete, so comparing it to another lane proves nothing."
        head -3 "$file"
        return 1
    fi
    if [ "$total" -ne "$EXPECTED_TOTAL" ]; then
        echo "FAIL $label: suite reported $total tests, expected $EXPECTED_TOTAL."
        echo "     A filtered or truncated run diffs clean while exercising"
        echo "     almost nothing, so the count is pinned, not just its presence."
        return 1
    fi
    return 0
}
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

# Strip colour, and blank the per-section duration -- the only nondeterministic
# field in the output.
normalise() {
    sed -e 's/\x1b\[[0-9;]*m//g' \
        -e 's/ in [0-9][0-9.e-]* seconds\./ in TIME seconds./'
}

fail=0
for backend_flag in "" "--tree-walker"; do
    name=${backend_flag:-"vm"}
    name=${name#--}

    # Each lane's status is captured rather than left to `set -e`: the tally
    # check below says why a run failed, which an abort at this line would not,
    # and a status that is non-zero after a full tally, such as a crash on the
    # way out or an error -k carried on past, still fails the lane after it.
    rc_off=0 rc_default=0 rc_stress=0
    PATINA_GC=0 "$BIN" $backend_flag "${RUN_ARGS[@]}" "$SUITE" 2>&1 | normalise > "$OUT/$name-off.txt" || rc_off=$?
    "$BIN" $backend_flag "${RUN_ARGS[@]}" "$SUITE" 2>&1 | normalise > "$OUT/$name-default.txt" || rc_default=$?
    PATINA_GC_STRESS="$STRESS" "$BIN" $backend_flag "${RUN_ARGS[@]}" "$SUITE" 2>&1 | normalise > "$OUT/$name-stress.txt" || rc_stress=$?

    for lane in off default stress; do
        assert_suite_ran "$name $lane lane" "$OUT/$name-$lane.txt" || fail=1
        rc_var="rc_$lane"
        if [ "${!rc_var}" -ne 0 ]; then
            echo "FAIL $name $lane lane: patina exited ${!rc_var}"
            fail=1
        fi
    done

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
    # adaptive mode actually collect and reclaim on churn workloads. Each
    # proof is in bytes (#606), and each requires `bytes-reclaimed` to have
    # grown, which only a collection that freed something does, so none can
    # pass without collecting.
    #
    # The stress proof: 20000 conses, each garbage at once, under stress.
    # What they allocated must have been reclaimed, to within the last
    # interval's worth, by more than a thousand collections. Measured as
    # deltas across the churn, so the bootstrap's own allocation, which grows
    # whenever lib/scheme grows, is not what is being bounded.
    cat > "$OUT/churn-stress.scm" <<'EOF'
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(define before (gc-stats))
(define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
(churn 20000)
(let* ((stats (gc-stats))
       (delta (lambda (key) (- (cdr (assq key stats)) (cdr (assq key before)))))
       (collections (cdr (assq 'collections stats)))
       (allocated (delta 'bytes-allocated))
       (reclaimed (delta 'bytes-reclaimed)))
  (if (and (> collections 1000) (> reclaimed 0) (>= (* 10 reclaimed) (* 9 allocated)))
      (begin (display "stress reclamation ok: ") (write collections)
             (display " collections reclaimed ") (write reclaimed)
             (display " of the ") (write allocated)
             (display " bytes 20000 churned conses allocated") (newline))
      (begin (display "STRESS RECLAMATION BROKEN (reclaimed ") (write reclaimed)
             (display " of ") (write allocated) (display " bytes): ")
             (write stats) (newline)
             (exit 1))))
EOF
    if PATINA_GC_STRESS="$STRESS" "$BIN" $backend_flag "$OUT/churn-stress.scm"; then
        echo "OK   $name stress reclamation proof"
    else
        echo "FAIL $name stress lane did not actually collect"
        fail=1
    fi

    # The default-mode proof: 20000 vectors of 1000 elements, each garbage at
    # once, about 160 MB. The adaptive trigger counts bytes, so the default
    # mode must have collected on its own, reclaimed most of it, and kept
    # what the heap holds (`committed-bytes`) under a quarter of what the
    # churn allocated. A trigger that counted objects would not collect at
    # all: 20000 allocations are under its old floor of 65536 (#606).
    cat > "$OUT/churn-default.scm" <<'EOF'
(import (scheme base) (scheme write) (scheme process-context) (patina debug))
(define before (gc-stats))
(define (churn n) (if (> n 0) (begin (make-vector 1000 n) (churn (- n 1)))))
(churn 20000)
(let* ((stats (gc-stats))
       (delta (lambda (key) (- (cdr (assq key stats)) (cdr (assq key before)))))
       (collections (delta 'collections))
       (allocated (delta 'bytes-allocated))
       (reclaimed (delta 'bytes-reclaimed))
       (committed (cdr (assq 'committed-bytes stats))))
  (if (and (> collections 0) (> reclaimed 0)
           (>= (* 2 reclaimed) allocated) (< (* 4 committed) allocated))
      (begin (display "default-mode reclamation ok: ") (write collections)
             (display " collections reclaimed ") (write reclaimed)
             (display " of ") (write allocated)
             (display " bytes, ") (write committed)
             (display " committed") (newline))
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
