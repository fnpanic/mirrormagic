# Changelog

## 0.5.20 — 2026-11-08 (PC and Mac)

- **Files copied on the Mac now paste on Windows.** The transfer itself worked: on 2026-11-08 at 22:03 and 22:04 the Mac pushed `logile_tether.txt` and `tether_input.txt`, and both arrived complete in `%TEMP%\MirrorMagiciles\<id>`. But arboard's Windows `file_list` writes CF_HDROP without emptying the clipboard first, so the text copied earlier stayed next to the files. Any app that prefers text (a chat box, an editor, the Explorer address bar) pasted that old text instead of the file. The PC now clears the clipboard before writing the file list. Ignored test `files_replace_text` (touches the real clipboard): passed. Before the fix, a probe left `UnicodeText` next to `FileDrop`; an Explorer paste of the file (Shell `Paste` verb) worked. On the Mac, arboard already clears the pasteboard. Pictures: PNG hand-overs worked both ways all along (both logs, `written`), and a copied picture file travels as a file.
- **Pause and Quit.** The tray / menu-bar menu has **Pause MirrorMagic** (a checkmark) and **Quit MirrorMagic**, and the window's General card has a Pause switch and a Quit button, on both machines. Paused (`config.paused`, kept across restarts):
  - The PC first takes keyboard, mouse and monitor back, then drops the session within 2 s and refuses new connections. Its input hooks are removed, so there is no crossing and no hotkey.
  - The Mac stops dialing the PC and turns its HDMI output on, as it would be without MirrorMagic.
  - Window and tray show "Paused". Unchecking resumes.
- Verified on the real machines (0.5.20): a screenshot copied on the Mac arrived at 22:24:47 (`received +image`, `written`), and the file `cgm-guardrail-v2-2d786ba.zip` copied in Finder was ready at 22:25:15. The user confirmed that both pasted on Windows. Pause, tested by the user at 22:27: PC `paused` 22:27:04, `resumed` and the Mac back 22:27:09; Mac `paused` with `HDMI output on (paused): Ok` at 22:27:16 and `resumed` at 22:27:26 (Mac clock), reconnected the same second. The user confirmed the pointer stayed on the PC while paused and crossing worked again after resuming.

## 0.5.19 — 2026-11-08 (Mac)

- **Ctrl+Alt+M: no more black monitor.** On 2026-11-08 at 11:49:50 the Mac turned its HDMI output on for monitor mode, but mirroring failed at once (`mirror true failed: mirror: 1001`). The freshly enabled monitor was still attaching and was missing from the active display list, so macOS refused the layout. The 2 s HDMI check treated "not mirrored" as "not in monitor mode" and switched the output off again at 11:49:52, so the monitor, already on HDMI, stayed black. Now:
  - The HDMI output stays on for the whole monitor mode (from `Enter Monitor` until monitor mode ends), whether mirroring worked or not.
  - If mirroring fails, MirrorMagic logs every display's state (`displays (id, built-in, mirrors, active)`). It then retries for up to 4 s, each time undoing any mirror set in its own step first. When the roles swap, the monitor stops mirroring the laptop screen in the same step.
  - The error says which step failed (`configure error …` or `complete error …`).
- In the working runs of 2026-11-06, mirroring was called in the same second the output came back and succeeded. That makes the failure a race with macOS attaching the display.
- Verified on the real machines (0.5.19, the user pressing Ctrl+Alt+M):
  - Switch to the Mac: the PC registered the hotkey at 12:22:10. On the Mac, HDMI came on at 12:22:13 and mirroring succeeded on the first try. `enter Monitor` at 12:22:15 found the monitor at 2560×1440.
  - The user worked on the Mac for 4 minutes (the PC logged `Mac in use`). The Mac's HDMI output stayed on the whole time, and the user confirmed the monitor showed the Mac screen.
  - Switch back: hotkey at 12:26:19, `mirror false` at 12:26:22, and HDMI off at 12:26:29, 7 s later, after the DDC switch back. On the way back, the Mac handed 165 characters of copied text to the PC.
  - The retry path didn't run, because the attach race didn't occur this time. If it does, its log line shows the display states.

## 0.5.18 — 2026-11-06 (Mac)

- **Camera off right after the call.** The 0.5.17 test call showed the end-of-call signal. OBS's camera extension logs `Stream Source … removing streaming client` when an app stops reading the virtual camera (20:06:54, the moment the call ended). MirrorMagic now checks immediately on that line and turns the PC camera off. In 0.5.17 the camera stayed on: the redacted control events made MirrorMagic believe the log worked, which stretched the safety probe to 10 minutes.
- **PC microphone in Teams:** in the 0.5.17 test the speaker side worked (the user heard the call on the PC), and the PC microphone reached the Mac at a good level (peak 21303/32768). But Teams showed no level on "Mikrofon (Jabra Link 390) – PC". At start, MirrorMagic now plays a 1 s tone into the tap and reads it back from that device (`audio self-test: … peak …`). If the read-back is silent, it rebuilds the "– PC" microphones with the real output as clock device (as Apple's tap sample does) and tests again.
- Self-test result on the user's Mac: `read back peak 8414/32768` for a tone at 8000. The "– PC" microphone works, so Teams' silence comes from macOS: tap audio reaches only apps allowed to record system audio. Teams must be added under System Audio Recording (see docs/macos.md).
- Verified by the user after adding Teams to System Audio Recording: a Teams test call 20:16–20:17 used "Mikrofon (Jabra Link 390) – PC" and "Lautsprecher (Jabra Link 390) – PC". The PC microphone reached Teams (peak levels 2865–8610), the call played on the Jabra, and the user said it "seems to work". The call ended at 20:17:23, and the PC camera was off at 20:17:24 (`an app stopped reading the OBS camera` → probe → off), 1 s later instead of up to 45 s or 10 min.

## 0.5.17 — 2026-11-06 (Mac)

- **PC microphone/speakers stream now.** In the 0.5.16 test call, MirrorMagic noticed the use correctly ("Mikrofon (Jabra Link 390)", "Lautsprecher (Jabra Link 390)"), but the audio connection closed again every second. tokio's `watch::Sender::send` drops the value while no receiver exists, so each new connection saw "nothing in use" and ended. The fix uses `send_replace`.
- **Camera after a call:** the macOS camera-control events do arrive (`[Microsoft Teams ModuleHost] … setting deviceControlPID <private>`), but macOS hides the PID from non-admin users, so 0.5.13–0.5.16 discarded them. A control change during a call now triggers an immediate check instead of waiting for the 45 s probe. At the end of the 0.5.16 test call no such event arrived (the camera went off at the 45 s probe, about 25 s after the call). This version therefore logs every distinct kind of CoreMediaIO message that mentions a PID, to find the end-of-call signal.
- 0.5.16: the camera-control log follower records `log stream` errors and the raw lines it can't parse.

## 0.5.15 — 2026-11-06 (Mac)

- **Monitor mode: the screen no longer goes black after a while.** Input that MirrorMagic posts on the Mac (CGEvents) doesn't count as user activity for the macOS display-sleep timer. While the user worked through the PC keyboard and mouse, the Mac put its display to sleep after its timeout (monitor mode 19:25–19:41, black in between). Now, while the PC controls the Mac, MirrorMagic reports user activity to macOS at most every 5 s (`IOPMAssertionDeclareUserActivity`), as real input does. Not yet verified in a long monitor-mode session (the user will test it later).
- **PC microphone in Teams: use is now noticed.** 0.5.12–0.5.14 asked each process for its device list to see whether an app used a "– PC" device. That list doesn't name aggregate devices, so the Teams test call never started the PC microphone (no `audio: Mac apps use …` in the Mac log). Now the device's own "running somewhere" flag is used; MirrorMagic itself never runs I/O on these devices. The Mac log reports the PC microphone's level every 5 s while it streams (`audio: … chunks of the PC microphone, peak level …`).

## 0.5.14 — 2026-11-06 (protocol 7)

- **Pick each PC audio device in Teams.** Instead of one "PC Microphone" and one "PC Speakers", the Mac now offers every PC input and output under its own name. Examples: "Mikrofon (Jabra Link 390) – PC", "Mikrofon (HD Pro Webcam C920) – PC", "Lautsprecher (Jabra Link 390) – PC". Teams adds "(Aggregate)", the macOS type of these devices. The PC records from exactly the microphone you pick and plays on exactly the speaker you pick. The PC sends its device list when the Mac connects; after plugging in a new device on the PC, reconnect (restart MirrorMagic on either side).
- Protocol 7: `AudioWant` carries device names, and `AudioDevices` (PC → Mac) is new. Version 6 was a draft that lived for one afternoon, so the PC refuses the audio channel below 7.

## 0.5.13 — 2026-11-06 (Mac)

- **Fix for 0.5.11: monitor mode failed after a MirrorMagic restart.** macOS keeps a display that MirrorMagic switched off off, even after MirrorMagic quits. After the update to 0.5.12, the new instance saw no external display and didn't know its ID. Ctrl+Alt+M then failed with `mirror true failed: no external display connected` (Mac log 18:56:02). Now:
  - MirrorMagic records the switched-off display in its config (`hdmi_off`).
  - It turns the display back on when it quits.
  - At start it turns on a display left off by an earlier run (after an update or crash). Without a record, it tries every display ID that isn't online. The 2 s check then switches it off again while the PC is connected. One display ID per reconfiguration: an unknown ID makes the whole reconfiguration fail (`complete error 1001`, seen on the first 0.5.13 build).
- **Fix for 0.5.12: "PC Microphone"/"PC Speakers" after a restart.** Public aggregate devices outlive the app; coreaudiod keeps them, with a dead tap inside. The second start found their IDs taken (`error 'nope'`): "PC Speakers" was missing, and a second, dead "PC Microphone" existed. MirrorMagic now removes the devices when it quits, and at start removes any left by an earlier run before creating them again under the same ID (Teams keeps the selection).
- Verified on the real machines (0.5.13), the user's own case: at 19:14:17 the PC screen went off, and at 19:14:38 the Mac put its displays to sleep (idle 62 s), so both were in standby. At 19:15:08 the Mac was woken (Tether), and the PC screen stayed off until 19:15:28. The Mac log shows no `HDMI output on` and no new `off`: the output stayed off through display sleep and wake. At 19:15:10 the PC read the monitor input over DDC: `display on: the monitor shows the PC`. Before the fix, at 18:10, it was `monitor input Some(17)` (HDMI).
- Verified on the real machines (0.5.13): at start, the display left off by 0.5.12 was found and turned on (`turned on [(2, Ok)]`), and the audio devices left behind were removed and recreated. The user pressed Ctrl+Alt+M at 19:09:05: `HDMI output on (monitor mode): Ok, online: true`, mirrored, `enter Monitor` at 2560×1440, no re-switch needed on the PC. Ctrl+Alt+M at 19:11:26: `mirror false`, and at 19:11:32 `HDMI output off (display 2)`. The user confirmed the switch works.
- Verified on the real machines (0.5.11/0.5.12): with the Mac awake, the PC screen was turned off at 18:42 and 18:47. The Mac logged `PC screen off`, but its HDMI output stayed off (no `HDMI output on`, no re-plug), so the monitor had no Mac signal to switch to. Before, at 18:10, the PC found the monitor on HDMI (`monitor input Some(17)`).

## 0.5.12 — 2026-11-06 (protocol 6)

- **PC microphone and PC speakers in Teams on the Mac, without a driver.** The Mac now has two audio devices, **PC Microphone** and **PC Speakers**; pick them in Teams (Settings → Devices), Zoom or the browser. Before, the microphone needed BlackHole 2ch, which the corporate Mac can't install without admin rights (Mac log: `BlackHole is not installed`), and Mac sound could not reach the PC at all.
- How it works (macOS 14.2+ Core Audio taps): "PC Microphone" is an aggregate device whose input is a muted tap on MirrorMagic itself. MirrorMagic plays the PC microphone there, and only apps recording from "PC Microphone" hear it. "PC Speakers" wraps the Mac's speakers with a muted tap on its output: what Teams plays there is silent on the Mac and plays on the PC's default output instead.
- Sound runs on its own connection (protocol 6), only while an app on the Mac uses one of the two devices, in 10 ms chunks of mono 16-bit. The microphone no longer streams with the camera; audio-only calls work too.
- The first use makes macOS ask once for **System Audio Recording** permission for MirrorMagic: allow it.

## 0.5.11 — 2026-11-06

- **The Mac's HDMI output is off except in monitor mode.** While connected to the PC, MirrorMagic turns the Mac's external display fully off, as if the cable were unplugged. Before, when the PC's screen was asleep and you woke the Mac, the monitor's automatic input detection picked up the Mac's HDMI signal and showed the Mac. The PC switched back only after it woke up itself (PC log of 18:09–18:10: `display off`, then `display on: monitor input Some(17)`). Now the monitor gets no Mac signal at all. **Ctrl+Alt+M** turns the output on, mirrors and switches as before. After switching back, the output goes off again about 5 s later, once the DDC switch over HDMI is done. The Mac re-checks every 2 s, because waking the monitor or re-plugging the cable brings the display back. Mac log: `HDMI output off (display …)` / `HDMI output on (monitor mode)`.
- Safeguards: the output stays on when the MacBook lid is closed (it is the only screen then), and it comes back on after 5 minutes without a connection to the PC (PC shut down). Correction (0.5.13): macOS does *not* undo the change when MirrorMagic ends, despite the "for this app only" option.
- PC: if the monitor finds no Mac signal yet after the switch and falls back to the PC, the PC switches it to the Mac once more after 4 s (PC log: `monitor went back to the PC before the Mac's HDMI signal was there`).
- This uses the private macOS call `CGSConfigureDisplayEnabled` (SkyLight, as BetterDisplay does): turning a display off needs no root that way.

## 0.5.10 — 2026-11-06 (Mac)

- After control goes back to the PC, the Mac sends a real 1-point move there and back. Apps such as Terminal hide the pointer while you type until the mouse moves, and that state could keep the Mac's pointer hidden after you switched back.
- Verified: after the handover the Mac log showed `pointer (Mac in charge): visible on the built-in display`, and the user confirmed that the Mac's own trackpad works without crossing over from Windows first.
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
