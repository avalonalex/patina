#!/usr/bin/env bash
# The nightly GC stress lane over Larceny's suites (#626; docs/TEST_ORGANIZATION.md,
# "GC lanes"): each suite runs under PATINA_GC_STRESS in a check build, and its
# tally must be the one pinned in scheme_tests/reports/larceny_gc_stress.tsv.
#
# The suites are run by scripts/run_larceny_tests.sh, one suite per call, so
# that each gets its own interval and its own collection count. That script
# exits 1 whenever a suite is not fully clean, and some are not today, so its
# status is not this lane's verdict. A suite fails the lane when:
#   - its tally (status, passed, total) differs from the pinned row: GC must
#     not change what a program computes (#201's lesson: a lane that never
#     compared its numbers passed while they moved);
#   - its log has a panic: a stale-reference, deferral or retired-register
#     check fired, or anything else did;
#   - it timed out;
#   - it ran fewer collections than the row's minimum, or its process left no
#     collection record, or the record says another mode: a lane that did not
#     collect tested nothing (#5's lesson). The record is PATINA_GC_COUNT_DIR's
#     (crates/patina-core/src/heap/gc.rs); the minimum is half the count
#     measured when the row was pinned, which is deterministic for a suite.
# A pinned row with no suite behind it fails too, when the whole lane runs.
#
# Intervals. A collection costs time in proportion to the live heap, so a
# stress run costs about allocations x live / interval. Most suites run at
# 16, as the chibi lanes do, and so does every (r6rs ...) suite. Five R7RS
# suites that allocate heavily over a large live heap run at 4096, which
# still collects every few thousand allocations: on the VM at 16 they took
# from 300 s to over 900 s each (2026-10-01, #626). ephemeron is left out
# until #609: every collection rescans the pending ephemerons, and it did not
# finish in 900 s at 4096. Measured 2026-10-02 on an M-series Mac under other
# load, the R7RS lane took 307 s on the VM, the slowest suites text (165 s
# at 16), stream (56 s) and char (54 s), and 855-1008 s on the tree-walker,
# with stream (338-486 s), text (276-281 s) and char (155 s); the
# (r6rs ...) lane took 20-22 s and 32-36 s.
#
# Run it against a check build, where a stale reference panics at its first
# use instead of reading whatever reused the slot: release built with
# `--features patina-core/gc-check`. The script refuses any other.
#
# Usage:
#   scripts/run_larceny_gc_stress.sh [--tree-walker] [--r6rs] [suite ...]
#   scripts/run_larceny_gc_stress.sh --update-baseline [...]   # re-pin rows
#
# --update-baseline rewrites the rows of the suites it ran from this run's
# results, minimums included, unless a run failed in a way no baseline can
# hold (a panic, a timeout, no collection record). The tallies it pins must
# be those of a plain release run, without stress
# (scripts/run_larceny_tests.sh); check the diff against one before
# committing it, since a stress run that differs from it is this lane's
# finding, not a new baseline.
#
# Environment: LARCENY_TESTS_DIR as for run_larceny_tests.sh (the checkout
# must be at its pinned commit, which this lane requires rather than warns
# about); PATINA_BIN (default target/release/patina); LARCENY_TEST_TIMEOUT,
# seconds per suite (default 600); LARCENY_GC_STRESS_OUT, a directory to
# keep the logs in (default: a temporary one, removed at exit).
set -euo pipefail

cd "$(dirname "$0")/.."

# Each run sets the one GC variable it means; one left exported in the shell
# would change what the lane measures.
unset PATINA_GC PATINA_GC_STRESS PATINA_GC_ZEAL PATINA_GC_COUNT_DIR

BASELINE=scheme_tests/reports/larceny_gc_stress.tsv
PINNED_COMMIT=fef550c7d3923deb7a5a1ccd5a628e54cf231c75
LARCENY_TESTS_DIR="${LARCENY_TESTS_DIR:-$HOME/Project/reference/larceny/test/R7RS/Lib}"
BIN="${PATINA_BIN:-target/release/patina}"
case "$BIN" in /*) ;; *) BIN="$PWD/$BIN" ;; esac
TIMEOUT="${LARCENY_TEST_TIMEOUT:-600}"

LANE_FLAGS=()
BACKEND=vm
LANE=r7rs
UPDATE=0
SUITES=()
for arg in "$@"; do
    case "$arg" in
        --tree-walker) LANE_FLAGS+=(--tree-walker); BACKEND=tree-walker ;;
        --r6rs) LANE_FLAGS+=(--r6rs); LANE=r6rs ;;
        --update-baseline) UPDATE=1 ;;
        -h | --help) sed -n '2,56p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        --*) echo "Unknown option: $arg" >&2; exit 2 ;;
        *) SUITES+=("$arg") ;;
    esac
done
LANE_NAME="$LANE-$BACKEND"
# The runner's log directory for this lane, under its report directory.
LOG_SUBDIR=larceny
[ "$LANE" = r6rs ] && LOG_SUBDIR="${LOG_SUBDIR}_r6rs"
[ "$BACKEND" = tree-walker ] && LOG_SUBDIR="${LOG_SUBDIR}_tree_walker"
case "$LANE" in
    r7rs) RUN_DIR=tests/scheme/run ;;
    r6rs) RUN_DIR=tests/r6rs/run ;;
esac

# The interval a suite runs at, or "-" for a suite left out of the lane.
interval_for() {
    case "$LANE_NAME:$1" in
        r7rs-*:ephemeron) echo - ;;
        r7rs-*:char | r7rs-*:flonum | r7rs-*:lazy | r7rs-*:sort | r7rs-*:stream) echo 4096 ;;
        *) echo 16 ;;
    esac
}

fail_setup() {
    echo "FAIL $*"
    exit 1
}

[ -x "$BIN" ] || fail_setup "no binary at $BIN; build it with" \
    "cargo build --release -p patina-repl --bin patina --features patina-core/gc-check"
version=$("$BIN" --version)
case "$version" in
    *"(gc-check)") ;;
    *) fail_setup "$BIN is not a check build ($version): a stale reference reads" \
        "whatever reused its slot there instead of panicking" ;;
esac
command -v python3 > /dev/null || fail_setup "python3 is required (scripts/larceny_report.py)"
[ -d "$LARCENY_TESTS_DIR/$RUN_DIR" ] || fail_setup "no Larceny suites at" \
    "$LARCENY_TESTS_DIR/$RUN_DIR; see scripts/run_larceny_tests.sh for how to fetch them"
actual=$(git -C "$LARCENY_TESTS_DIR" rev-parse HEAD 2> /dev/null || echo unknown)
[ "$actual" = "$PINNED_COMMIT" ] || fail_setup "the Larceny checkout is at $actual;" \
    "the baseline was measured at $PINNED_COMMIT"
[ -f "$BASELINE" ] || fail_setup "no baseline at $BASELINE"

ALL_SUITES=0
if [ ${#SUITES[@]} -eq 0 ]; then
    ALL_SUITES=1
    while IFS= read -r f; do
        f="${f#"$LARCENY_TESTS_DIR/$RUN_DIR/"}"
        SUITES+=("${f%.sps}")
    done < <(find "$LARCENY_TESTS_DIR/$RUN_DIR" -name '*.sps' | sort)
fi

if [ -n "${LARCENY_GC_STRESS_OUT:-}" ]; then
    OUT="$LARCENY_GC_STRESS_OUT"
    mkdir -p "$OUT"
else
    OUT=$(mktemp -d)
    trap 'rm -rf "$OUT"' EXIT
fi
RESULTS="$OUT/results.tsv"
: > "$RESULTS"

# The pinned row for a suite: "status passed total min_collections", or
# nothing.
pinned_row() {
    awk -F'\t' -v lane="$LANE_NAME" -v suite="$1" \
        '$1 == lane && $2 == suite { print $3 "\t" $4 "\t" $5 "\t" $6 }' "$BASELINE"
}

echo "GC stress lane: Larceny $LANE_NAME, $BIN ($version)"
echo "Suites: $LARCENY_TESTS_DIR/$RUN_DIR at $actual; per-suite budget ${TIMEOUT}s"
echo ""

fail=0
lane_start=$SECONDS
for suite in "${SUITES[@]}"; do
    interval=$(interval_for "$suite")
    if [ "$interval" = - ]; then
        echo "SKIP $suite: left out of the lane (see the header)"
        continue
    fi
    run="$OUT/${suite//\//_}"
    rm -rf "$run"
    mkdir -p "$run/count"
    start=$SECONDS
    # Not this lane's verdict: see the header.
    PATINA_GC_STRESS=$interval PATINA_GC_COUNT_DIR="$run/count" PATINA_BIN="$BIN" \
        LARCENY_REPORT_DIR="$run" LARCENY_TEST_TIMEOUT="$TIMEOUT" \
        LARCENY_TESTS_DIR="$LARCENY_TESTS_DIR" \
        ./scripts/run_larceny_tests.sh "${LANE_FLAGS[@]+"${LANE_FLAGS[@]}"}" "$suite" \
        > "$run/runner.txt" 2>&1 || true
    took=$((SECONDS - start))
    log="$run/$LOG_SUBDIR/${suite//\//_}.txt"
    problems=()

    status=missing passed=0 total=0 detail=""
    if [ -f "$log" ]; then
        IFS=$'\t' read -r status passed total detail \
            < <(python3 scripts/larceny_report.py --classify "$log") || true
        [ -n "$status" ] || status=unclassified
    else
        problems+=("the runner left no log; its output ends:")
        problems+=("$(tail -5 "$run/runner.txt")")
    fi
    if [ -f "$log" ] && grep -q "panicked at" "$log"; then
        problems+=("a panic: $(grep -m1 -A1 'panicked at' "$log" | tr '\n' ' ')")
    fi
    [ "$status" = timeout ] && problems+=("no result within ${TIMEOUT}s")

    # The collection record: one process, under this interval.
    collections=0
    records=("$run"/count/gc-count.*)
    if [ ! -e "${records[0]}" ]; then
        problems+=("no collection record: PATINA_GC_COUNT_DIR never reached a process")
    else
        for record in "${records[@]}"; do
            if ! grep -q " env=PATINA_GC_STRESS=$interval " "$record"; then
                problems+=("the process ran under another mode: $(cat "$record")")
            fi
            n=$(sed -n 's/.* collections=0*\([0-9][0-9]*\)$/\1/p' "$record")
            collections=$((collections + ${n:-0}))
        done
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$LANE_NAME" "$suite" "$status" "$passed" "$total" \
        "$collections" "$interval" >> "$RESULTS"

    row=$(pinned_row "$suite")
    if [ "$UPDATE" -eq 0 ]; then
        if [ -z "$row" ]; then
            problems+=("no pinned row for $LANE_NAME $suite in $BASELINE")
        else
            IFS=$'\t' read -r want_status want_passed want_total min_collections <<< "$row"
            if [ "$status $passed $total" != "$want_status $want_passed $want_total" ]; then
                problems+=("tally $status $passed/$total, pinned $want_status $want_passed/$want_total")
            fi
            if [ "$collections" -lt "$min_collections" ]; then
                problems+=("$collections collections, pinned minimum $min_collections")
            fi
        fi
    fi

    line="$suite at $interval: $status $passed/$total, $collections collections, ${took}s"
    if [ ${#problems[@]} -eq 0 ]; then
        echo "OK   $line"
    else
        echo "FAIL $line"
        for p in "${problems[@]}"; do echo "       $p"; done
        fail=1
    fi
done

if [ "$UPDATE" -eq 0 ] && [ "$ALL_SUITES" -eq 1 ]; then
    while IFS=$'\t' read -r suite; do
        if ! awk -F'\t' -v s="$suite" '$2 == s { found = 1 } END { exit !found }' "$RESULTS"; then
            echo "FAIL $suite: pinned in $BASELINE, but the lane ran no such suite"
            fail=1
        fi
    done < <(awk -F'\t' -v lane="$LANE_NAME" '$1 == lane { print $2 }' "$BASELINE")
fi

if [ "$UPDATE" -eq 1 ] && [ "$fail" -ne 0 ]; then
    echo ""
    echo "Not rewriting $BASELINE: a panic, a timeout or a missing collection record is"
    echo "never a baseline."
elif [ "$UPDATE" -eq 1 ]; then
    python3 - "$BASELINE" "$RESULTS" << 'EOF'
import sys
baseline, results = sys.argv[1], sys.argv[2]
new = {}
for line in open(results):
    lane, suite, status, passed, total, collections, _ = line.rstrip("\n").split("\t")
    new[(lane, suite)] = [lane, suite, status, passed, total, str(int(collections) // 2)]
out, seen = [], set()
for line in open(baseline):
    fields = line.rstrip("\n").split("\t")
    key = tuple(fields[:2])
    if not line.startswith("#") and key in new:
        out.append("\t".join(new[key]) + "\n")
        seen.add(key)
    else:
        out.append(line)
out += ["\t".join(row) + "\n" for key, row in new.items() if key not in seen]
open(baseline, "w").writelines(out)
EOF
    echo ""
    echo "Rewrote the rows of $LANE_NAME's suites in $BASELINE; review the diff."
fi

echo ""
echo "Lane time: $((SECONDS - lane_start))s"
exit $fail
