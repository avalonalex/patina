#!/bin/bash
S=$(cd "$(dirname "$0")" && pwd)
P=~/Project/patina/target/release/patina
export PATINA_LIBRARY_PATH=~/Project/patina/lib
cd $S/run
t() { timeout -s KILL 120 "$@"; }
echo "### A3 eq"
for b in "" "--tree-walker"; do echo "patina $b:"; t $P $b eq.scm; echo "exit=$?"; done
echo "chibi:"; t chibi-scheme eq.scm; echo "exit=$?"
echo "gosh:"; t gosh -r7 eq.scm; echo "exit=$?"
echo "### A4 emfile (ulimit -n 1024)"
for b in "" "--tree-walker"; do echo "patina $b:"; /bin/sh -c "ulimit -n 1024; exec timeout -s KILL 120 $P $b emfile.scm"; echo "exit=$?"; done
echo "chibi:"; /bin/sh -c "ulimit -n 1024; exec timeout -s KILL 120 chibi-scheme emfile.scm"; echo "exit=$?"
echo "gosh:"; /bin/sh -c "ulimit -n 1024; exec timeout -s KILL 120 gosh -r7 emfile.scm"; echo "exit=$?"
echo "### A5 p1 q2"
for f in p1 q2; do
 for b in "" "--tree-walker"; do echo "$f patina $b:"; t $P $b $f.scm 2>&1 | tr '\n' ' '; echo "exit=${PIPESTATUS[0]}"; done
 echo "$f chibi:"; t chibi-scheme $f.scm 2>&1 | tr '\n' ' '; echo
 echo "$f gosh:"; t gosh -r7 $f.scm 2>&1 | tr '\n' ' '; echo
done
echo "### A9 deflib2"
for b in "" "--tree-walker"; do echo "patina $b:"; t $P $b deflib2.scm 2>&1; echo "exit=$?"; done
echo "chibi:"; t chibi-scheme deflib2.scm 2>&1; echo "exit=$?"
echo "gosh:"; t gosh -r7 deflib2.scm 2>&1; echo "exit=$?"
echo "deflib (non-top-level):"; for b in "" "--tree-walker"; do t $P $b deflib.scm 2>&1; done
