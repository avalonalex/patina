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
#     compared its numbers passed while they moved). A row may hold `-` for
#     its status and passed count, and then holds only its total and that
#     the suite reached a tally, pass or fail: `time`, whose passed count
#     measures wall-clock time rather than the collector;
#   - its process exited other than as its tally says: 0 after a pass or a
#     fail, 1 after a run cut short. A crash after the tally was printed,
#     a signal or an abort without a panic message, would otherwise pass;
#   - its log has a panic: a stale-reference, deferral or retired-register
#     check fired, or anything else did;
#   - it timed out;
#   - it ran fewer collections than the row's minimum, or its process left no
#     collection record, or the record says another mode, or another backend
#     than the lane's: a lane that did not collect tested nothing (#5's
#     lesson), and a tree-walker job that ran the VM would pass on nearly
#     every row. The record is PATINA_GC_COUNT_DIR's
#     (crates/patina-core/src/heap/gc.rs); the minimum is half the count
#     measured when the row was pinned, which is deterministic for a suite,
#     and at least 1.
# A pinned row with no suite behind it fails too, when the whole lane runs.
#
# A suite whose tally differs from its row is run once more without stress,
# and the lane says which of two things happened. If the plain run gives the
# stress run's tally, the collector is not the cause: a change moved the
# tally and did not re-pin the row, which it must do in the same pull
# request (AGENTS.md; scripts/run_larceny_tests.sh warns), or the row does
# not hold on this machine (below). If it does not, stress changed what the
# suite computes, which is this lane's finding. Either way the lane fails, and it prints what failed without
# quoting the suite (it is LGPL): the classifier's detail and the failing
# assertions' permalinks, as the tracked reports give them.
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
# (r6rs ...) lane took 20-22 s and 32-36 s. On the nightly's ubuntu runner
# (2026-10-03) each took about 2.2 times as long: the VM's R7RS lane 434 s
# (text 180 s), the tree-walker's 1774 s (stream 783 s, text 414 s, char
# 394 s), and the (r6rs ...) lanes 24 s and 51 s. The baseline was measured
# on the Mac and checked on the runner, whose tallies it holds: flonum fails
# one more assertion on x86_64 than on arm64 (#634). A row that differs on a
# platform and is not the collector's doing (the plain re-run above says
# which) is re-pinned from the runner, with nightly.yml's update-baseline
# input.
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
# results, minimums included (half this run's collections, and never below
# 1), keeping a `-` a row holds. It rewrites nothing if a run failed in a
# way no baseline can hold: a panic, a timeout, an exit status its tally
# does not explain, no collection record, another mode or backend, or no
# collections at all. A tally it pins must be a plain run's, without stress:
# where a suite's tally differs from its row, or it has none, the suite runs
# again without stress, and a stress run that differs from that is this
# lane's finding, not a new baseline. A suite's tally alone can also be
# re-pinned by hand: its status, passed and total are those of
# scripts/run_larceny_tests.sh's report.
#
# Environment: LARCENY_TESTS_DIR as for run_larceny_tests.sh (the checkout
# must be at its pinned commit, which this lane requires rather than warns
# about); PATINA_BIN (default target/release/patina); LARCENY_TEST_TIMEOUT,
# seconds per suite (default 600); LARCENY_GC_STRESS_OUT, a directory to
# keep the logs and results.tsv in (default: a temporary one, removed at
# exit). results.tsv has a row per suite run: lane, suite, status, passed,
# total, collections, interval and seconds.
set -euo pipefail

cd "$(dirname "$0")/.."

# Each run sets the one GC variable it means; one left exported in the shell
# would change what the lane measures.
unset PATINA_GC PATINA_GC_STRESS PATINA_GC_ZEAL PATINA_GC_COUNT_DIR
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
        -h | --help) sed -n '2,86p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
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

# Whether a tally (status passed total) is the one a row pins (its status,
# passed and total, either of the first two possibly `-`): see the header.
holds() {
    local want_status=$1 want_passed=$2 want_total=$3 status=$4 passed=$5 total=$6
    [ "$total" = "$want_total" ] || return 1
    case "$want_status" in
        -) case "$status" in pass | fail) ;; *) return 1 ;; esac ;;
        *) [ "$status" = "$want_status" ] || return 1 ;;
    esac
    [ "$want_passed" = - ] || [ "$passed" = "$want_passed" ]
}

# A suite's tally without stress, "status passed total", from a run in
# directory $2 by the same binary and runner.
plain_tally() {
    local suite=$1 dir=$2 log
    mkdir -p "$dir"
    PATINA_BIN="$BIN" LARCENY_REPORT_DIR="$dir" LARCENY_TEST_TIMEOUT="$TIMEOUT" \
        LARCENY_TESTS_DIR="$LARCENY_TESTS_DIR" \
        ./scripts/run_larceny_tests.sh "${LANE_FLAGS[@]+"${LANE_FLAGS[@]}"}" "$suite" \
        > "$dir/runner.txt" 2>&1 || true
    log="$dir/$LOG_SUBDIR/${suite//\//_}.txt"
    if [ -f "$log" ]; then
        python3 scripts/larceny_report.py --classify "$log" | cut -f1-3 | tr '\t' ' '
    else
        echo "missing 0 0"
    fi
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

    # The exit status the runner appended, against the one the tally
    # implies; a suite with no tally fails on the tally instead.
    if [ -f "$log" ]; then
        rc=$(sed -n 's/^--- run_larceny_tests\.sh: exit status \([0-9][0-9]*\) ---$/\1/p' "$log" \
            | tail -1)
        case "$status" in
            pass | fail) want_rc=0 ;;
            truncated) want_rc=1 ;;
            *) want_rc="" ;;
        esac
        if [ -n "$want_rc" ] && [ "${rc:-none}" != "$want_rc" ]; then
            problems+=("exit status ${rc:-missing from the log} after a $status tally, which exits $want_rc")
        fi
    fi

    # The collection record: one process, under this interval, running this
    # lane's backend.
    collections=0
    records=("$run"/count/gc-count.*)
    if [ ! -e "${records[0]}" ]; then
        problems+=("no collection record: PATINA_GC_COUNT_DIR never reached a process")
    else
        for record in "${records[@]}"; do
            if ! grep -q " env=PATINA_GC_STRESS=$interval " "$record"; then
                problems+=("the process ran under another mode: $(cat "$record")")
            fi
            if ! grep -q " backends=$BACKEND " "$record"; then
                problems+=("the process ran other than the $BACKEND backend: $(cat "$record")")
            fi
            n=$(sed -n 's/.* collections=0*\([0-9][0-9]*\)$/\1/p' "$record")
            collections=$((collections + ${n:-0}))
        done
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$LANE_NAME" "$suite" "$status" "$passed" \
        "$total" "$collections" "$interval" "$took" >> "$RESULTS"

    row=$(pinned_row "$suite")
    differs=0
    if [ -z "$row" ]; then
        if [ "$UPDATE" -eq 0 ]; then
            problems+=("no pinned row for $LANE_NAME $suite in $BASELINE")
        else
            differs=1
        fi
    else
        IFS=$'\t' read -r want_status want_passed want_total min_collections <<< "$row"
        if ! holds "$want_status" "$want_passed" "$want_total" "$status" "$passed" "$total"; then
            differs=1
            if [ "$UPDATE" -eq 0 ]; then
                problems+=("tally $status $passed/$total, pinned $want_status $want_passed/$want_total")
            fi
        fi
        if [ "$UPDATE" -eq 0 ] && [ "$collections" -lt "$min_collections" ]; then
            problems+=("$collections collections, pinned minimum $min_collections")
        fi
    fi
    if [ "$UPDATE" -eq 1 ] && [ "$collections" -eq 0 ]; then
        problems+=("no collections: a run that did not collect is never a baseline")
    fi
    # Told apart from a run without stress: see the header.
    if [ "$differs" -eq 1 ]; then
        plain=$(plain_tally "$suite" "$run/plain")
        read -r plain_status plain_passed plain_total <<< "$plain"
        if [ "$plain" = "$status $passed $total" ]; then
            if [ "$UPDATE" -eq 0 ]; then
                problems+=("without stress the same $plain_status $plain_passed/$plain_total: not the collector; the row is out of date, or does not hold on this machine: re-pin it (see the header)")
            fi
        else
            problems+=("without stress $plain_status $plain_passed/$plain_total: stress changed the tally")
        fi
    fi

    line="$suite at $interval: $status $passed/$total, $collections collections, ${took}s"
    if [ ${#problems[@]} -eq 0 ]; then
        echo "OK   $line"
    else
        echo "FAIL $line"
        for p in "${problems[@]}"; do echo "       $p"; done
        # What failed, without quoting the suite: the classifier's detail,
        # and each failing assertion's permalink from the runner's report.
        [ -z "$detail" ] || echo "       detail: $detail"
        links=$(grep -E '^- (\[|\(not located\))' "$run/$LOG_SUBDIR.md" 2> /dev/null || true)
        if [ -n "$links" ]; then
            echo "       failing assertions:"
            head -20 <<< "$links" | sed 's/^/         /'
            count=$(wc -l <<< "$links")
            [ "$count" -le 20 ] || echo "         and $((count - 20)) more"
        fi
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
    echo "Not rewriting $BASELINE: a run above failed in a way no baseline can hold"
    echo "(see the header)."
elif [ "$UPDATE" -eq 1 ]; then
    python3 - "$BASELINE" "$RESULTS" << 'EOF'
import sys
baseline, results = sys.argv[1], sys.argv[2]
new = {}
for line in open(results):
    lane, suite, status, passed, total, collections = line.rstrip("\n").split("\t")[:6]
    # Half the count, as the header says, and never 0: a minimum of 0
    # passes a run that did not collect.
    minimum = max(1, int(collections) // 2)
    new[(lane, suite)] = [lane, suite, status, passed, total, str(minimum)]
out, seen = [], set()
for line in open(baseline):
    fields = line.rstrip("\n").split("\t")
    key = tuple(fields[:2])
    if not line.startswith("#") and key in new:
        row = new[key]
        # A field the row does not hold stays unheld.
        for i in (2, 3):
            if fields[i] == "-":
                row[i] = "-"
        out.append("\t".join(row) + "\n")
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
