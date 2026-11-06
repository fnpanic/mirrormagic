#!/bin/bash
# Runs ON THE MAC once (typed in Terminal): installs the build runner LaunchAgent, then queues a
# build of commit $1 (optional). After this, builds start by uploading src-<c>.tgz and a file
# named <c> into ~/mirrormagic-build/queue.
set -e
B=~/mirrormagic-build
P=~/Library/LaunchAgents/com.mirrormagic.buildrunner.plist
mkdir -p "$B/queue" ~/Library/LaunchAgents
cat >"$P" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.mirrormagic.buildrunner</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>$B/runner.sh</string></array>
  <key>QueueDirectories</key><array><string>$B/queue</string></array>
  <key>AbandonProcessGroup</key><true/>
  <key>StandardOutPath</key><string>$B/runner.log</string>
  <key>StandardErrorPath</key><string>$B/runner.log</string>
</dict></plist>
EOF
launchctl bootout "gui/$(id -u)" "$P" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$P"
echo "runner installed"
if [ -n "${1:-}" ]; then echo build >"$B/queue/$1" && echo "build $1 queued: tail -f $B/build.log"; fi
