# macOS

## Permissions

- **Accessibility** (Privacy & Security → Accessibility): required to move the pointer and type. The app asks on first start; the banner's **Open settings** button jumps there.
- **Local Network**: macOS 15 asks the first time MirrorMagic connects to the PC.
- The app is signed with the self-signed "Tether Code Signing" certificate (the same as Tether). The designated requirement stays the same across builds, so the Accessibility grant survives updates. If macOS still shows a stale grant: `tccutil reset Accessibility app.mirrormagic.kvm`, then allow it again.

## Camera through OBS

macOS only accepts new cameras as signed Camera Extensions, which need a paid Apple Developer ID; legacy DAL plugins stopped working in macOS 14.1. MirrorMagic therefore feeds OBS's signed virtual camera:

1. Install OBS Studio into `/Applications`.
2. Press **Set up OBS** in MirrorMagic. If OBS isn't running, it enables obs-websocket in `~/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json` with a random password (stored in MirrorMagic's config). It then starts OBS hidden, creates scene **MirrorMagic** with the media source **PC camera** (`http://127.0.0.1:24851/camera.mjpg`), fits it to the canvas and starts the virtual camera.
3. Approve OBS's camera extension in System Settings if macOS asks (once).
4. In Teams, Zoom or the browser, choose **OBS Virtual Camera**.

OBS's virtual camera stays **off** while no app uses it, because macOS counts OBS feeding it as "in use" (which would keep the PC webcam on all the time). When an app opens "OBS Virtual Camera", MirrorMagic starts the PC webcam and microphone and turns OBS's virtual camera on; the app shows the OBS placeholder for 1–2 s first. Every 45 s MirrorMagic pauses OBS for about half a second to check whether an app still watches; if not, the webcam turns off. See `crates/mm-core/src/camdemand.rs`.

## Building on the Mac (no CI)

npm is blocked on the corporate Mac, so the frontend is built on Windows (Git Bash, repo root):

```bash
cd app && npm run build && cd ..
git -c core.autocrlf=false archive --format=tar -o /c/vms/mmup/src.tar HEAD
tar -rf /c/vms/mmup/src.tar app/dist && gzip -f /c/vms/mmup/src.tar && mv -f /c/vms/mmup/src.tar.gz /c/vms/mmup/src.tgz
```

Upload these files to the Mac, e.g. with `node scripts/mac/session.mjs put` over a hidden Tether file-transfer session (set `MSYS_NO_PATHCONV=1`, or Git Bash rewrites `/Users/...`):

- `src.tgz` and `scripts/mac/build-on-mac.sh` → `~/mirrormagic-build/`
- `C:\vms\macsign\cs.p12` and `p12.pass` → `~/mirrormagic-build/sign/`

Then run `bash ~/mirrormagic-build/build-on-mac.sh` in a Terminal on the Mac. The script:

1. Imports the certificate into a temporary keychain, then deletes the p12 and its password.
2. Runs `cargo test`.
3. Prints a read-only DDC probe.
4. Builds and signs the app, and zips it to `out/`.
5. Replaces `/Applications/MirrorMagic.app` and starts it.

The log goes to `~/mirrormagic-build/build.log`.
