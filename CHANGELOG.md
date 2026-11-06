# Changelog

## 0.5.10 — 2026-11-06 (Mac)

- After control goes back to the PC, the Mac sends a real 1-point move there and back. Apps such as Terminal hide the pointer while you type until the mouse moves, and that state could keep the Mac's pointer hidden after you switched back.
- Diagnostics: while the Mac is in charge, the log records whether the pointer is visible and which display it is on, once per change (`pointer (Mac in charge): …`).

## 0.5.9 — 2026-11-06 (Mac)

- **Keys keep the Mac keyboard's positions:** Ctrl = Control, Win = Option, Alt = Command. Before, Ctrl acted as Command, which left no Command key at Alt's position. The right Alt (AltGr) stays Option, with the German fix-ups for @ € { } [ ] \ | ~. The old behaviour is the "Windows shortcuts" setting (off by default). The former "Ctrl works as ⌘" setting is not carried over.
- **F-keys work like a MacBook's top row:** F1/F2 brightness, F3 Mission Control, F4 Spotlight (⌘ Space), F7/F8/F9 back/play/forward, F10 mute, F11/F12 volume. F5 and F6 stay F5 and F6 (Apple's dictation and Do Not Disturb keys have no documented code). **Win+F-key** sends the real F1–F12 without modifiers, like fn on a Mac. The PC keyboard's own media keys (volume, play, next/previous) act as Mac media keys too. Media keys are posted as NSSystemDefined events like the MacBook sends them.

## 0.5.8 — 2026-11-06 (PC only)

- Diagnostics for the 0.5.6 drift fix: when the PC's hidden cursor has slipped off its park point while you control the Mac, the log says `hidden cursor slipped by x,y px: corrected (would have pulled the Mac cursor)`, at most once a minute. Each such line is a pull that 0.5.5 and earlier would have shown on the Mac.

## 0.5.7 — 2026-11-06

- **The PC screen stays on while you work on the Mac with its own keyboard and trackpad.** Windows saw no input and turned its screen off after its timeout. The monitor's auto input detection then jumped to the Mac's HDMI signal and showed the Mac's second screen while you were working on the MacBook. While the Mac has input, it now tells the PC every 30 s (protocol 5), and the PC resets its display timer. When you are away from both, the PC screen turns off as usual, and the Mac puts its displays to sleep after 60 s (0.5.6). The away test of 0.5.6 showed the gap: the Mac received "PC screen off" but was in use, so it rightly kept its displays on.

Verified on the real machines: from 14:49 to 15:04 the user worked only on the MacBook ("Mac in use" at 14:49 and 14:59 in the PC log). The PC screen stayed on for about 14.5 minutes without input, beyond Windows' 5-minute timeout. Before, `display off` followed every 5 minutes, and the monitor jumped to the Mac.

## 0.5.6 — 2026-11-06

- **Fixed: the Mac cursor was sometimes pulled left or right on its own** (PC fix). While you control the Mac, the PC's hidden cursor waits at a park point and every move is sent as the distance from it. Moves that slipped past the input hook (during its 2 s re-install, or a hook Windows dropped) shifted the hidden cursor, and every later move carried that shift as a constant offset, so the Mac cursor drifted toward one side until the next switch. Each move is now measured from where the hidden cursor really is.
- **The monitor no longer shows the Mac's second screen while you are away.**
  - While you control the Mac with the PC's mouse, the PC keeps its screen awake, because the hook can swallow the input that would otherwise keep it on.
  - When Windows turns its screen off, it tells the Mac (protocol 4). Once nobody has touched the Mac for 60 s either, the Mac puts its displays to sleep (`pmset displaysleepnow`), so the monitor gets no signal and goes to standby instead of showing the Mac. This doesn't happen while a program keeps the display awake (video, `caffeinate -d`, a test run), and any input wakes the Mac.
  - When you come back, the 0.5.2 switch-back puts the monitor on the PC.

## 0.5.5 — 2026-11-06 (Mac)

- **Fixed: the Mac cursor drifted and jumped (since 0.5.1).** While the screen was mirrored for monitor mode, macOS alternated which display of the mirror set it reported as active. The half-second layout check from 0.5.1 then flipped the cursor area between the laptop's size and the monitor's, and the cursor jumped or got clamped. The display list now holds all connected displays and leaves out a display that only mirrors another. In addition, the real cursor position overrides the tracked one only if it is more than 3 points away (the Mac's own trackpad moved it), so a rounded or late read-back can't nudge the cursor. When the layout changes, the cursor is clamped into the new area instead of jumping to its middle.
- **The Mac's cursor stays visible after you go back to the PC.** It used to wait in the last pixel column of the shared edge, where macOS draws the arrow off-screen, so the Mac seemed to have no cursor until you crossed over from Windows again. It now waits 48 points inside the edge, at the height where you left, ready for the Mac's own trackpad.
- Leaving monitor mode puts the Mac's cursor in the middle of the laptop screen. Otherwise it could stay on the shared monitor's area, which shows the PC again.

### Verified on the real machines (2026-11-06)

- Away (0.5.2): screen off for 30 s at 12:24:26. The monitor's auto input detection jumped to the Mac (input 0x11). When the screen came back at 12:24:58, MirrorMagic switched back to DisplayPort, and a DDC read confirmed input 0x0F.
- Game mode (0.5.3/0.5.4): a borderless full-screen window removed the hooks at 12:24:21, and they came back at 12:24:24 after it closed. Since 0.5.4, Alt+Tab no longer appears as game mode in the log.
- Mac cursor (0.5.5): after going back to Windows, both screens show a cursor, and the Mac can be used with its own trackpad (confirmed by the user).

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
