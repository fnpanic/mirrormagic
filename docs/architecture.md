# Architecture

## Crates and modules

| Path | What |
|---|---|
| `crates/mm-core/src/proto.rs` | Wire messages (postcard). `PROTOCOL_VERSION` must match on both sides. |
| `crates/mm-core/src/secure.rs` | Noise `NNpsk0` channel and the pairing key (Crockford base32, 26 chars). Messages are chunked over 64 KiB Noise frames. |
| `crates/mm-core/src/primary.rs` | PC state machine: edge detection, key/button ownership, monitor-mode toggle, emergency exit. Pure and unit-tested. |
| `crates/mm-core/src/secondary.rs` | Mac cursor logic: entry point, clamping per display, return edge. |
| `crates/mm-core/src/keymap.rs` | Scan code → macOS key code (ISO swap), hotkey parser, German AltGr → Option translation, modifier flags. |
| `app/src-tauri/src/pc.rs` + `win/` | Windows runtime: `WH_KEYBOARD_LL`/`WH_MOUSE_LL` hooks, cursor park/hide, TCP server, UDP discovery, DDC, webcam. |
| `app/src-tauri/src/mac/` | macOS runtime: CGEvent injection, displays, Accessibility, CoreMediaIO "camera in use", OBS websocket automation. |
| `app/src-tauri/src/clipboard.rs` | Clipboard hand-over on switches (text, HTML, PNG). |
| `app/src-tauri/src/mjpeg.rs` | Localhost `multipart/x-mixed-replace` server that OBS reads, with a placeholder frame between streams. |
| `app/src` | React settings window (same design tokens as Tether). |

## Session flow

1. The Mac connects to `PC:24850`, either to the configured address or one found by a UDP broadcast `MIRRORMAGIC?1` that the PC answers.
2. Noise handshake with the pairing-key PSK, then `Hello{version, name, channel}` both ways. A second connection with `channel = Camera` carries only JPEG frames, so video never delays input.
3. The Mac sends `PeerInfo` (displays, Accessibility state). Ping/Pong every 2 s, and a 7 s silence drops the session. On a drop the PC takes its input back.
4. Crossing: the PC parks its hidden cursor at the screen centre, and every hook move becomes a delta (`Motion`). The Mac applies the delta to its real cursor position and clamps it to the active display. Pushing past the PC side sends `Leave{along}`.
5. Monitor mode: the PC sends DDC `0x60 = mac_input`, then `Enter{mode: Monitor}`. Switching back sends `Ddc{pc_input}` to the Mac (its input is the active one then) and also tries DDC from the PC.
6. Clipboard: the side giving up control sends `Clipboard{text, html, png}` unless its fingerprint equals the last one that crossed. No polling, so clipboard managers see exactly one change per hand-over.
7. Camera: the Mac polls CoreMediaIO every second. While "OBS Virtual Camera" is running somewhere (plus 10 s), it sends `CameraWanted{true}`; the PC opens the webcam as MJPEG and streams frames.

## Hook rules (Windows)

- Hook callbacks must return in milliseconds or Windows silently unhooks them. The callbacks only lock the state machine and queue messages. UI updates, cursor hiding, DDC and clipboard work run on other threads.
- Injected events (`LLKHF_INJECTED`/`LLMHF_INJECTED`) are ignored, so remote-control tools on the PC never cross to the Mac.
- No crossing while a mouse button is held, so drag-and-drop stays on the PC.
- The hidden cursor uses `SetSystemCursor`. Start-up and Quit call `SPI_SETCURSORS`, so a crash can't leave the cursor invisible.
