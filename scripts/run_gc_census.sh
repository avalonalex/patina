#!/bin/bash
# The GC census over the GC benchmark set (#651). See docs/TEST_ORGANIZATION.md.
set -euo pipefail
exec python3 "$(dirname "$0")/benchmarks.py" census "$@"
