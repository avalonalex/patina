#!/bin/bash
# usage: larceny.sh <on|off> <vm|tw> suites...
EXP=$(cd "$(dirname "$0")" && pwd)
PAT=$EXP/bin/patina; gc=$1; be=$2; shift 2
mkdir -p $EXP/larceny-logs
cd $EXP/suites/lib-$gc-$be
for s in "$@"; do
  args=(--isolated-libraries -k); [ $be = tw ] && args+=(--tree-walker)
  log=$EXP/larceny-logs/$s-$gc-$be.log
  if [ $gc = off ]; then e=PATINA_GC=0; else e=X=1; fi
  start=$(date +%s)
  env $e timeout -s KILL 400 $PAT "${args[@]}" -I . tests/scheme/run/$s.sps </dev/null > $log 2>&1; rc=$?
  end=$(date +%s)
  summ=$(grep -E '^[0-9]+ tests passed$|^[0-9]+ of [0-9]+ tests failed\.$' $log | tail -1)
  echo "$s gc=$gc be=$be rc=$rc t=$((end-start))s: ${summ:-NO-SUMMARY}"
done
