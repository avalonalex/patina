#!/bin/bash
# reads "impl probe args..." lines from stdin; appends JSON to $OUT
D=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$D/results/batch.jsonl}
while read -r impl probe rest; do
  [ -z "$impl" ] && continue
  rest=${rest//@D/$D}
  python3 "$D/run.py" $impl $probe $rest | tee -a "$OUT" | python3 "$D/fmt.py"
done
