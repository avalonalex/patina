#!/bin/bash
# usage: run.sh <impl> <file>   impl: vm|tw|chibi|gosh|chez
S=$(cd "$(dirname "$0")/.." && pwd)
P=$S/target/release/patina
impl=$1; f=$2
cd ~/Project/patina
case $impl in
  vm) timeout -s KILL ${T:-60} $P "$f" >$S/n1/out.txt 2>&1; rc=$? ;;
  tw) timeout -s KILL ${T:-60} $P --tree-walker "$f" >$S/n1/out.txt 2>&1; rc=$? ;;
  chibi) timeout -s KILL ${T:-60} chibi-scheme "$f" >$S/n1/out.txt 2>&1; rc=$? ;;
  gosh) timeout -s KILL ${T:-60} gosh -r7 "$f" >$S/n1/out.txt 2>&1; rc=$? ;;
  chez) timeout -s KILL ${T:-60} chez --script "$f" >$S/n1/out.txt 2>&1; rc=$? ;;
esac
echo "$impl rc=$rc : $(tail -c 300 $S/n1/out.txt | tr '\n' ' ' | cut -c1-200)"
