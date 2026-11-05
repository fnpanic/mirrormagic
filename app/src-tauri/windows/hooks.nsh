; Let the Mac reach MirrorMagic from the local subnet only (any profile: home Wi-Fi is often "Public").
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
  nsExec::Exec 'netsh advfirewall firewall add rule name="MirrorMagic" dir=in action=allow program="$INSTDIR\mirrormagic.exe" enable=yes profile=any remoteip=localsubnet'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
!macroend
