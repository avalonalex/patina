#!/bin/bash
# Run the chibi-scheme r7rs test suite under Patina and generate a report.
#
#   ./scripts/run_chibi_tests.sh                 # VM backend (default)
#   ./scripts/run_chibi_tests.sh --tree-walker   # CPS tree-walker backend
#
# The suite reports itself through (chibi test), which Patina bundles verbatim
# from upstream, so the totals here are the framework's own -- not a count this
# script derives. That matters: the previous hand-written (chibi test) subset
# under-reported by 63 tests and silently swallowed failures it could not
# express.

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

BACKEND_FLAG=""
BACKEND_NAME="VM"
SUFFIX=""
for arg in "$@"; do
    case "$arg" in
        --tree-walker)
            BACKEND_FLAG="--tree-walker"
            BACKEND_NAME="tree-walker"
            SUFFIX="_tree_walker"
            ;;
        *)
            echo "Unknown option: $arg" >&2
            echo "Usage: $0 [--tree-walker]" >&2
            exit 2
            ;;
    esac
done

PATINA_BIN="./target/release/patina"
TEST_FILE="scheme_tests/chibi/r7rs-tests.scm"
REPORT_DIR="scheme_tests/reports"
RESULTS_FILE="${REPORT_DIR}/results${SUFFIX}.txt"
COMPAT_REPORT="${REPORT_DIR}/compatibility${SUFFIX}.md"

if [ ! -f "$PATINA_BIN" ]; then
    echo -e "${RED}Error: Patina binary not found at $PATINA_BIN${NC}"
    echo "Please build with: cargo build --release"
    exit 1
fi
if [ ! -f "$TEST_FILE" ]; then
    echo -e "${RED}Error: Test file not found at $TEST_FILE${NC}"
    exit 1
fi

mkdir -p "$REPORT_DIR"

echo -e "${GREEN}Running chibi-scheme r7rs test suite (${BACKEND_NAME} backend)...${NC}"
echo "Test file: $TEST_FILE"
echo "Results:   $RESULTS_FILE"
echo ""

# (chibi test) exits non-zero when any test fails; keep going so the report is
# still written.
if "$PATINA_BIN" $BACKEND_FLAG "$TEST_FILE" > "$RESULTS_FILE" 2>&1; then
    echo -e "${GREEN}Suite completed${NC}"
else
    echo -e "${YELLOW}Suite completed with failures (exit code: $?)${NC}"
fi

# (chibi test) colourises its output; every downstream reader wants it plain.
PLAIN=$(sed 's/\x1b\[[0-9;]*m//g' "$RESULTS_FILE")

# Grand total: the framework's own final tally, emitted unindented at column 0.
# Section tallies are indented, so anchoring on ^ distinguishes them.
read -r PASS_COUNT TRUE_TOTAL <<<"$(printf '%s\n' "$PLAIN" \
    | awk '/^[0-9]+ out of [0-9]+ .*tests passed/ { p=$1; t=$4 } END { print p+0, t+0 }')"
SUBGROUPS=$(printf '%s\n' "$PLAIN" \
    | awk '/^[0-9]+ out of [0-9]+ .*subgroups? passed/ { print $1"/"$4 }' | tail -1)

FAIL_COUNT=$((TRUE_TOTAL - PASS_COUNT))
# A test that crashes the interpreter never reaches an assertion, so it is
# counted separately from a test that ran and disagreed.
ERROR_COUNT=$(printf '%s\n' "$PLAIN" | grep -c "^Error" || true)

if [ "$TRUE_TOTAL" -eq 0 ]; then
    echo -e "${RED}Could not parse a total from the suite output.${NC}"
    echo "This usually means the run aborted early — see $RESULTS_FILE"
    exit 1
fi

pct() { awk "BEGIN {printf \"%.1f%%\", ($1/$TRUE_TOTAL)*100}"; }

# Section breakdown: each group prints "<name>: <dots>" and then its own tally.
SECTION_BREAKDOWN=$(printf '%s\n' "$PLAIN" | awk '
    /^ +[^ ].*: *\.*$/ {
        line = $0
        sub(/^ +/, "", line)
        sub(/: *\.*$/, "", line)
        if (line != "") name = line
        next
    }
    /^ +[0-9]+ out of [0-9]+ .*tests passed/ {
        if (name == "") next
        passed = $1; total = $4; failed = total - passed
        status = (failed == 0) ? "✅" : ((passed == 0) ? "❌" : "⚠️")
        printf "| %s | %s | %d | %d | %d |\n", status, name, total, passed, failed
        name = ""
    }
')

cat > "$COMPAT_REPORT" << EOF
# Patina R7RS Compatibility Report

**Generated:** $(date '+%Y-%m-%d %H:%M:%S')
**Backend:** $BACKEND_NAME
**Test Suite:** chibi-scheme r7rs-tests.scm, reported by upstream \`(chibi test)\`

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | $PASS_COUNT | $(pct "$PASS_COUNT") |
| ❌ Failed | $FAIL_COUNT | $(pct "$FAIL_COUNT") |
| **Total** | **$TRUE_TOTAL** | **100%** |

Subgroups passed: **${SUBGROUPS:-n/a}**

Counts come from \`(chibi test)\` itself rather than being re-derived here, so
they cannot drift from what the framework actually asserted.

## Section Breakdown

| Status | Section | Total | Passed | Failed |
|--------|---------|-------|--------|--------|
$SECTION_BREAKDOWN

**Legend:** ✅ = all passing, ⚠️ = partial, ❌ = none passing

EOF

if [ "$FAIL_COUNT" -gt 0 ]; then
    {
        echo "## Failures"
        echo ""
        echo '```'
        printf '%s\n' "$PLAIN" | grep -A2 "FAIL:" || true
        echo '```'
        echo ""
    } >> "$COMPAT_REPORT"
fi

if [ "$ERROR_COUNT" -gt 0 ]; then
    {
        echo "## Errors"
        echo ""
        echo "A test that crashes never reaches its assertion, so these are not"
        echo "included in the failure count above."
        echo ""
        echo '```'
        printf '%s\n' "$PLAIN" | grep "^Error" | sort | uniq -c | sort -rn || true
        echo '```'
        echo ""
    } >> "$COMPAT_REPORT"
fi

cat >> "$COMPAT_REPORT" << EOF
## Full Output

See \`$(basename "$RESULTS_FILE")\` in this directory.
EOF

echo ""
echo -e "${GREEN}=== R7RS Compatibility Summary (${BACKEND_NAME}) ===${NC}"
echo -e "  Passed: ${GREEN}${PASS_COUNT}${NC} ($(pct "$PASS_COUNT"))"
if [ "$FAIL_COUNT" -gt 0 ]; then
    echo -e "  Failed: ${RED}${FAIL_COUNT}${NC} ($(pct "$FAIL_COUNT"))"
else
    echo -e "  Failed: ${GREEN}0${NC} (0.0%)"
fi
if [ "$ERROR_COUNT" -gt 0 ]; then
    echo -e "  Errors: ${RED}${ERROR_COUNT}${NC} (crashed before asserting)"
fi
echo "  Total:  $TRUE_TOTAL"
echo "  Subgroups: ${SUBGROUPS:-n/a}"
echo ""
echo "Report: $COMPAT_REPORT"

[ "$FAIL_COUNT" -eq 0 ] && [ "$ERROR_COUNT" -eq 0 ]
