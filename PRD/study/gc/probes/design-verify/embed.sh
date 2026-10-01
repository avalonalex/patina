#!/bin/bash
V=$(cd "$(dirname "$0")" && pwd)/repro/target
S=$(cd "$(dirname "$0")" && pwd)/run
export PATINA_LIBRARY_PATH=~/Project/patina/lib
cd $S
for p in debug release; do for b in "" tw; do echo "== $p uaf $b"; timeout -s KILL 120 $V/$p/uaf $b 2>&1 | grep -v "^note:" | head -4; done; echo "== $p resilient"; timeout -s KILL 120 $V/$p/resilient 2>&1 | grep -v "^note:\|^Error\|^  " | head -4; done
for b in "" tw; do rm -f td$b.txt; echo "== release teardown $b"; timeout -s KILL 120 $V/release/teardown $S/td$b.txt $b; echo "file bytes after exit: $(wc -c < $S/td$b.txt)"; done
