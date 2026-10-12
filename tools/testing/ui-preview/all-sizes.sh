#!/bin/sh
# Screenshots every Deck page at one window size: all-sizes.sh <folder> <w> <h> <port>
out=$1; w=$2; h=$3; export PORT=${4:-9333}; mkdir -p "$out"
grep -E "^[a-z]" all.sh | grep -v "^out=\|^while\|^done\|^  node" | while read -r page sub; do
  node shot.mjs "$page" "${sub:--}" "$out/${page}_${sub:--}.png" $w $h 2>&1 | grep -E "^(EXC|ERR)" | sed "s/^/[$page $sub ${w}x$h] /"
done
