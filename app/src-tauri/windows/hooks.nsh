; Let the Mac reach MirrorMagic from the local subnet only (any profile: home Wi-Fi is often "Public"),
; and only on its own port: TCP 24850 for the sessions, UDP 24850 for discovery.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
  nsExec::Exec 'netsh advfirewall firewall add rule name="MirrorMagic" dir=in action=allow program="$INSTDIR\mirrormagic.exe" enable=yes profile=any remoteip=localsubnet protocol=TCP localport=24850'
  nsExec::Exec 'netsh advfirewall firewall add rule name="MirrorMagic" dir=in action=allow program="$INSTDIR\mirrormagic.exe" enable=yes profile=any remoteip=localsubnet protocol=UDP localport=24850'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
!macroend
