# Changelog

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
