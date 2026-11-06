# Changelog

## 0.5.1 — 2026-11-06

- **Fixed: after a monitor switch the Mac cursor could stick to the top edge** and only move sideways until the next switch. macOS applies the mirror layout asynchronously; when the shared monitor's area was read before it moved (from above the laptop screen to 0,0), every move was clamped into the old rectangle. While the PC controls it, the Mac now re-reads the display layout every 500 ms and moves the session to the new area. That also covers HDMI re-plugs and resolution changes mid-session.
- Mac builds can start without typing on the Mac: a LaunchAgent build runner, triggered by file transfer (see docs/macos.md).

## 0.5.0 — 2026-11-06

- **Copy files through the clipboard.** Copy files or folders in Explorer or Finder, switch to the other machine, paste. They travel on their own encrypted connection, so input never waits behind a big file. The status shows progress, and the files land in a temporary MirrorMagic folder that is cleaned after a day. Paths from the peer are sanitised (no `..`, absolute paths or drive letters). Limits: 4 GB and 50,000 items per copy.
- Protocol version 3, and **version negotiation**: the PC answers with the lower of both versions and never sends a peer messages it does not know. Updating PC and Mac in either order keeps keyboard, mouse and clipboard working; file copy needs 0.5 on both.

## 0.4.0 — 2026-11-06

- **The PC camera runs only while an app uses it.** macOS reports OBS's virtual camera as "in use" whenever OBS feeds it, which kept the webcam on all the time. Now the virtual camera stays off while idle; when an app opens "OBS Virtual Camera", the webcam, its microphone and OBS start. MirrorMagic follows the macOS CoreMediaIO log for camera-control events (the method Objective-See's OverSight uses); when an app releases the camera, it checks once whether anyone still watches and turns everything off. During a call there is no interruption except a 10-minute safety check. Without access to that log it falls back to a check every 45 s (OBS paused for about half a second).
- **Monitor hotkey:**
  - Ctrl+Alt+M (and other AltGr letters such as Q, E, 7–0 on German keyboards) can be recorded again. The browser reports those as AltGraph instead of Ctrl+Alt.
  - New **Reset** button restores Ctrl+Alt+M.
- **PC app robustness:**
  - The keyboard hook re-installs itself every 2 s and runs at top priority. Windows silently drops a late hook, which had stopped typing on the Mac and the hotkey.
  - Panics in hook callbacks are caught.
  - All panics are written to MirrorMagic.log.
- Fixed double-encoded characters in the settings window.
- Protocol version 2. A 0.2 Mac talking to a 0.3 PC used to reconnect in a loop; mismatched versions are now refused cleanly.

## 0.3.0 — 2026-11-05

- **PC microphone on the Mac.** The webcam's microphone streams together with the camera (10 ms chunks, sent ahead of video on the camera connection). On the Mac it plays into the **BlackHole 2ch** virtual audio device; pick "BlackHole 2ch" as the microphone in your app. On the PC you can turn this off under Camera → Microphone.
- OBS sets itself up when an app opens the camera. If OBS runs without remote control, MirrorMagic restarts it once with remote control enabled, so you no longer have to quit OBS and press Set up.
- Diagnostic log `MirrorMagic.log` (next to config.json) covering camera, OBS, microphone, clipboard and mirroring.

## 0.2.0 — 2026-11-05

- Monitor mode shows the Mac's **main screen**. While it is on, the shared monitor is the master of a mirror set at its full native resolution (no letterboxing) and the MacBook screen mirrors it; leaving monitor mode restores the extended layout. A crash during monitor mode is undone at the next start.
- **Ctrl works as ⌘** on the Mac (Mac setting, on by default), so Ctrl+C and Ctrl+V copy and paste like on Windows. The Win key becomes Control.
- The settings show the last clipboard hand-over ("sent text (12 chars), formatting" / "received …").
- Code-review fixes:
  - Cursor hide/show runs on one worker.
  - The park point is re-measured.
  - Stale key ownership is pruned (Ctrl+Alt+Del, UAC).
  - Replacement sessions re-enter.
  - DDC requests are serialized.
  - The camera is generation-tagged.
  - Clipboard size is capped.
  - Hello has a timeout.
  - The Mac tracks all five mouse buttons.
  - OBS websocket settings are written to user.ini.
- Safety: the PC hands over control only once the Mac has Accessibility.

## 0.1.0 — 2026-11-05

First release.

- Edge crossing from the PC to the Mac and back:
  - relative motion with the PC cursor hidden and parked
  - no crossing while dragging
  - Scroll Lock as the emergency exit
- Monitor mode on **Ctrl+Alt+M**: DDC/CI switches the shared monitor to the Mac's HDMI input and moves keyboard and mouse there. The switch back is sent from both the PC and the Mac.
- The clipboard travels with every switch: text, formatted text (HTML) and images. Clipboard managers on both sides see it.
- The PC webcam becomes the Mac's camera through OBS Virtual Camera, on demand (the webcam runs only while an app uses it).
- Noise NNpsk0 encryption with a pairing key; LAN discovery.
- Input handling:
  - German AltGr fix-ups
  - ISO key swap
  - double-click and drag
  - vertical and horizontal wheel
- Tray/menu-bar app with a settings window; starts with Windows or at login.
- Windows installer: firewall rule for the local subnet only.
