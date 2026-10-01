#!/bin/bash
# usage: scaled.sh IMPL...   runs each probe at N and 4N
D=$(cd "$(dirname "$0")" && pwd)
OUT=$D/results/scaled.jsonl
for impl in "$@"; do
while read -r probe n extra; do
  [ -z "$probe" ] && continue
  case $impl in chez) [ -f "$D/chez/$probe.ss" ] || continue;; esac
  for mult in 1 4; do
    N=$((n*mult))
    python3 "$D/run.py" $impl $probe $N $extra | tee -a $OUT | python3 "$D/fmt.py"
  done
done <<LIST
sym-churn 250000
eval-fresh-names 25000
eval-redefine 50000
eval-lambda 50000
env-churn 10000
env-lambda 10000
hidden-define 25000
define-values-null 25000
record-redefine 10000
unbound-ref 25000
port-churn 50000
cont-churn 20000 100
load-repeat 2000 $D/probes/loaded.scm
file-port-churn 5000 $D/results/fp-$impl.txt
LIST
done
