#!/bin/bash
# Run Patina benchmark suite and generate performance report
# Similar to run_chibi_tests.sh but for benchmarks

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Paths
REPORT_DIR="benchmark_reports"
RESULTS_FILE="${REPORT_DIR}/results.txt"
PERF_REPORT="${REPORT_DIR}/performance.md"
HISTORY_FILE="${REPORT_DIR}/history.csv"

# Benchmark configuration
# Tuned for ~5 minute total runtime
# Reduce sample count and measurement time for faster runs
BENCH_FLAGS="--sample-size 20 --measurement-time 2"

# Parse arguments
QUICK_MODE=false
FILTER=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            QUICK_MODE=true
            BENCH_FLAGS="--sample-size 10 --measurement-time 1"
            shift
            ;;
        --filter)
            FILTER="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --quick     Run with minimal samples (~2 min)"
            echo "  --filter X  Only run benchmarks matching X"
            echo "  --help      Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo not found${NC}"
    exit 1
fi

# Create reports directory
mkdir -p "$REPORT_DIR"

# Collect platform information
get_platform_info() {
    echo "=== Platform Information ==="

    # OS
    if [[ "$OSTYPE" == "darwin"* ]]; then
        OS_NAME="macOS $(sw_vers -productVersion 2>/dev/null || echo "unknown")"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        OS_NAME="Linux $(uname -r)"
    else
        OS_NAME="$OSTYPE"
    fi
    echo "OS: $OS_NAME"

    # CPU
    if [[ "$OSTYPE" == "darwin"* ]]; then
        CPU_NAME=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown")
        CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo "?")
    else
        CPU_NAME=$(grep -m1 "model name" /proc/cpuinfo 2>/dev/null | cut -d: -f2 | xargs || echo "unknown")
        CPU_CORES=$(nproc 2>/dev/null || echo "?")
    fi
    echo "CPU: $CPU_NAME"
    echo "Cores: $CPU_CORES"

    # Memory
    if [[ "$OSTYPE" == "darwin"* ]]; then
        MEM_BYTES=$(sysctl -n hw.memsize 2>/dev/null || echo "0")
        MEM_GB=$((MEM_BYTES / 1024 / 1024 / 1024))
    else
        MEM_KB=$(grep MemTotal /proc/meminfo 2>/dev/null | awk '{print $2}' || echo "0")
        MEM_GB=$((MEM_KB / 1024 / 1024))
    fi
    echo "Memory: ${MEM_GB}GB"

    # Rust version
    RUST_VER=$(rustc --version 2>/dev/null || echo "unknown")
    echo "Rust: $RUST_VER"
}

PLATFORM_INFO=$(get_platform_info)

# Header
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║          Patina Scheme Benchmark Suite                     ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "$PLATFORM_INFO" | while read -r line; do
    echo -e "${CYAN}$line${NC}"
done
echo ""
echo -e "Mode:    ${CYAN}$([ "$QUICK_MODE" = true ] && echo "Quick" || echo "Standard")${NC}"
echo -e "Filter:  ${CYAN}${FILTER:-"(all)"}${NC}"
echo -e "Report:  ${CYAN}$PERF_REPORT${NC}"
echo ""

# Build release first
echo -e "${BLUE}Building release binary...${NC}"
cargo build --release --quiet

# Run benchmarks
echo -e "${BLUE}Running benchmarks...${NC}"
echo ""

START_TIME=$(date +%s)

# Run criterion benchmarks and capture output
if [ -n "$FILTER" ]; then
    cargo bench --package patina-tests -- $BENCH_FLAGS "$FILTER" 2>&1 | tee "$RESULTS_FILE"
else
    cargo bench --package patina-tests -- $BENCH_FLAGS 2>&1 | tee "$RESULTS_FILE"
fi

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
ELAPSED_MIN=$((ELAPSED / 60))
ELAPSED_SEC=$((ELAPSED % 60))

echo ""
echo -e "${GREEN}Benchmarks completed in ${ELAPSED_MIN}m ${ELAPSED_SEC}s${NC}"
echo ""

# Parse benchmark results from Criterion output
echo -e "${BLUE}Generating performance report...${NC}"

# Extract benchmark results using awk
# Criterion format: "benchmark_name    time:   [lower_bound median upper_bound]"
BENCH_RESULTS=$(awk '
    /^[a-zA-Z].*time:.*\[.*\]/ {
        # Extract benchmark name (everything before "time:")
        split($0, parts, "time:")
        name = parts[1]
        gsub(/^[ \t]+|[ \t]+$/, "", name)

        # Extract timing from brackets [low median high]
        # Find the bracket content
        start = index($0, "[")
        end = index($0, "]")
        if (start > 0 && end > start) {
            bracket = substr($0, start+1, end-start-1)
            # Split by spaces, median is values 3-4 (value + unit)
            n = split(bracket, times, " ")
            if (n >= 4) {
                median = times[3] " " times[4]
            } else if (n >= 2) {
                median = times[1] " " times[2]
            }
        }

        if (name != "" && median != "") {
            printf "%s|%s\n", name, median
        }
    }
' "$RESULTS_FILE")

# Count benchmarks
BENCH_COUNT=$(echo "$BENCH_RESULTS" | grep -c "|" || echo "0")

# Extract platform details for report
OS_INFO=$(echo "$PLATFORM_INFO" | grep "^OS:" | cut -d: -f2 | xargs)
CPU_INFO=$(echo "$PLATFORM_INFO" | grep "^CPU:" | cut -d: -f2 | xargs)
CORES_INFO=$(echo "$PLATFORM_INFO" | grep "^Cores:" | cut -d: -f2 | xargs)
MEM_INFO=$(echo "$PLATFORM_INFO" | grep "^Memory:" | cut -d: -f2 | xargs)
RUST_INFO=$(echo "$PLATFORM_INFO" | grep "^Rust:" | cut -d: -f2 | xargs)

# Generate markdown report
cat > "$PERF_REPORT" << EOF
# Patina Performance Report

**Generated:** $(date '+%Y-%m-%d %H:%M:%S')
**Commit:** $(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
**Branch:** $(git branch --show-current 2>/dev/null || echo "unknown")
**Total Time:** ${ELAPSED_MIN}m ${ELAPSED_SEC}s

## Platform

| Property | Value |
|----------|-------|
| OS | $OS_INFO |
| CPU | $CPU_INFO |
| Cores | $CORES_INFO |
| Memory | $MEM_INFO |
| Rust | $RUST_INFO |

## Summary

- **Benchmarks Run:** $BENCH_COUNT
- **Mode:** $([ "$QUICK_MODE" = true ] && echo "Quick (10 samples)" || echo "Standard (20 samples)")

## Results

| Benchmark | Median Time | Category |
|-----------|-------------|----------|
EOF

# Add benchmark results to report
echo "$BENCH_RESULTS" | while IFS='|' read -r name time; do
    [ -z "$name" ] && continue
    # Determine category from benchmark name
    case "$name" in
        r7rs/*) category="R7RS Classic" ;;
        continuations/*) category="Continuations" ;;
        data/*) category="Data Structures" ;;
        numeric/*) category="Numeric" ;;
        *) category="Other" ;;
    esac
    echo "| \`$name\` | $time | $category |"
done >> "$PERF_REPORT"

# Add category summaries
cat >> "$PERF_REPORT" << 'EOF'

## Category Breakdown

EOF

# Calculate category statistics
echo "$BENCH_RESULTS" | awk -F'|' '
BEGIN {
    print "| Category | Count | Representative Benchmark | Time |"
    print "|----------|-------|-------------------------|------|"
}
{
    name = $1
    time = $2

    if (name ~ /^r7rs\//) {
        cat = "R7RS Classic"
        if (r7rs_count == 0) { r7rs_rep = name; r7rs_time = time }
        r7rs_count++
    } else if (name ~ /^continuations\//) {
        cat = "Continuations"
        if (cont_count == 0) { cont_rep = name; cont_time = time }
        cont_count++
    } else if (name ~ /^data\//) {
        cat = "Data Structures"
        if (data_count == 0) { data_rep = name; data_time = time }
        data_count++
    } else if (name ~ /^numeric\//) {
        cat = "Numeric"
        if (num_count == 0) { num_rep = name; num_time = time }
        num_count++
    }
}
END {
    if (r7rs_count > 0) printf "| R7RS Classic | %d | `%s` | %s |\n", r7rs_count, r7rs_rep, r7rs_time
    if (cont_count > 0) printf "| Continuations | %d | `%s` | %s |\n", cont_count, cont_rep, cont_time
    if (data_count > 0) printf "| Data Structures | %d | `%s` | %s |\n", data_count, data_rep, data_time
    if (num_count > 0) printf "| Numeric | %d | `%s` | %s |\n", num_count, num_rep, num_time
}
' >> "$PERF_REPORT"

# Add comparison section if history exists
if [ -f "$HISTORY_FILE" ]; then
    PREV_DATE=$(tail -1 "$HISTORY_FILE" | cut -d',' -f1)
    if [ -n "$PREV_DATE" ]; then
        cat >> "$PERF_REPORT" << EOF

## Historical Comparison

Previous run: $PREV_DATE

See \`$HISTORY_FILE\` for full history.
EOF
    fi
fi

# Add notes section
cat >> "$PERF_REPORT" << 'EOF'

## Notes

### Benchmark Sources

Benchmarks are ported from [ecraven/r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks),
the standard R7RS benchmark suite based on Gabriel and Larceny benchmarks.

### Running Benchmarks

```bash
# Full benchmark run (~5 min)
./scripts/run_benchmarks.sh

# Quick run (~2 min)
./scripts/run_benchmarks.sh --quick

# Run specific category
./scripts/run_benchmarks.sh --filter "r7rs/fib"
```

### Optimization Targets

See `PRD/phase1/CLONE_OPTIMIZATION_ANALYSIS.md` for identified optimization opportunities.
Key areas:
- CPS evaluator clone overhead
- Continuation environment (HashMap) cloning
- Dynamic wind record cloning
EOF

# Append to history file
DATE=$(date '+%Y-%m-%d %H:%M')
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Create history header if file doesn't exist
if [ ! -f "$HISTORY_FILE" ]; then
    echo "date,commit,benchmark,median_time" > "$HISTORY_FILE"
fi

# Append current results to history
echo "$BENCH_RESULTS" | while IFS='|' read -r name time; do
    [ -z "$name" ] && continue
    echo "$DATE,$COMMIT,$name,$time" >> "$HISTORY_FILE"
done

echo -e "${GREEN}Report generated: $PERF_REPORT${NC}"
echo ""

# Print summary table
echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}                        BENCHMARK SUMMARY                       ${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
echo ""

printf "${CYAN}%-45s %15s${NC}\n" "Benchmark" "Median Time"
printf "%-45s %15s\n" "---------------------------------------------" "---------------"

echo "$BENCH_RESULTS" | while IFS='|' read -r name time; do
    [ -z "$name" ] && continue
    # Color based on time (rough heuristic)
    if echo "$time" | grep -qE "^[0-9.]+ [nμµ]s"; then
        color=$GREEN  # nanoseconds/microseconds = fast
    elif echo "$time" | grep -qE "^[0-9.]+ ms" && [ "$(echo "$time" | cut -d' ' -f1 | cut -d'.' -f1)" -lt 100 ]; then
        color=$YELLOW  # < 100ms = medium
    else
        color=$RED  # >= 100ms = slow
    fi
    printf "${color}%-45s %15s${NC}\n" "$name" "$time"
done

echo ""
echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
echo -e "Total benchmarks: ${CYAN}$BENCH_COUNT${NC}"
echo -e "Total time:       ${CYAN}${ELAPSED_MIN}m ${ELAPSED_SEC}s${NC}"
echo -e "Report:           ${CYAN}$PERF_REPORT${NC}"
echo -e "History:          ${CYAN}$HISTORY_FILE${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
