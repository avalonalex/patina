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
# Parse lines like "Tests run: 10, Passed: 8, Failed: 2"
PASS_COUNT=$(grep "Tests run:" "$RESULTS_FILE" | awk -F'Passed: ' '{sum += $2} END {print sum+0}')
FAIL_COUNT=$(grep "Tests run:" "$RESULTS_FILE" | awk -F'Failed: ' '{sum += $2} END {print sum+0}')
TOTAL_COUNT=$(grep "Tests run:" "$RESULTS_FILE" | awk -F'Tests run: ' '{sum += $2} END {print sum+0}')

# Count lines that start with "FAIL:" for additional failures
ADDITIONAL_FAILS=$(grep -c "^FAIL:" "$RESULTS_FILE" || echo "0")

# Count errors from error messages (these are tests that crashed)
ERROR_COUNT=$(grep -c "^Error:" "$RESULTS_FILE" || echo "0")

# Calculate true total: tests that ran + tests that crashed
TRUE_TOTAL=$((TOTAL_COUNT + ERROR_COUNT))

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

**Note:** "Error" means the test crashed before assertions could run. These count as failures in the overall percentage.

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

- Some R7RS libraries are not yet fully implemented (see \`IMPORT_AND_LIBRARIES.md\`)
- I/O and ports functionality is incomplete
- Exception handling (guard, raise) not yet implemented
- Continuations (call/cc) not yet implemented

### Next Steps

See \`docs/FEATURE_STATUS.md\` for detailed R7RS compliance matrix and
\`PRD/phase1/IMPLEMENTATION_STATUS.md\` for roadmap.
EOF

echo -e "${GREEN}Report generated: $COMPAT_REPORT${NC}"
echo ""
echo "Summary:"
echo -e "  Passed: ${GREEN}$PASS_COUNT${NC}"
echo -e "  Failed: ${RED}$FAIL_COUNT${NC}"
echo -e "  Errors: ${YELLOW}$ERROR_COUNT${NC}"
echo -e "  Total:  $TOTAL_COUNT"
