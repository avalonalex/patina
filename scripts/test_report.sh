#!/bin/bash
# Test Progress Report Generator
# Usage: ./scripts/test_report.sh

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════╗"
echo "║     Patina R7RS Compliance Test Report        ║"
echo "╔════════════════════════════════════════════════╗"
echo ""

# Run tests and capture output
TEST_OUTPUT=$(cargo test --test compliance --test integration 2>&1)

# Parse test results for compliance tests
COMPLIANCE_OUTPUT=$(echo "$TEST_OUTPUT" | grep -A 100 "Running tests/compliance.rs")

# Count tests by category (from the compliance test output)
count_tests() {
    local category=$1
    local passed=$(echo "$COMPLIANCE_OUTPUT" | grep "^test $category::" | grep "ok$" | wc -l | tr -d ' ')
    local ignored=$(echo "$COMPLIANCE_OUTPUT" | grep "^test $category::" | grep "ignored$" | wc -l | tr -d ' ')
    local total=$(echo "$COMPLIANCE_OUTPUT" | grep "^test $category::" | wc -l | tr -d ' ')

    echo "$passed $ignored $total"
}

PRIMITIVES_STATS=($(count_tests "primitives"))
NUMBERS_STATS=($(count_tests "numbers"))
LISTS_STATS=($(count_tests "lists"))
PREDICATES_STATS=($(count_tests "predicates"))
DERIVED_STATS=($(count_tests "derived"))

# Extract values
PRIMITIVES_PASSED=${PRIMITIVES_STATS[0]:-0}
PRIMITIVES_IGNORED=${PRIMITIVES_STATS[1]:-0}
PRIMITIVES=${PRIMITIVES_STATS[2]:-0}

NUMBERS_PASSED=${NUMBERS_STATS[0]:-0}
NUMBERS_IGNORED=${NUMBERS_STATS[1]:-0}
NUMBERS=${NUMBERS_STATS[2]:-0}

LISTS_PASSED=${LISTS_STATS[0]:-0}
LISTS_IGNORED=${LISTS_STATS[1]:-0}
LISTS=${LISTS_STATS[2]:-0}

PREDICATES_PASSED=${PREDICATES_STATS[0]:-0}
PREDICATES_IGNORED=${PREDICATES_STATS[1]:-0}
PREDICATES=${PREDICATES_STATS[2]:-0}

DERIVED_PASSED=${DERIVED_STATS[0]:-0}
DERIVED_IGNORED=${DERIVED_STATS[1]:-0}
DERIVED=${DERIVED_STATS[2]:-0}

# Calculate percentages
calc_percentage() {
    local passed=$1
    local total=$2
    if [ "$total" -eq 0 ]; then
        echo "0"
    else
        echo "scale=0; ($passed * 100) / $total" | bc
    fi
}

PRIMITIVES_PCT=$(calc_percentage $PRIMITIVES_PASSED $PRIMITIVES)
NUMBERS_PCT=$(calc_percentage $NUMBERS_PASSED $NUMBERS)
LISTS_PCT=$(calc_percentage $LISTS_PASSED $LISTS)
PREDICATES_PCT=$(calc_percentage $PREDICATES_PASSED $PREDICATES)
DERIVED_PCT=$(calc_percentage $DERIVED_PASSED $DERIVED)

# Print report
echo "Test Category Results"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
printf "%-20s %3s/%-3s (%3s%%) %s\n" "Primitives:" "$PRIMITIVES_PASSED" "$PRIMITIVES" "$PRIMITIVES_PCT" "$([ $PRIMITIVES_PCT -gt 70 ] && echo '✅' || echo '⚠️ ')"
printf "%-20s %3s/%-3s (%3s%%) %s\n" "Numbers:" "$NUMBERS_PASSED" "$NUMBERS" "$NUMBERS_PCT" "$([ $NUMBERS_PCT -gt 50 ] && echo '✅' || echo '⚠️ ')"
printf "%-20s %3s/%-3s (%3s%%) %s\n" "Lists:" "$LISTS_PASSED" "$LISTS" "$LISTS_PCT" "$([ $LISTS_PCT -gt 30 ] && echo '⚠️ ' || echo '❌')"
printf "%-20s %3s/%-3s (%3s%%) %s\n" "Predicates:" "$PREDICATES_PASSED" "$PREDICATES" "$PREDICATES_PCT" "$([ $PREDICATES_PCT -gt 50 ] && echo '✅' || echo '⚠️ ')"
printf "%-20s %3s/%-3s (%3s%%) %s\n" "Derived Forms:" "$DERIVED_PASSED" "$DERIVED" "$DERIVED_PCT" "$([ $DERIVED_PCT -gt 20 ] && echo '⚠️ ' || echo '❌')"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Overall stats
TOTAL_CAT=$((PRIMITIVES + NUMBERS + LISTS + PREDICATES + DERIVED))
PASSED_CAT=$((PRIMITIVES_PASSED + NUMBERS_PASSED + LISTS_PASSED + PREDICATES_PASSED + DERIVED_PASSED))
IGNORED_CAT=$((PRIMITIVES_IGNORED + NUMBERS_IGNORED + LISTS_IGNORED + PREDICATES_IGNORED + DERIVED_IGNORED))
OVERALL_PCT=$(calc_percentage $PASSED_CAT $TOTAL_CAT)

echo ""
echo "Overall Statistics"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Total Tests:        $TOTAL_CAT"
echo "Passing:            $PASSED_CAT (${OVERALL_PCT}%)"
echo "Ignored:            $IGNORED_CAT"
echo ""

# Progress bar
PROGRESS=$((OVERALL_PCT / 2))  # Scale to 50 chars
echo -n "Progress: ["
for i in $(seq 1 50); do
    if [ $i -le $PROGRESS ]; then
        echo -n "█"
    else
        echo -n "░"
    fi
done
echo "] ${OVERALL_PCT}%"
echo ""

# Next priorities
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Next Priorities (from FEATURE_MATRIX.md):"
echo "  1. let, let*, letrec (binding constructs)"
echo "  2. and, or (boolean operators)"
echo "  3. apply (higher-order function support)"
echo "  4. Basic number operations (abs, quotient, etc.)"
echo "  5. List operations (length, append, reverse)"
echo ""
echo "See tests/FEATURE_MATRIX.md for complete status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
