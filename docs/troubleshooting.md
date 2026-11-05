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
| Invisible cursor on the PC after a crash | Start MirrorMagic again (it restores the cursor scheme), or sign out and in. |
