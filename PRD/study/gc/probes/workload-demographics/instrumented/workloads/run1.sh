#!/bin/bash
# usage: run1.sh BINARY BENCH [patina args...]
# Runs a workload (Larceny R7RS src+common.scm, or extra/<name>.scm) with its reduced input on stdin.
SCRATCH=$(cd "$(dirname "$0")/../.." && pwd)
W=$SCRATCH/instrumented/workloads
BIN=$1; B=$2; shift 2
case "$BIN" in
  *demog*) export PATINA_LIBRARY_PATH=$SCRATCH/instrumented/demographics/lib ;;
  *) export PATINA_LIBRARY_PATH=~/Project/patina/lib ;;
esac
cd $W/inputs
if [ -f $W/extra/$B.scm ]; then SRC=$W/extra/$B.scm; else SRC=$W/src/$B.scm; fi
if [ -f $W/inputs/$B.input ]; then IN=$W/inputs/$B.input; else IN=/dev/null; fi
exec "$BIN" "$@" "$SRC" < "$IN"
