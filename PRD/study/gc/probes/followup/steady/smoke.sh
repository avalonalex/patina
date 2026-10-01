#!/bin/bash
D=$(cd "$(dirname "$0")" && pwd)
while read -r line; do
  [ -z "$line" ] && continue
  PATINA_STATS=1 python3 "$D/run.py" patina $line
done <<LIST
sym-churn 1000
eval-fresh-names 1000
eval-redefine 1000
eval-lambda 1000
env-churn 100
env-define 100
hidden-define 1000
define-values-null 1000
record-redefine 1000
unbound-ref 1000
port-churn 1000
cont-churn 100 100
load-repeat 100 $D/probes/loaded.scm
file-port-churn 100 $D/results/fp.txt
LIST
