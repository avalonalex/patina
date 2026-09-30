#!/bin/bash
# Criterion measurements with fresh backend-qualified results and metadata.
set -euo pipefail
exec python3 "$(dirname "$0")/benchmarks.py" criterion "$@"
