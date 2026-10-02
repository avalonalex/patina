#!/bin/bash
S=$(cd "$(dirname "$0")" && pwd)
P=~/Project/patina/target/release/patina
export PATINA_LIBRARY_PATH=~/Project/patina/lib
cd $S/run
rss() { /usr/bin/time -l timeout -s KILL 300 "$@" 2>/tmp/_rss_$$ ; st=$?; m=$(grep "maximum resident set size" /tmp/_rss_$$ | awk '{print $1}'); echo "  exit=$st peakRSS=$((m/1000000)) MB"; rm -f /tmp/_rss_$$; }
echo "### A7"
for f in bigvec captures empty; do for b in "" "--tree-walker"; do echo "patina $b $f:"; rss $P $b $f.scm; done; done
for f in bigvec-plain captures-plain; do echo "chibi $f:"; rss chibi-scheme $f.scm; echo "gosh $f:"; rss gosh -r7 $f.scm; done
echo "### A8"
for n in 1000 4000 8000 16000; do for o in forward reverse; do for b in "" "--tree-walker"; do echo -n "patina $b: "; timeout -s KILL 300 $P $b ephem-chain.scm $n $o; done; done; done
