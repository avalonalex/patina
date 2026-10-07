#!/bin/bash
# The GC benchmark set, absolute or against a base. See docs/TEST_ORGANIZATION.md.
set -euo pipefail
exec python3 "$(dirname "$0")/benchmarks.py" gc "$@"
