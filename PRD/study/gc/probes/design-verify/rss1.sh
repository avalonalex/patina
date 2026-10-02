#!/bin/bash
S=$(cd "$(dirname "$0")" && pwd); P=~/Project/patina/target/release/patina; export PATINA_LIBRARY_PATH=~/Project/patina/lib; cd $S/run
for cmd in "chibi-scheme bigvec-plain.scm" "gosh -r7 bigvec-plain.scm" "chibi-scheme captures-plain.scm" "gosh -r7 captures-plain.scm" "$P empty.scm" "$P --tree-walker captures.scm" "$P bigvec.scm" "$P captures.scm"; do
 /usr/bin/time -l timeout -s KILL 300 $cmd >/dev/null 2>$S/rss.txt; m=$(grep "maximum resident set size" $S/rss.txt | awk '{print $1}'); printf "%-40s %.1f MB\n" "${cmd##*/}" $(echo "$m/1000000" | bc -l); done
