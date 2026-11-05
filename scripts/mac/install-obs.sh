#!/bin/bash
# Runs ON THE MAC: installs OBS from ~/mirrormagic-build/OBS.dmg after checking it is the
# notarized build signed by the OBS Project. Log: ~/mirrormagic-build/obs.log
B=~/mirrormagic-build
exec >"$B/obs.log" 2>&1
set -u
mnt=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$B/OBS.dmg" || { echo "FAIL attach"; exit 1; }
app="$mnt/OBS.app"
sig=$(spctl -a -vv -t exec "$app" 2>&1)
echo "$sig"
if echo "$sig" | grep -q "accepted" && echo "$sig" | grep -q "Notarized Developer ID" && codesign -dv "$app" 2>&1 | grep -q "TeamIdentifier=2MMRE5MTB8"; then
  rm -rf /Applications/OBS.app && ditto "$app" /Applications/OBS.app && echo "OBS INSTALLED"
else
  codesign -dv "$app" 2>&1 | grep -E "Authority|TeamIdentifier"
  echo "FAIL signature check"
fi
hdiutil detach "$mnt" >/dev/null
rm -f "$B/OBS.dmg"
