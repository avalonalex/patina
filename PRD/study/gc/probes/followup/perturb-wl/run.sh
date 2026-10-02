#!/bin/bash
# usage: run.sh BIN WORKLOAD  -> prints wall seconds (and exit status on failure)
HERE=$(cd "$(dirname "$0")" && pwd)
SCR=$(cd "$HERE/../.." && pwd)
W=$SCR/workload-demographics/instrumented/workloads
export PATINA_LIBRARY_PATH=$SCR/followup/patina-perturb/lib
BIN=$1; B=$2
if [ -f $HERE/$B.scm ]; then SRC=$HERE/$B.scm; IN=/dev/null
elif [ -f $W/extra/$B.scm ]; then SRC=$W/extra/$B.scm; IN=$W/inputs/$B.input
else SRC=$W/src/$B.scm; IN=$W/inputs/$B.input; fi
[ -f "$IN" ] || IN=/dev/null
cd $W/inputs
s=$(python3 -c 'import time;print(time.perf_counter())')
out=$(timeout -s KILL 120 "$BIN" "$SRC" < "$IN" 2>&1); st=$?
e=$(python3 -c 'import time;print(time.perf_counter())')
python3 -c "print(f'{$e-$s:.4f}')"
[ $st -ne 0 ] && echo "FAIL $st: ${out:0:200}" >&2
exit 0
