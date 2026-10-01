#!/bin/bash
D=$(cd "$(dirname "$0")" && pwd)
OUT=$D/results/series.jsonl
run() { SERIES=1 PROBE_TIMEOUT=200 python3 "$D/run.py" "$@" >> "$OUT"; }
for impl in patina patina-tw chibi gosh chez; do run $impl steady-alloc 30; done
for impl in patina patina-tw chibi gosh chez; do run $impl deep-then-steady 1000000 10; done
for impl in patina patina-tw chibi gosh chez; do run $impl peak-then-drop 2000000 10; done
echo done >> "$D/results/series.done"
