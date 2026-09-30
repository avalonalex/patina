#!/bin/bash
# Correctness-checked execution comparison. See docs/VM_TESTING.md.
set -euo pipefail
exec python3 "$(dirname "$0")/benchmarks.py" compare "$@"
