#!/usr/bin/env bash
# The per-PR GC stress lane (#626; docs/TEST_ORGANIZATION.md, "GC lanes"): the
# cargo test targets that exercise the collector, control flow and library
# loading, under PATINA_GC_STRESS=16 in a build with the stale-reference
# checks, and the suite files' driver, scheme_suite.rs, at 4096. The chibi
# lanes reach the collector only through the CLI and only with the chibi
# suite's programs; these targets drive it through the embedding API
# (Interpreter, Backend::eval, library loading from Rust) and with the shapes
# a GC bug has needed: an ephemeron holding a continuation (#130), an escape
# from a primitive, a continuation re-entered after a collection.
#
# Each target must pass, and must report at least the number of collections
# pinned for it below: a lane that passes without collecting has tested
# nothing, and four have (#5, #164, #200, #201). The count is the record
# PATINA_GC_COUNT_DIR asks for (crates/patina-core/src/heap/gc.rs), one per
# process, which must also say the process ran under this interval. A
# minimum is half the count measured when it was pinned (the counts are
# deterministic), and every one is above what the target makes without
# stress, so a run the variable did not reach fails here even if the record
# went missing some other way. Lower a minimum only with a measurement.
#
# Each target must also run all its tests: none filtered out, and at least
# the number pinned below, which is how many it has. A run cut short or
# filtered still passes and can still collect, and a GC lane passed that way
# once, with the chibi suite run two assertions deep (#201). Raise a pin when
# a target gains tests; lower one only with the tests it lost.
#
# One minimum is near zero, and that is its healthy count, not an oversight.
# library_loading does nearly all its work inside library loads, and a
# library load defers collection for as long as its unevaluated body exists
# (ParsedLibrary's GcDeferGuard::holding). Under stress it is there for the
# day that deferral is lost: it then collects inside the load, and the checks
# catch what that frees. So is macro_definition_env, all of whose tests load
# libraries; one of them then expands a library's macro after a collection,
# which is what its minimum counts.
#
# scheme_suite.rs runs every tests/scheme file on both backends, and at 16 it
# did not finish in 25 minutes (#626). One file is nearly all of that:
# srfi/regex-graphemes.scm, which compiles SRFI 115's grapheme regex, a large
# live heap of character sets, and then matches it against 400 syllables. Run
# alone through the debug CLI it took 9 s on the VM and 22 s on the
# tree-walker without stress, 12 s and 28 s at 4096, 58 s and 81 s at 256,
# and had not finished after 300 s on either at 16; every other file together
# took about 90 s and 130 s at 16. The whole target took 20 s without stress,
# 27 s at 4096, 47 s at 1024 and 125 s at 256 (measured 2026-10-02, under
# other load), so it runs at 4096, where it collects six times as often as
# it did without stress for a third more time. Since the byte trigger (#606)
# it collects less without stress, and 4096 is 28 times as often.
#
# Run it against a check build: a debug build, as here, or release with
# `--features patina-tests/gc-check` passed through. CI's Test Suite job
# runs it after `cargo test --all --lib --tests`, whose build it reuses: it
# names the workspace (`--workspace`), as that command does, so the features
# resolve the same and nothing recompiles.
#
# Usage: scripts/run_gc_stress_tests.sh [extra cargo test arguments]
# (a build's, such as `--release --features patina-tests/gc-check`; a test
# filter fails the tally)
set -euo pipefail

cd "$(dirname "$0")/.."

# This lane sets the one GC variable it means; one left exported in the
# shell would change what it measures.
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

# target, interval, minimum collections, tests (measured 2026-10-02: the
# minimum is half the count at that interval; in parentheses the count, and
# the count without stress, re-measured 2026-10-03 under the byte trigger,
# #606, which left every count under stress as it was; gc_tree_walker's and
# gc_vm's counts re-measured that day too, when their reclamation proofs
# moved to bytes, which raised them slightly, and again when the arena
# comparison stopped making a third run, which lowered them; ephemerons'
# minimum re-pinned 2026-10-04, when #639's tests of `(gc)` collecting at its
# call doubled its count, and again that day for seven more of them, `gc`
# as a control primitive's thunk among them; escape_from_primitive's count
# re-measured that day, 728 when it was pinned, and its minimum re-pinned
# that day too, for #607's test of opens that collect and retry, which
# collect without stress as well, and again for `load`'s retry, which that
# test also re-enters; interpreter_api's re-pinned 2026-10-06, when #643
# deleted its test of the source map's pruning, with the store it measured,
# and again that day for #612's test of a hundred `eval`s of one datum)
TARGETS=(
    "callability 16 31 11"                    # 62 (0)
    "control_flow_matrix 16 340 3"            # 680 (0)
    "cps_features 16 469 1"                   # 938 (0)
    "ephemerons 16 289 30"                    # 578 (204)
    "escape_from_primitive 16 480 13"         # 960 (74)
    "finished_forms_release_code 16 1450 9"   # 2901 (13)
    "gc_tree_walker 16 11421 19"              # 22851 (55)
    "gc_vm 16 8001 23"                        # 16012 (66)
    "hygiene_matrix 16 400 13"                # 800 (2)
    "interpreter_api 16 138 27"               # 277 (4)
    "library_loading 16 1 9"                  # 3 (0): see the header
    "macro_definition_env 16 129 15"          # 258 (2): see the header
    "vm_callprimitive 16 23 15"               # 47 (0)
    "scheme_suite 4096 1534 11"               # 3068 (111): see the header
)

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT

fail=0
lane_start=$SECONDS
for entry in "${TARGETS[@]}"; do
    read -r target interval minimum tests <<< "$entry"
    run="$OUT/$target"
    mkdir -p "$run/count"
    start=$SECONDS
    rc=0
    PATINA_GC_STRESS=$interval PATINA_GC_COUNT_DIR="$run/count" \
        cargo test --workspace --test "$target" "$@" > "$run/output.txt" 2>&1 || rc=$?
    took=$((SECONDS - start))
    problems=()

    if [ "$rc" -ne 0 ]; then
        problems+=("cargo test exited $rc")
    fi
    result=$(grep -E '^test result: ' "$run/output.txt" | tail -1 || true)
    if [ -z "$result" ]; then
        problems+=("no test result line")
    else
        passed=$(sed -n 's/^test result: [A-Za-z]*\. \([0-9]*\) passed;.*/\1/p' <<< "$result")
        filtered=$(sed -n 's/.*; \([0-9]*\) filtered out.*/\1/p' <<< "$result")
        if [ "${filtered:-x}" != 0 ]; then
            problems+=("${filtered:-an unknown number of} test(s) filtered out")
        fi
        if [ "${passed:-0}" -lt "$tests" ]; then
            problems+=("${passed:-no} test(s) passed, pinned $tests")
        fi
    fi

    # The test binary's own record, and any from processes it started; each
    # must have run under this interval.
    collections=0
    own=0
    for record in "$run"/count/gc-count.*; do
        [ -e "$record" ] || continue
        case "$(cat "$record")" in
            *" exe=$target-"*) own=1 ;;
        esac
        if ! grep -q " env=PATINA_GC_STRESS=$interval " "$record"; then
            problems+=("a process ran under another mode: $(cat "$record")")
        fi
        n=$(sed -n 's/.* collections=0*\([0-9][0-9]*\)$/\1/p' "$record")
        collections=$((collections + ${n:-0}))
    done
    if [ "$own" -eq 0 ]; then
        problems+=("no collection record from the test binary: PATINA_GC_COUNT_DIR never reached it")
    fi
    if [ "$collections" -lt "$minimum" ]; then
        problems+=("$collections collections, pinned minimum $minimum")
    fi

    line="$target at $interval: ${result#test result: }, $collections collections (minimum $minimum), ${took}s"
    if [ ${#problems[@]} -eq 0 ]; then
        echo "OK   $line"
    else
        echo "FAIL $line"
        for p in "${problems[@]}"; do echo "       $p"; done
        echo "       output (failures and panics):"
        grep -E 'panicked at|^test .* FAILED$|^failures:|^error' "$run/output.txt" | head -20 \
            | sed 's/^/         /' || true
        fail=1
    fi
done

echo ""
echo "GC stress lane: $((SECONDS - lane_start))s"
exit $fail
