#!/bin/bash
# Run the chibi-scheme r7rs test suite under the CPS tree-walker backend.
#
# Thin wrapper: the two backends shared a ~250-line script that differed only in
# a flag and two filenames, so the parsing lives in one place now. Kept as its
# own entry point because docs/VM_TESTING.md and CLAUDE.md reference it by name.
exec "$(dirname "$0")/run_chibi_tests.sh" --tree-walker "$@"
