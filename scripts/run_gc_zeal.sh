#!/usr/bin/env bash
# GC zeal lane (#625, docs/GC_DESIGN.md §11): the control-flow suite files must
# produce identical output with GC off (PATINA_GC=0) and under zeal
# (PATINA_GC_ZEAL=entry), which collects at every outermost safe point,
# allocation or not, on both backends.
#
# Stress collects only once allocations cross its interval, so a stretch of
# instructions that allocates nothing never collects under it, and most pcs
# never have their liveness maps checked. Zeal collects before every
# instruction (VM) and every trampoline step (tree-walker) that a collection
# may run at. Run it against a check build, where retirement fills each
# register a map calls dead with DEAD_SLOT and the VM panics on reading one:
# a debug build, or release built with `--features patina-core/gc-check`.
# CI's zeal job runs the release check build.
#
# Zeal costs about 7x stress 1, so it runs on a subset, never the whole chibi
# suite (882-920 s on the VM alone, measured 2026-10-01): the files of
# crates/patina-tests/tests/scheme/control/ except tail-recursion.scm, whose
# long loops that allocate little collect at nearly every instruction (277 s on
# the VM and 627 s on the tree-walker by itself, against 176 s and 275 s for
# the other ten files together).
#
# Before the lane, a probe checks that the binary honours zeal at all: the
# loop of `crates/patina-repl/tests/gc_zeal.rs`, 1000 iterations that
# allocate nothing, must collect at least 1000 times under zeal and fewer
# than 100 times with no GC variable set, on both backends, or the script
# fails before the lane starts. The lane's own check cannot tell: each run
# must have collected (the script appends a form to each file that writes the
# run's collection count, checks it is non-zero under zeal, and removes that
# line before comparing), but every file collects at least once under the
# default GC too, while it loads SRFI 64, so a binary that ignored
# PATINA_GC_ZEAL would pass it. The probe also fails a zeal mode that had
# decayed into stress, which collects only after allocations. gc_zeal.rs runs
# the same loop in ci.yml's Test Suite, against that job's binary rather than
# the one this lane tests; keep the two programs the same.
#
# Usage: scripts/run_gc_zeal.sh [path-to-patina-binary]
# Default binary: target/release/patina (build it first).
set -euo pipefail

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
# The files run from a scratch directory, since SRFI 64 writes its log into
# the working directory.
case "$BIN" in
    /*) ;;
    *) BIN="$PWD/$BIN" ;;
esac
SUBSET=crates/patina-tests/tests/scheme/control
EXCLUDE=tail-recursion.scm
MARK="gc-zeal-lane collections:"

case "$("$BIN" --version)" in
    *"(gc-check)") ;;
    *) echo "warning: $BIN is not a check build: a retired register read by mistake"
       echo "         reads as a legal value there instead of panicking." ;;
esac

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

# The probe: gc_zeal.rs's loop, which allocates nothing, writing the
# collections it ran through, measured by `gc-stats` before and after it.
PROBE_ITERATIONS=1000
PROBE_DEFAULT_MAX=100
PROBE="(import (scheme base) (scheme write) (patina debug))
(define (spin n) (if (> n 0) (spin (- n 1))))
(define (collections) (cdr (assq 'collections (gc-stats))))
(define before (collections))
(spin $PROBE_ITERATIONS)
(define after (collections))
(write (- after before))
(newline)"
mkdir -p "$OUT/probe"
printf '%s\n' "$PROBE" > "$OUT/probe/spin.scm"

# Run the probe on the backend that $1 selects ("" for the VM), with the GC
# variables given in the remaining arguments (none for the default GC).
run_probe() {
    local backend_flag=$1
    shift
    (cd "$OUT/probe" && env "$@" "$BIN" $backend_flag spin.scm) \
        2> "$OUT/probe/stderr.txt"
}

probe_fail=0
for backend_flag in "" "--tree-walker"; do
    name=${backend_flag:-"vm"}
    name=${name#--}
    ok=1 zeal="" default=""
    for mode in zeal default; do
        rc=0
        if [ "$mode" = zeal ]; then
            count=$(run_probe "$backend_flag" PATINA_GC_ZEAL=entry) || rc=$?
            vars="PATINA_GC_ZEAL=entry"
        else
            count=$(run_probe "$backend_flag") || rc=$?
            vars="no GC variable"
        fi
        if [ "$rc" -ne 0 ]; then
            echo "FAIL zeal probe, $name: the run under $vars exited $rc"
            tail -5 "$OUT/probe/stderr.txt"
            ok=0
            continue
        fi
        case "$count" in
            "" | *[!0-9]*)
                echo "FAIL zeal probe, $name: the run under $vars printed '$count', not a count"
                ok=0
                continue ;;
        esac
        if [ "$mode" = zeal ]; then
            zeal=$count
            if [ "$count" -lt "$PROBE_ITERATIONS" ]; then
                echo "FAIL zeal probe, $name: $count collections under $vars across" \
                     "$PROBE_ITERATIONS iterations that allocate nothing, want at least" \
                     "$PROBE_ITERATIONS; $BIN is not honouring zeal, and the lane would" \
                     "test some other mode"
                ok=0
            fi
        else
            default=$count
            if [ "$count" -ge "$PROBE_DEFAULT_MAX" ]; then
                echo "FAIL zeal probe, $name: $count collections with $vars, want fewer" \
                     "than $PROBE_DEFAULT_MAX; the loop allocates, or the default GC" \
                     "collects without allocations, and the probe cannot tell zeal from it"
                ok=0
            fi
        fi
    done
    if [ "$ok" -eq 1 ]; then
        echo "OK   zeal probe, $name: $zeal collections under zeal, $default with no GC variable"
    else
        probe_fail=1
    fi
done
if [ "$probe_fail" -ne 0 ]; then
    echo "FAIL the zeal probe failed, so the lane would not be testing zeal; not running it"
    exit 1
fi

# The suffix that reports the run's collections, after the file's own forms.
FOOTER="(import (scheme base) (scheme write) (patina debug))
(display \"$MARK \")
(write (cdr (assq 'collections (gc-stats))))
(newline)"

files=()
for file in "$SUBSET"/*.scm; do
    [ "$(basename "$file")" = "$EXCLUDE" ] && continue
    files+=("$file")
done
if [ "${#files[@]}" -eq 0 ]; then
    echo "FAIL no suite files under $SUBSET"
    exit 1
fi

fail=0
for backend_flag in "" "--tree-walker"; do
    name=${backend_flag:-"vm"}
    name=${name#--}
    for file in "${files[@]}"; do
        base=$(basename "$file" .scm)
        run="$OUT/$name-$base"
        mkdir -p "$run"
        { cat "$file"; printf '\n%s\n' "$FOOTER"; } > "$run/$base.scm"

        rc_off=0 rc_zeal=0
        (cd "$run" && PATINA_GC=0 "$BIN" $backend_flag "$base.scm") \
            > "$run/off.txt" 2>&1 || rc_off=$?
        start=$SECONDS
        (cd "$run" && PATINA_GC_ZEAL=entry "$BIN" $backend_flag "$base.scm") \
            > "$run/zeal.txt" 2>&1 || rc_zeal=$?
        took=$((SECONDS - start))

        label="$name $base"
        ok=1
        for lane in off zeal; do
            rc_var="rc_$lane"
            if [ "${!rc_var}" -ne 0 ]; then
                echo "FAIL $label: the $lane run exited ${!rc_var}"
                tail -5 "$run/$lane.txt"
                ok=0
            fi
            # The SRFI 64 summary: the runner ran and reached its end. Not
            # anchored: a file's own output may leave a line unfinished.
            if ! grep -q '# of expected passes' "$run/$lane.txt"; then
                echo "FAIL $label: the $lane run printed no SRFI 64 summary"
                ok=0
            fi
        done
        collections=$(sed -n "s/^$MARK \([0-9][0-9]*\)$/\1/p" "$run/zeal.txt")
        if [ -z "$collections" ] || [ "$collections" -eq 0 ]; then
            echo "FAIL $label: the zeal run reported ${collections:-no} collections"
            ok=0
        fi
        grep -v "^$MARK " "$run/off.txt" > "$run/off.cmp" || true
        grep -v "^$MARK " "$run/zeal.txt" > "$run/zeal.cmp" || true
        if ! diff -u "$run/off.cmp" "$run/zeal.cmp" > "$run/diff.txt"; then
            echo "FAIL $label: zeal diverges from GC-off:"
            head -40 "$run/diff.txt"
            ok=0
        fi
        if [ "$ok" -eq 1 ]; then
            echo "OK   $label: byte-identical to GC-off, $collections collections, ${took}s"
        else
            fail=1
        fi
    done
done

exit $fail
