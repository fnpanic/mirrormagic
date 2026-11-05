# Changelog

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
