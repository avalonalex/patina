#!/bin/bash
# usage: rss.sh N cmd...   -> prints RSS (MiB) while the program idles after the drop, and max RSS
N=$1; shift
D=$(dirname "$0")
fifo=$D/fifo.$$; rm -f $fifo; mkfifo $fifo
out=$D/out.$$
( timeout -s KILL 120 /usr/bin/time -l "$@" $N < $fifo > $out 2> $out.err ) &
exec 3> $fifo
for i in $(seq 1 200); do grep -q idle $out 2>/dev/null && break; sleep 0.1; done
sleep 0.5
pid=$(pgrep -n -f "$(basename ${@: -1})")
rss=$(ps -o rss= -p $pid)
echo >&3; exec 3>&-
wait
max=$(grep "maximum resident" $out.err | awk '{print $1}')
printf "N=%s idle-RSS=%.1f MiB max-RSS=%.1f MiB\n" $N $(echo "$rss/1024" | bc -l) $(echo "$max/1048576" | bc -l)
rm -f $fifo $out $out.err
