#!/bin/bash
# usage: run.sh <impl> <prog.scm> [extra flags]; runs in run-<impl>/ and prints rc
EXP=$(cd "$(dirname "$0")" && pwd)
PAT=~/Project/patina/target/release/patina
export PATINA_LIBRARY_PATH=~/Project/patina/lib
impl=$1; prog=$2; shift 2
case $impl in
  chibi) dir=run-chibi; cmd=(chibi-scheme "$prog");;
  gauche) dir=run-gauche; cmd=(gosh -r7 "$prog");;
  patina) dir=run-patina; cmd=($PAT "$@" "$prog");;
  patina-tw) dir=run-patina; cmd=($PAT --tree-walker "$@" "$prog");;
esac
cd $EXP/$dir
timeout -s KILL ${TMO:-60} "${cmd[@]}"; rc=$?
echo "[$impl $prog rc=$rc]"
