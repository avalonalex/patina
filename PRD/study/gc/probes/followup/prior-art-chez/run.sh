#!/bin/bash
C=$(cd "$(dirname "$0")/../chez" && pwd)  # holds src-t/ and src-nt/, Chez built with and without threads (not retained)
B=$(dirname "$0")
t() { timeout -s KILL 300 $C/src-t/tarm64osx/bin/tarm64osx/scheme -b $C/src-t/tarm64osx/boot/tarm64osx/petite.boot -b $C/src-t/tarm64osx/boot/tarm64osx/scheme.boot "$@"; }
n() { timeout -s KILL 300 $C/src-nt/arm64osx/bin/arm64osx/scheme -b $C/src-nt/arm64osx/boot/arm64osx/petite.boot -b $C/src-nt/arm64osx/boot/arm64osx/scheme.boot "$@"; }
echo '(begin (display (threaded?)) (newline))' | t -q
echo '(begin (display (threaded?)) (newline))' | n -q
for r in 1 2 3 4 5; do
  n --script $B/bench.ss | sed "s/^/NT\t$r\t/"
  t --script $B/bench.ss | sed "s/^/T\t$r\t/"
done
