#!/bin/sh
# Screenshots every Deck page into the folder given as $1 (default ./shots) using the fake backend.
out=${1:-shots}; mkdir -p "$out"
while read -r page sub; do
  node shot.mjs "$page" "$sub" "$out/${page}_${sub}.png" 1260 900 2>&1 | grep -E "^(EXC|ERR)" | grep -v "outputs" | sed "s/^/[$page $sub] /"
done <<'LIST'
home -
sensors -
system network
boost -
clipboard -
audio -
power timers
power awake
power plan
cpu -
gpu -
fans -
lighting -
lcd -
overlay -
controller overlay
controller keys
controller battery
controller test
deaths now
deaths bosses
deaths history
deaths games
deaths settings
drives -
thispc -
windows open
windows layouts
windows workspaces
system display
system processes
system startup
files space
files cleanup
tools -
tweaks overview
tweaks privacy
tweaks gaming
tweaks apps
tweaks store
tweaks changed
transfer -
claudesync -
phone -
settings -
LIST
