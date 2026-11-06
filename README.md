# MirrorMagic

Share the Windows PC's keyboard, mouse, monitor, clipboard and webcam with a Mac, like a software KVM switch with a few extra tricks.

| You do | What happens |
|---|---|
| Push the pointer past the configured screen edge | Mouse and keyboard control the Mac (its built-in screen sits next to the monitor). Push it back to return. |
| Press **Ctrl+Alt+M** | The shared monitor switches to the Mac's HDMI input (DDC/CI) and shows the Mac's **main screen** at the monitor's full native resolution (the MacBook screen mirrors the monitor while monitor mode is on). Mouse and keyboard move to the Mac. Press again to switch back. |
| Switch either way | The clipboard comes with you: text, formatted text (HTML) and images. Clipboard managers on both sides record it. |
| Open "OBS Virtual Camera" in Teams/Zoom/a browser on the Mac | The PC's webcam turns on and streams to the Mac, and its microphone to "BlackHole 2ch" (pick that as the microphone). Both run only while an app uses the camera and turn off within a minute after the call (it follows the macOS camera-control log, so calls are not interrupted). |
| Press **Scroll Lock** | Emergency exit: control returns to the PC at once. |

Windows is the primary side (it owns keyboard, mouse and webcam). The Mac connects out to the PC over the LAN, which works on a locked-down corporate Mac that doesn't accept inbound connections.

## Install

**PC**: run `dist/windows/MirrorMagic_<version>_x64-setup.exe`. The installer adds a firewall rule that only admits the local subnet. MirrorMagic lives in the tray; open it to see the **pairing key**.

**Mac**: unzip `dist/macos/MirrorMagic_macos_arm64.zip` into `/Applications`, then open it.
1. Paste the pairing key and press Save. The PC address is found automatically, or you can enter it (e.g. `192.168.178.30`).
2. Allow **Accessibility** when macOS asks (Privacy & Security). Without it the Mac can't be controlled.
3. Allow **Local Network** when macOS asks.
4. For the camera: install [OBS Studio](https://obsproject.com) and allow its camera extension when macOS asks; MirrorMagic configures OBS by itself. Then pick "OBS Virtual Camera" in your apps.
5. For the microphone: install [BlackHole 2ch](https://existential.audio/blackhole/) once and pick "BlackHole 2ch" as the microphone in your apps.

## Settings (PC)

- **Screen edge**: click where the Mac sits relative to the monitor. The default is left.
- **Monitor mode**: the hotkey, and which input the Mac uses (HDMI 1/2…). The PC's own input is learned automatically.
- **Camera**: resolution (the camera's native MJPEG, no re-encoding).
- **General**: clipboard sharing, start with Windows.

## How it works

```
PC (Windows, primary)                                  Mac (secondary)
low-level hooks ──► state machine (mm-core) ─┐   ┌──► CGEvent injection
DDC/CI 0x60 ──► monitor input                │   │    DDC/CI over HDMI (switch back)
webcam MJPEG (Media Foundation) ─────────────┼───┼──► 127.0.0.1 MJPEG ──► OBS ──► OBS Virtual Camera
clipboard (text/HTML/PNG) ◄──────────────────┼───┼──► clipboard
                      TCP 24850, Noise NNpsk0 (pairing key), 2 connections: control + camera
```

- **Security**: every connection is encrypted and mutually authenticated with Noise `NNpsk0_25519_ChaChaPoly_BLAKE2s`, keyed with the 128-bit pairing key. A wrong key fails the handshake. LAN discovery (UDP broadcast) reveals only the PC's name. The camera stream reaches OBS only through 127.0.0.1 on the Mac.
- **No stuck keys**: each key and button is owned by the side that saw its press. Its release goes to the same side, and every switch releases everything on the Mac.
- **Monitor switch-back**: many monitors ignore DDC on an inactive input. When you switch back, both the PC and the Mac (whose input is active then) send the command.
- **Keyboard**: keys are mapped by position, like a hardware KVM; the Mac's layout decides the character. With a German layout, AltGr combos are translated (`AltGr+Q` → `@`, `AltGr+8/9` → `[ ]`, `AltGr+7/0` → `{ }`, `AltGr+ß` → `\`, `AltGr+<` → `|`, `AltGr++` → `~`). With "Ctrl works as ⌘" (on by default) Ctrl+C/V copy and paste on the Mac like on Windows, and the Win key acts as Control; with it off, the Win key is Command.

More: [docs/architecture.md](docs/architecture.md), [docs/macos.md](docs/macos.md), [docs/troubleshooting.md](docs/troubleshooting.md).

## Build and test

```bash
cargo test --workspace            # protocol, crypto, state machines, keymap, loopback session
cd app && npm ci && npm test      # UI helpers
cd app && npx tauri build --bundles nsis    # Windows installer → target/release/bundle/nsis
```

The Mac build runs on the Mac (`scripts/mac/build-on-mac.sh`; npm is blocked there, so the frontend is prebuilt on Windows). See [docs/macos.md](docs/macos.md).

## Credits

Ideas and research came from [lan-mouse](https://github.com/feschber/lan-mouse), [Deskflow](https://github.com/deskflow/deskflow), [ddc-hi](https://crates.io/crates/ddc-hi)/[m1ddc](https://github.com/waydabber/m1ddc), and the key-code tables of [rustdesk-org/rdev](https://github.com/rustdesk-org/rdev) (MIT). The camera path uses [OBS Studio](https://obsproject.com)'s signed virtual camera, because macOS camera extensions require an Apple Developer ID.
