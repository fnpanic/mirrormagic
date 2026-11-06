# Troubleshooting

| Symptom | Check |
|---|---|
| Mac says "no PC found" | Is MirrorMagic running on the PC (tray)? Enter the PC's IP on the Mac. The firewall rule "MirrorMagic" must exist (the installer creates it, local subnet only). |
| "the PC refused the pairing key" | Copy the key again from the PC window. Generating a new key on the PC disconnects old Macs. |
| Pointer doesn't cross | Is the edge toggle on? Is the Mac connected (green dot)? Release all mouse buttons, because nothing crosses while dragging. The edge must be an outer edge of the PC desktop. |
| Mac doesn't react to keys/mouse | Accessibility isn't granted (banner in the Mac app). |
| Stuck "on the Mac" | Press **Scroll Lock**, or "Take control back" in the PC window. A dropped connection also returns control within 7 s. |
| Monitor doesn't switch back | Many monitors accept DDC only on the active input; MirrorMagic asks the Mac too. If neither works, use the monitor's input button. The PC window shows the last DDC result. |
| Wrong characters on the Mac | Mapping is positional; the Mac's layout decides. Try "German AltGr keys" On/Off. |
| Clipboard didn't come along | The clipboard is handed over when you switch, not continuously. Copy, then cross. Is "Share the clipboard" on (both sides)? |
| Camera stays black | OBS installed and set up? Is "OBS Virtual Camera" selected in the app? Is another Windows app using the webcam? The PC window shows the camera status. |
| The monitor shows the Mac when you come back (PC screen asleep, you woke the Mac) | 0.5.11: the Mac keeps its HDMI output off while connected to the PC and turns it on only for monitor mode (Mac log: `HDMI output off`). If it still happens, check that line; `HDMI output off failed: …` means macOS refused, and the 0.5.2/0.5.6/0.5.7 fallbacks below still apply. Quitting MirrorMagic on the Mac turns the output back on. If MirrorMagic is not running and the monitor gets no Mac signal, start MirrorMagic (it turns on displays left off) or re-plug the HDMI cable. |
| Monitor mode: the monitor goes black after some minutes | The Mac's display slept: injected input doesn't count as activity. Fixed in 0.5.15 (MirrorMagic declares user activity while the PC controls the Mac). |
| A "– PC" microphone is silent in Teams | Mac log: `audio: Mac apps use PC microphone "…"` must appear when the call starts, then `audio: connected` and `… chunks of the PC microphone, peak level …`. Peak 0: the PC microphone is muted or not the one picked. No line at all: the use was not noticed (fixed in 0.5.15). |
| Ctrl+Alt+M: Mac log `mirror true failed: no external display connected` | The Mac's HDMI output was off and MirrorMagic did not know the display (0.5.11/0.5.12 after a restart). Fixed in 0.5.13; restarting MirrorMagic on the Mac turns the display on again. |
| The monitor shows the Mac after Windows turned the screen off (away/idle) | The monitor's automatic input detection switched to the Mac's HDMI signal while the PC sent none. Since 0.5.2 the PC switches the input back as soon as Windows turns the screen on again (mouse or key), unless monitor mode is on. MirrorMagic.log on the PC shows `display off` and `display on: monitor input …, switching back`. Without MirrorMagic: turn off "Auto input detect" in the monitor's menu. |
| Monitor mode: the Mac cursor sticks to the top (or another) edge | Displays changed after the switch (mirroring settles asynchronously, HDMI re-plugged, resolution changed). Since 0.5.1 the Mac re-reads the layout every 500 ms while you control it, and the cursor frees itself within half a second. MirrorMagic.log shows `enter … in Displays{…}` and `displays changed while in control`. |
| Pointer doesn't cross / hotkey does nothing while a game or full-screen video is open | Game mode: MirrorMagic stays out of the input of full-screen apps. Leave the app (Alt+Tab) and it works again within half a second. |
| No cursor on the Mac after working on the PC | Fixed in 0.5.5: the Mac's cursor waits 48 points inside the shared edge (and in the middle of the laptop screen after monitor mode), visible for the Mac's own trackpad. |
| The Mac cursor is pulled to one side on its own | Fixed in 0.5.6 (PC): moves are measured from the hidden PC cursor's real position. |
| The monitor shows the Mac's second screen while you work on the MacBook | 0.5.7: the Mac reports "in use" every 30 s and the PC keeps its screen on (PC log: `Mac in use: keeping the PC screen on`). |
| The monitor shows the Mac's second screen while you are away | 0.5.6: the Mac puts its displays to sleep once the PC's screen is off and the Mac has been idle for 60 s (Mac log: `PC screen off, Mac idle … displays to sleep`). Not while a program keeps the display awake (`pmset -g assertions`). |
| Invisible cursor on the PC after a crash | Start MirrorMagic again (it restores the cursor scheme), or sign out and in. |
