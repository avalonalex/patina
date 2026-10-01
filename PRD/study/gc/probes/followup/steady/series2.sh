#!/bin/bash
D=$(cd "$(dirname "$0")" && pwd)
OUT=$D/results/series2.jsonl
run() { SERIES=1 PROBE_TIMEOUT=200 python3 "$D/run.py" "$@" >> "$OUT"; }
while [ ! -f "$D/results/series.done" ]; do sleep 2; done
for impl in patina patina-tw chibi gosh chez; do run $impl steady-alloc 30; done
for impl in patina chibi gosh chez; do run $impl deep-then-steady 100000 5; done
run patina-tw peak-then-drop 2000000 30
echo done > "$D/results/series2.done"
