#!/bin/bash
# Run Larceny's R7RS test suites under Patina and write a per-suite report.
#
# The suites are Will Clinger's R7RS rewrite of Racket's R6RS test suite
# (test/R7RS/Lib in the Larceny repository). They are LGPL, so they are NOT
# vendored into this repo; this script runs them from a reference checkout
# outside it, the same way ~/Project/reference/chibi-scheme is used. Fetch it
# with the sparse-checkout commands printed below when the directory is absent.
#
#   ./scripts/run_larceny_tests.sh                    # R7RS lane, VM backend
#   ./scripts/run_larceny_tests.sh --tree-walker      # CPS tree-walker backend
#   ./scripts/run_larceny_tests.sh --r6rs             # (r6rs ...) emulation lane
#   ./scripts/run_larceny_tests.sh base char          # a subset of suites
#
# Environment:
#   LARCENY_TESTS_DIR     the Lib directory (default: ~/Project/reference/larceny/test/R7RS/Lib)
#   LARCENY_TEST_TIMEOUT  seconds per suite (default: 300). One suite floors
#                         it instead of obeying it: `stream` on the
#                         tree-walker gets at least 900 s, because it PASSES
#                         given time (81/81, measured 792 s) and is merely
#                         ~20x slower there than on the VM — triage family
#                         26. Burning 300 s to report "timeout" told us
#                         nothing that 13 minutes of tally does not.
#
# Exits non-zero if any suite fails, errors, or times out. The tallies are the
# suite's own ("N tests passed" / "N of M tests failed."), never re-derived.

set -eo pipefail

cd "$(dirname "$0")/.."

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# The commit the sweep was calibrated against. Larceny's last commit is from
# 2017, so drift is unlikely, but a different checkout would silently change
# the denominator, so it is checked rather than assumed.
PINNED_COMMIT="fef550c7d3923deb7a5a1ccd5a628e54cf231c75"

LARCENY_TESTS_DIR="${LARCENY_TESTS_DIR:-$HOME/Project/reference/larceny/test/R7RS/Lib}"
TIMEOUT="${LARCENY_TEST_TIMEOUT:-300}"

BACKEND_ARGS=()
BACKEND_NAME="VM"
LANE="r7rs"
SUFFIX=""
SUITES=()
for arg in "$@"; do
    case "$arg" in
        --tree-walker)
            BACKEND_ARGS=(--tree-walker)
            BACKEND_NAME="tree-walker"
            ;;
        --r6rs) LANE="r6rs" ;;
        --r7rs) LANE="r7rs" ;;
        -h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        --*)
            echo "Unknown option: $arg" >&2
            exit 2
            ;;
        *) SUITES+=("$arg") ;;
    esac
done
[ "$LANE" = "r6rs" ] && SUFFIX="_r6rs"
[ "$BACKEND_NAME" = "tree-walker" ] && SUFFIX="${SUFFIX}_tree_walker"

PATINA_BIN="$PWD/target/release/patina"
REPORT_DIR="scheme_tests/reports"
LOG_DIR="${REPORT_DIR}/larceny${SUFFIX}"
REPORT="${REPORT_DIR}/larceny${SUFFIX}.md"

if [ ! -f "$PATINA_BIN" ]; then
    echo -e "${RED}Error: Patina binary not found at $PATINA_BIN${NC}"
    echo "Please build with: cargo build --release"
    exit 1
fi
if [ ! -d "$LARCENY_TESTS_DIR/tests/scheme" ]; then
    echo -e "${RED}Error: Larceny test suite not found at $LARCENY_TESTS_DIR${NC}"
    cat <<FETCH
Fetch it (sparse, ~1 MB of Scheme) with:

  mkdir -p ~/Project/reference && cd ~/Project/reference
  git clone --filter=blob:none --no-checkout --depth 1 https://github.com/larcenists/larceny.git larceny
  cd larceny && git sparse-checkout set test/R7RS/Lib && git checkout

or point LARCENY_TESTS_DIR at an existing checkout's test/R7RS/Lib.
FETCH
    exit 1
fi

ACTUAL_COMMIT=$(git -C "$LARCENY_TESTS_DIR" rev-parse HEAD 2>/dev/null || echo unknown)
if [ "$ACTUAL_COMMIT" != "$PINNED_COMMIT" ]; then
    echo -e "${YELLOW}Warning: checkout is at $ACTUAL_COMMIT, calibrated against $PINNED_COMMIT${NC}"
fi

# Lane layout. The R6RS lane exercises the bundled (r6rs ...) libraries through
# Clinger's R7RS-syntax rewrite of the R6RS suite; its sources use #vu8( and
# brackets, hence --allow-r6rs.
case "$LANE" in
    r7rs)
        RUN_DIR="tests/scheme/run"
        LANE_ARGS=()
        LANE_DESC="Larceny tests/scheme (R7RS-small + Red edition)"
        ;;
    r6rs)
        RUN_DIR="tests/r6rs/run"
        LANE_ARGS=(--allow-r6rs)
        LANE_DESC="Larceny tests/r6rs ((r6rs ...) emulation libraries)"
        ;;
esac

# Every run/*.sps (recursively, for r6rs's arithmetic/ and io/) unless a
# subset was named. Names are the path under run/ without the extension.
if [ ${#SUITES[@]} -eq 0 ]; then
    while IFS= read -r f; do
        f="${f#"$LARCENY_TESTS_DIR/$RUN_DIR/"}"
        SUITES+=("${f%.sps}")
    done < <(find "$LARCENY_TESTS_DIR/$RUN_DIR" -name '*.sps' | sort)
fi

mkdir -p "$LOG_DIR"
rm -f "$LOG_DIR"/*.txt

echo -e "${GREEN}Running ${LANE_DESC} (${BACKEND_NAME} backend)...${NC}"
echo "Suite dir: $LARCENY_TESTS_DIR"
echo "Logs:      $LOG_DIR/"
echo ""

TOTAL_PASSED=0
TOTAL_ASSERTED=0
SUITES_CLEAN=0
SUITES_RUN=0
FAILING=()

# Larceny's convention is to run from the Lib directory: base.sld includes
# "tests/scheme/base-test1.scm" cwd-relative, and -I . is how upstream's own
# scripts find (tests scheme test). No coreutils `timeout` on macOS, so a perl
# alarm does the job; SIGALRM surfaces as exit 142.
run_suite() {
    local suite="$1"
    local log="$LOG_DIR/${suite//\//_}.txt"
    local start end secs status detail passed failed total errors
    # Family 26: `stream` is correct on the tree-walker but ~20x slower than
    # the VM on nested infinite streams — 792 s measured for the full suite
    # against the 300 s default budget. Floor that one suite's budget at
    # 900 s so the lane reports a tally instead of a timeout; everything
    # else keeps $TIMEOUT, ephemeron included (family 32's 100-million-pair
    # allocation has no measured finishing time to size a budget by).
    local budget="$TIMEOUT"
    if [ "$BACKEND_NAME" = "tree-walker" ] && [ "$suite" = "stream" ] && [ "$budget" -lt 900 ]; then
        budget=900
    fi
    start=$(date +%s)
    # -e off for the whole run-and-parse span: the suite may exit non-zero,
    # and every grep below legitimately matches nothing for some status.
    set +e
    (
        cd "$LARCENY_TESTS_DIR" &&
        perl -e 'alarm shift; exec @ARGV' "$budget" \
            "$PATINA_BIN" "${BACKEND_ARGS[@]}" "${LANE_ARGS[@]}" -I . \
            "$RUN_DIR/$suite.sps" </dev/null
    ) 2>&1 | sed 's/\x1b\[[0-9;]*m//g' > "$log"
    local rc=${PIPESTATUS[0]}
    end=$(date +%s)
    secs=$((end - start))

    passed=$(grep -E '^[0-9]+ tests passed$' "$log" | tail -1 | awk '{print $1}')
    failed=$(grep -E '^[0-9]+ of [0-9]+ tests failed\.$' "$log" | tail -1 | awk '{print $1}')
    total=$(grep -E '^[0-9]+ of [0-9]+ tests failed\.$' "$log" | tail -1 | awk '{print $3}')
    errors=$(grep -c '^Error' "$log" || true)

    if [ "$rc" -eq 142 ]; then
        status="timeout"; passed=0; failed=0; total=0
        detail="no result after ${budget}s"
    elif [ -n "$passed" ]; then
        status="pass"; failed=0; total=$passed
        detail=""
    elif [ -n "$failed" ]; then
        status="fail"; passed=$((total - failed))
        # Deliberately not quoting the failing expressions: the suite is LGPL
        # and this report is tracked in an MIT repo. They are in the log.
        detail="${failed} assertion(s) failed — see the log"
    elif grep -q 'overflowed its stack' "$log"; then
        status="crash"; passed=0; failed=0; total=0
        detail="stack overflow (exit $rc)"
    elif [ "$rc" -ge 128 ]; then
        status="crash"; passed=0; failed=0; total=0
        detail="signal $((rc - 128)): $(grep -v '^$' "$log" | tail -1 | cut -c1-120)"
    else
        status="error"; passed=0; failed=0; total=0
        detail="$(grep -m1 '^Error' "$log" | cut -c1-140)"
    fi
    if [ "$errors" -gt 0 ] && [ "$status" != "error" ]; then
        detail="${errors} top-level error(s); ${detail}"
    fi
    # Patina's messages name files by absolute path; the report is tracked,
    # so keep the local checkout location out of it.
    detail="${detail//$LARCENY_TESTS_DIR\//}"
    detail="${detail//$HOME\//~/}"
    set -e

    SUITES_RUN=$((SUITES_RUN + 1))
    TOTAL_PASSED=$((TOTAL_PASSED + passed))
    TOTAL_ASSERTED=$((TOTAL_ASSERTED + total))
    case "$status" in
        pass) icon="✅"; SUITES_CLEAN=$((SUITES_CLEAN + 1)) ;;
        fail) icon="⚠️"; FAILING+=("$suite") ;;
        *)    icon="❌"; FAILING+=("$suite") ;;
    esac
    printf "  %s %-22s %-8s %5s/%-5s %4ss  %s\n" "$icon" "$suite" "$status" "$passed" "$total" "$secs" "$detail"
}

for suite in "${SUITES[@]}"; do
    run_suite "$suite"
done

pct() { awk -v n="$1" -v t="$2" 'BEGIN { if (t == 0) print "n/a"; else printf "%.1f%%", n / t * 100 }'; }
# The tracked report is organised by kind of problem and links each failing
# assertion to its test case upstream (a permalink at the pinned commit) —
# it quotes nothing from the LGPL suite. Rendering needs the suite sources,
# so it is a small Python helper rather than more awk.
if command -v python3 >/dev/null; then
    python3 scripts/larceny_report.py \
        --logs "$LOG_DIR" --suites "$LARCENY_TESTS_DIR" --lane "$LANE" \
        --commit "$ACTUAL_COMMIT" --backend "$BACKEND_NAME" \
        --generated "$(date '+%Y-%m-%d %H:%M:%S')" --out "$REPORT" >/dev/null
else
    echo -e "${YELLOW}python3 not found; the report was not rendered (logs are in $LOG_DIR)${NC}"
fi

echo ""
echo -e "${GREEN}=== Larceny suite summary (${LANE}, ${BACKEND_NAME}) ===${NC}"
echo "  Suites fully passing: ${SUITES_CLEAN} of ${SUITES_RUN}"
echo "  Assertions passed:    ${TOTAL_PASSED} of ${TOTAL_ASSERTED} ($(pct "$TOTAL_PASSED" "$TOTAL_ASSERTED"))"
echo ""
echo "Report: $REPORT"

if [ ${#FAILING[@]} -gt 0 ]; then
    exit 1
fi
