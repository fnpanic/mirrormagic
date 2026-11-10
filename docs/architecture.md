# Architecture

## Crates and modules

| Path | What |
|---|---|
| `crates/mm-core/src/proto.rs` | Wire messages (postcard). The PC answers with the lower of both `PROTOCOL_VERSION`s; peers older than `MIN_COMPATIBLE_VERSION` are refused. |
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
4. Crossing: the PC parks its hidden cursor at the screen centre, and every hook move becomes a delta (`Motion`). The Mac applies the delta to its real cursor position and clamps it to the active display. Pushing past the PC side sends `Leave{along}`, and the Mac parks its cursor 48 points inside that edge, so it stays visible for the Mac's own trackpad.
5. Monitor mode: the PC sends DDC `0x60 = mac_input`, then `Enter{mode: Monitor}`. Switching back sends `Ddc{pc_input}` to the Mac (its input is the active one then) and also tries DDC from the PC. When Windows turns the display back on after power saving (`win/power.rs`, `GUID_CONSOLE_DISPLAY_STATE` off → on) and monitor mode is off, the PC reads the input and runs the same switch-back if the monitor's auto input detection moved to the Mac meanwhile. The Mac keeps its HDMI output off outside monitor mode (`sys::set_display_enabled`, private `CGSConfigureDisplayEnabled`; despite kCGConfigureForAppOnly the display stays off after the app ends, so its ID is kept in `config.hdmi_off`, turned on in `shutdown()` and again by `hdmi_restore()` at start): `Enter{Monitor}` turns it on before mirroring, and the 2 s pinger turns it off again 5 s after monitor mode ends (`MONITOR_ON`, not the mirror state: a failed mirror must not blank the monitor). A mirror that fails right after the output comes back (macOS still attaching the display, error 1001) is retried for 4 s, each time after an unmirror step. If the monitor fell back to the PC before the HDMI signal arrived, the PC re-sends the switch after 4 s (`recheck_mac_input`).
6. Clipboard: the side giving up control sends `Clipboard{text, html, png}` unless its fingerprint equals the last one that crossed. No polling, so clipboard managers see exactly one change per hand-over.
7. Pause (`config.paused`, `commands::set_paused`, tray and window): the PC releases control, then both control loops end the session on the next message (pings every 2 s), the PC refuses connections after the hello and `hooks_wanted` keeps the hooks off; the Mac's `client_loop` does not dial while paused. A Mac pause in monitor mode first ends monitor mode itself: it unmirrors, waits 1 s, then switches the monitor to `config.pc_input` (learned from the PC's `Ddc` switch-backs). While paused, `client_loop` keeps HDMI off. A PC pause in monitor mode toggles the monitor back first. When a session ends in monitor mode, the PC reads the input over DDC after 3 s and clears `monitor_on_mac` if the monitor shows the PC (`recheck_monitor_after_drop`).
8. Files: a copied file list becomes `FilesOffer` on the control channel. The Mac opens a `Files` channel (only the Mac can dial) and either pulls the PC's offer or, after `FilesPushRequest`, pushes its own. Items stream as `FileEntry` plus 512 KB `FileData` chunks, are written below `<temp>/MirrorMagic/files/<id>` with sanitised paths, and are put on the clipboard as a file list (`crates/mm-core/src/files.rs`).
9. Camera (`crates/mm-core/src/camdemand.rs`): OBS's virtual camera stays off while idle, because macOS counts OBS feeding it as "in use". The Mac polls CoreMediaIO every second; "OBS Virtual Camera" running then means a real app opened it, so it sends `CameraWanted{true}` (the PC opens the webcam as MJPEG and the microphone) and starts OBS's virtual camera. `mac/camlog.rs` follows the unified log for `DeviceControlPID` events. When an app releases a camera, OBS is paused for about half a second to check whether anyone still watches, and if not everything stops (safety check every 10 min, every 45 s without the log).
10. Audio (`app/src-tauri/src/mac/vaudio.rs`, `audio.rs`): no driver (BlackHole needs admin rights). The Mac creates Core Audio taps (macOS 14.2+) and public aggregate devices. The PC sends its device names (`AudioDevices`, protocol 7), and the Mac creates one aggregate per PC device ("<name> – PC"). **PC input** = a muted tap on MirrorMagic's own process: MirrorMagic plays the PC mic to the default output, the tap keeps it off the speakers and feeds it to apps recording from the device. **PC output** = an aggregate around the real output plus a muted tap on its stream 0 (all processes but MirrorMagic). MirrorMagic reads that tap through a private aggregate ("MirrorMagic speaker tap") and sends it to the PC. `audio_watch` polls each device's `DeviceIsRunningSomewhere` every second to see which one an app uses. MirrorMagic runs no I/O on the public devices; the per-process device list (`pdv#`) does not name aggregates. While one is used, `audio_stream` holds a `Channel::Audio` connection (protocol 6): `AudioWant{mic, speaker}` (device names) up, PC mic `Audio` down, speaker `Audio` up. `secure::Receiver::recv` is not cancel-safe, so both ends read the channel on a separate task (`audio::reader`).

## Hook rules (Windows)

- Game mode: every 500 ms the hook thread checks whether the foreground window covers its whole monitor without being maximized (`win::fullscreen_app_in_front`). If so, and the PC has control, both hooks are removed until that changes (`pc::hooks_wanted`), so games see no hook latency and the pointer can't cross.
- Hook callbacks must return in milliseconds or Windows silently unhooks them. The callbacks only lock the state machine and queue messages. UI updates, cursor hiding, DDC and clipboard work run on other threads.
- Injected events (`LLKHF_INJECTED`/`LLMHF_INJECTED`) are ignored, so remote-control tools on the PC never cross to the Mac.
- No crossing while a mouse button is held, so drag-and-drop stays on the PC.
- The hidden cursor uses `SetSystemCursor`. Start-up and Quit call `SPI_SETCURSORS`, so a cursor left invisible by a crash is healed by the next start.

## What ends a connection (PC)

- `pc::must_end`: every channel loop (control, camera, audio) asks it. It answers "paused" or "pairing key changed" (`KEY_GEN`, bumped by `commands::new_pairing_key`). A file transfer in flight finishes.
- At most 32 handshakes run at a time (`HANDSHAKES`); the permit is returned after the hello.
- The Mac's wishes are kept (`MIC_WANT`, `CAM_WANT`) apart from what the PC allows (`config.mic`, `config.camera`): switching either in the window stops or starts the stream at once (`pc::sharing_changed`).
- A control session that replaces a live one (the Mac's socket died one-sidedly and it reconnected within the 7 s timeout) gets `Primary::connected()` on its first `PeerInfo`, which sends `Enter` again.

## Settings

`commands::save_config` merges the patch under the config lock and refuses the fields the backend keeps for itself (`mirrored`, `hdmi_off`, `paused`, `obs_password`, `pc_input`, and on the PC `pairing_key`). A settings file that does not parse is kept as `config.json.bad`, and the window says so. On the Mac, saved settings apply at once (`mac::refresh`).
