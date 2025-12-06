#!/bin/bash
# Run chibi-scheme r7rs test suite with Patina interpreter
# Generates compatibility report

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Paths
PATINA_BIN="./target/release/patina"
TEST_FILE="scheme_tests/chibi/r7rs-tests.scm"
REPORT_DIR="scheme_tests/reports"
RESULTS_FILE="${REPORT_DIR}/results.txt"
COMPAT_REPORT="${REPORT_DIR}/compatibility.md"

# Check if binary exists
if [ ! -f "$PATINA_BIN" ]; then
    echo -e "${RED}Error: Patina binary not found at $PATINA_BIN${NC}"
    echo "Please build with: cargo build --release"
    exit 1
fi

# Check if test file exists
if [ ! -f "$TEST_FILE" ]; then
    echo -e "${RED}Error: Test file not found at $TEST_FILE${NC}"
    exit 1
fi

# Create reports directory if it doesn't exist
mkdir -p "$REPORT_DIR"

echo -e "${GREEN}Running chibi-scheme r7rs test suite...${NC}"
echo "Test file: $TEST_FILE"
echo "Results will be saved to: $RESULTS_FILE"
echo ""

# Run the tests and capture output
if "$PATINA_BIN" "$TEST_FILE" > "$RESULTS_FILE" 2>&1; then
    echo -e "${GREEN}Tests completed successfully${NC}"
else
    echo -e "${YELLOW}Tests completed with errors (exit code: $?)${NC}"
fi

# Parse results and generate report
echo ""
echo -e "${GREEN}Generating compatibility report...${NC}"

# Count test results from output
# Only count "Tests run:" lines that immediately follow "Running test suite:" lines
# This avoids double-counting from nested test sections (parent sections print cumulative totals)
COUNTS=$(awk '
    /^Running test suite:/ {
        saw_suite = 1
    }
    /^Tests run:/ && saw_suite == 1 {
        line = $0
        gsub(/,/, "", line)
        split(line, parts)
        for (i = 1; i <= length(parts); i++) {
            if (parts[i] == "run:") total += parts[i+1]
            if (parts[i] == "Passed:") passed += parts[i+1]
            if (parts[i] == "Failed:") failed += parts[i+1]
            if (parts[i] == "Errors:") errors += parts[i+1]
        }
        saw_suite = 0
    }
    END {
        print passed+0, failed+0, total+0, errors+0
    }
' "$RESULTS_FILE")

PASS_COUNT=$(echo "$COUNTS" | awk '{print $1}')
FAIL_COUNT=$(echo "$COUNTS" | awk '{print $2}')
TOTAL_COUNT=$(echo "$COUNTS" | awk '{print $3}')
ERROR_COUNT=$(echo "$COUNTS" | awk '{print $4}')

# TRUE_TOTAL is the total from the framework
TRUE_TOTAL=$TOTAL_COUNT

# Count lines that start with "FAIL:" for detailed failure reporting
ADDITIONAL_FAILS=$(grep -c "^FAIL:" "$RESULTS_FILE" || echo "0")

# Generate section breakdown
# Parse "Running test suite: X" followed immediately by "Tests run: ..." lines
# Skip cumulative totals from parent sections
SECTION_BREAKDOWN=$(awk '
    /^Running test suite:/ {
        section = $0
        sub(/^Running test suite: /, "", section)
        saw_suite = 1
    }
    /^Tests run:/ && saw_suite == 1 {
        if (section != "" && section != "R7RS") {
            # Parse: Tests run: X, Passed: Y, Failed: Z[, Errors: W]
            line = $0
            gsub(/,/, "", line)
            split(line, parts)
            total = 0; passed = 0; failed = 0; errors = 0
            for (i = 1; i <= length(parts); i++) {
                if (parts[i] == "run:") total = parts[i+1]
                if (parts[i] == "Passed:") passed = parts[i+1]
                if (parts[i] == "Failed:") failed = parts[i+1]
                if (parts[i] == "Errors:") errors = parts[i+1]
            }
            # Determine status emoji
            if (errors > 0 || failed > 0) {
                if (passed == total) status = "✅"
                else if (passed > 0) status = "⚠️"
                else status = "❌"
            } else {
                status = "✅"
            }
            printf "| %s | %s | %d | %d | %d | %d |\n", status, section, total, passed, failed, errors
        }
        section = ""
        saw_suite = 0
    }
' "$RESULTS_FILE")

# Generate markdown report
cat > "$COMPAT_REPORT" << EOF
# Patina R7RS Compatibility Report

**Generated:** $(date '+%Y-%m-%d %H:%M:%S')
**Test Suite:** chibi-scheme r7rs-tests.scm

## Summary

| Status | Count | Percentage |
|--------|-------|------------|
| ✅ Passed | $PASS_COUNT | $(awk "BEGIN {printf \"%.1f%%\", ($PASS_COUNT/$TRUE_TOTAL)*100}") |
| ❌ Failed | $FAIL_COUNT | $(awk "BEGIN {printf \"%.1f%%\", ($FAIL_COUNT/$TRUE_TOTAL)*100}") |
| ⚠️ Error (crashed) | $ERROR_COUNT | $(awk "BEGIN {printf \"%.1f%%\", ($ERROR_COUNT/$TRUE_TOTAL)*100}") |
| **Total** | **$TRUE_TOTAL** | **100%** |

**Note:** "Error" means the test crashed before assertions could run (usually missing features like call/cc, guard).

## Section Breakdown

| Status | Section | Total | Passed | Failed | Errors |
|--------|---------|-------|--------|--------|--------|
$SECTION_BREAKDOWN

**Legend:** ✅ = All passing, ⚠️ = Partial, ❌ = None passing

## Failed Tests

EOF

# Add failed test details if any
if [ "$ADDITIONAL_FAILS" -gt 0 ]; then
    echo "### Test Failures" >> "$COMPAT_REPORT"
    echo "" >> "$COMPAT_REPORT"
    echo "\`\`\`" >> "$COMPAT_REPORT"
    grep "^FAIL:" "$RESULTS_FILE" >> "$COMPAT_REPORT" || true
    echo "\`\`\`" >> "$COMPAT_REPORT"
    echo "" >> "$COMPAT_REPORT"
fi

# Add error details if any
if [ "$ERROR_COUNT" -gt 0 ]; then
    echo "### Errors" >> "$COMPAT_REPORT"
    echo "" >> "$COMPAT_REPORT"
    echo "\`\`\`" >> "$COMPAT_REPORT"
    grep "^Error:" "$RESULTS_FILE" >> "$COMPAT_REPORT" || true
    echo "\`\`\`" >> "$COMPAT_REPORT"
    echo "" >> "$COMPAT_REPORT"
fi

# Add full results link
cat >> "$COMPAT_REPORT" << EOF

## Full Results

See [results.txt](./results.txt) for complete test output.

## Notes

This report tracks Patina's compatibility with the R7RS-small specification
using the chibi-scheme test suite. The goal is to reach 100% compatibility
with all R7RS-small features.

### Known Limitations

- Exception handling (guard, raise) not yet implemented
- Continuations (call/cc) not yet implemented
- Some R7RS libraries not yet implemented: (scheme eval), (scheme r5rs), (scheme process-context)
- See \`PRD/phase1/EXCEPTION_HANDLING.md\` for exception handling roadmap

### Next Steps

See \`docs/FEATURE_STATUS.md\` for detailed R7RS compliance matrix and
\`PRD/phase1/IMPLEMENTATION_STATUS.md\` for roadmap.
EOF

echo -e "${GREEN}Report generated: $COMPAT_REPORT${NC}"
echo ""
echo "=== Summary ==="
PASS_PCT=$(awk -v p="$PASS_COUNT" -v t="$TRUE_TOTAL" 'BEGIN {printf "%.1f", (p/t)*100}')
FAIL_PCT=$(awk -v f="$FAIL_COUNT" -v t="$TRUE_TOTAL" 'BEGIN {printf "%.1f", (f/t)*100}')
ERROR_PCT=$(awk -v e="$ERROR_COUNT" -v t="$TRUE_TOTAL" 'BEGIN {printf "%.1f", (e/t)*100}')
echo -e "  Passed: ${GREEN}$PASS_COUNT${NC} ($PASS_PCT%)"
echo -e "  Failed: ${RED}$FAIL_COUNT${NC} ($FAIL_PCT%)"
echo -e "  Errors: ${YELLOW}$ERROR_COUNT${NC} ($ERROR_PCT%)"
echo -e "  Total:  $TRUE_TOTAL"
echo ""
echo "=== Section Breakdown ==="
printf "%-40s %6s %6s %6s %6s\n" "Section" "Total" "Pass" "Fail" "Error"
printf "%-40s %6s %6s %6s %6s\n" "----------------------------------------" "------" "------" "------" "------"
echo "$SECTION_BREAKDOWN" | while IFS='|' read -r _ status section total passed failed errors _; do
    # Skip empty lines
    [ -z "$section" ] && continue
    # Trim whitespace
    section=$(echo "$section" | xargs)
    total=$(echo "$total" | xargs)
    passed=$(echo "$passed" | xargs)
    failed=$(echo "$failed" | xargs)
    errors=$(echo "$errors" | xargs)
    # Color based on status
    if [ "$errors" -gt 0 ] || [ "$failed" -gt 0 ]; then
        if [ "$passed" -eq "$total" ]; then
            color=$GREEN
        elif [ "$passed" -gt 0 ]; then
            color=$YELLOW
        else
            color=$RED
        fi
    else
        color=$GREEN
    fi
    printf "${color}%-40s %6s %6s %6s %6s${NC}\n" "$section" "$total" "$passed" "$failed" "$errors"
done
