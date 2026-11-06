#!/bin/bash
# Runs ON THE MAC from launchd (QueueDirectories, see install-runner.sh): every file named
# <commit> dropped into ~/mirrormagic-build/queue builds src-<commit>.tgz with build-on-mac.sh.
# Lets the PC start Mac builds through Tether file transfer alone, without typing on the Mac's
# screen (other agents may be driving it).
B=~/mirrormagic-build
for f in "$B"/queue/*; do
  [ -e "$f" ] || continue
  c=$(basename "$f")
  rm -f "$f"
  [[ "$c" =~ ^[0-9a-f]{7,40}$ && -f "$B/src-$c.tgz" ]] || { echo "$(date) ignored $c"; continue; }
  echo "$(date) build $c"
  cp "$B/src-$c.tgz" "$B/src.tgz" && bash "$B/build-on-mac.sh"
  echo "$(date) done $c: $(tail -1 "$B/build.log")"
done
