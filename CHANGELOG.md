# Changelog

## 0.5.5 — 2026-11-06 (Mac)

- **The Mac's cursor stays visible after you go back to the PC.** It used to wait in the last pixel column of the shared edge, where macOS draws the arrow off-screen, so the Mac seemed to have no cursor until you crossed over from Windows again. It now waits 48 points inside the edge, at the height where you left, ready for the Mac's own trackpad.
- Leaving monitor mode puts the Mac's cursor in the middle of the laptop screen. Otherwise it could stay on the shared monitor's area, which shows the PC again.

## 0.5.4 — 2026-11-06 (PC only)

- Game mode no longer switches on for a moment at every Alt+Tab. The Windows 11 task switcher is a full-screen Explorer window (`XamlExplorerHostIslandWindow`), so it counted as a full-screen app for about a second and briefly blocked crossing. Shell overlays are excluded now, and the log names the window class that turned game mode on.

## 0.5.3 — 2026-11-06 (PC only)

- **Game mode.** While a full-screen app is in front (a game in exclusive or borderless full-screen, F11 video, a presentation) and you work on the PC, MirrorMagic removes its keyboard and mouse hooks completely. The game gets its input exactly as without MirrorMagic, with zero added latency, and the pointer can't cross to the Mac at the screen edge. A maximized window is not full-screen. Within half a second of leaving the game (Alt+Tab, closing it), the hooks are back. The monitor hotkey doesn't work while game mode is on. The PC window shows "Game mode", and MirrorMagic.log has `game mode on/off`.

## 0.5.2 — 2026-11-06 (PC only)

- **Fixed: after Windows turned the screen off (you were away), the monitor stayed on the Mac.** With the PC's DisplayPort signal gone, the monitor's automatic input detection jumped to the Mac's HDMI signal and stayed there, so you had to unplug HDMI. MirrorMagic now notices when Windows turns the screen back on. Unless monitor mode deliberately shows the Mac, it reads the monitor's input over DDC and, if it isn't the PC's, switches back from both the PC and the Mac. Log lines: `display off (Windows power saving)`, `display on: …`.

## 0.5.1 — 2026-11-06

- **Fixed: after a monitor switch the Mac cursor could stick to the top edge** and only move sideways until the next switch. macOS applies the mirror layout asynchronously; when the shared monitor's area was read before it moved (from above the laptop screen to 0,0), every move was clamped into the old rectangle. While the PC controls it, the Mac now re-reads the display layout every 500 ms and moves the session to the new area. That also covers HDMI re-plugs and resolution changes mid-session.
- Verified end-to-end on the real machines: Photo Booth opening "OBS Virtual Camera" turned the PC webcam and microphone on within a second, and quitting it turned them off 21 s later. The monitor hotkey UI was checked in a headless browser (`app/e2e/hotkey.e2e.mjs`): record a combo, Reset to Ctrl+Alt+M, and record Ctrl+Alt+M by keyboard again (also as AltGr).
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
