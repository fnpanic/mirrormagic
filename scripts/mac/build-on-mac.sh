#!/bin/bash
# Runs ON THE MAC (npm is blocked there, so the frontend comes prebuilt in src.tgz).
# Expects in ~/mirrormagic-build: src.tgz, sign/cs.p12, sign/p12.pass (deleted after import).
# Builds, tests, signs with the stable "Tether Code Signing" cert (keeps the Accessibility grant
# across updates), installs to /Applications and starts it. Log: ~/mirrormagic-build/build.log
set -uo pipefail
B=~/mirrormagic-build
cd "$B" || exit 1
exec >"$B/build.log" 2>&1
log() { echo "[$(date +%H:%M:%S)] $*"; }
fail() { log "FAIL $*"; exit 1; }
log "start"
[ -f ~/.cargo/env ] && . ~/.cargo/env
rm -rf "$B/src" && mkdir -p "$B/src" "$B/out" && tar -xzf "$B/src.tgz" -C "$B/src" || fail untar

kc="$B/sign/tmp.keychain-db"
kp=$(openssl rand -hex 16)
orig=$(security list-keychains -d user | tr -d '"' | xargs)
cleanup() {
  security list-keychains -d user -s $orig
  security delete-keychain "$kc" 2>/dev/null
  rm -P -f "$B/sign/cs.p12" "$B/sign/p12.pass" 2>/dev/null
}
trap cleanup EXIT
security create-keychain -p "$kp" "$kc" &&
  security set-keychain-settings -lut 21600 "$kc" &&
  security unlock-keychain -p "$kp" "$kc" &&
  security import "$B/sign/cs.p12" -k "$kc" -P "$(cat "$B/sign/p12.pass")" -T /usr/bin/codesign >/dev/null &&
  security set-key-partition-list -S apple-tool:,apple: -s -k "$kp" "$kc" >/dev/null || fail "keychain import"
rm -P -f "$B/sign/cs.p12" "$B/sign/p12.pass"
security list-keychains -d user -s "$kc" $orig
log "certificate imported"

cd "$B/src"
log "rust $(rustc --version)"
log "tests"
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked|error(\[|:)" || true
cargo test --workspace >/dev/null 2>&1 || fail "tests"
log "ddc probe (read-only)"
(cd spikes/ddc-probe && cargo run -q 2>&1 | head -20)

cargo tauri --version 2>/dev/null | grep -q "2.12" || cargo install tauri-cli --version 2.12.4 --locked || fail "tauri-cli"
log "build"
(cd app && APPLE_SIGNING_IDENTITY="Tether Code Signing" cargo tauri build --bundles app \
  --config '{"build":{"beforeBuildCommand":"","frontendDist":"../dist"}}') || fail build
APP="$B/src/target/release/bundle/macos/MirrorMagic.app"
codesign --verify --deep --strict "$APP" || fail "codesign verify"
codesign -d -r- "$APP" 2>&1 | grep designated
(cd "$(dirname "$APP")" && ditto -c -k --keepParent MirrorMagic.app "$B/out/MirrorMagic_macos_arm64.zip")

log "install"
osascript -e 'quit app "MirrorMagic"' 2>/dev/null
sleep 2
pkill -f "/Applications/MirrorMagic.app/Contents/MacOS/" 2>/dev/null
rm -rf /Applications/MirrorMagic.app && ditto "$APP" /Applications/MirrorMagic.app || fail install
open -a /Applications/MirrorMagic.app
sleep 5
pgrep -f "/Applications/MirrorMagic.app/Contents/MacOS/" >/dev/null && log "BUILD OK: MirrorMagic running" || fail "app did not start"
