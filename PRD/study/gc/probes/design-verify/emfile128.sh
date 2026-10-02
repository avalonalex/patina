#!/bin/bash
cd "$(dirname "$0")/run"
P=~/Project/patina/target/release/patina
export PATINA_LIBRARY_PATH=~/Project/patina/lib
for f in emfile5k emfile5k-out; do
 for c in "$P" "$P --tree-walker" "chibi-scheme" "gosh -r7"; do echo -n "$f | ${c##*/}: "; /bin/sh -c "ulimit -n 128; exec timeout -s KILL 120 $c $f.scm" 2>&1 | head -2 | tr '\n' ' '; echo; done
done
