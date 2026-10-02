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
# Each run must also have collected: the script appends a form to each file
# that writes the run's collection count, checks it is non-zero under zeal,
# and removes that line before comparing. A zeal mode that had decayed into
# stress would still collect here; `crates/patina-repl/tests/gc_zeal.rs` is
# the control for that.
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
