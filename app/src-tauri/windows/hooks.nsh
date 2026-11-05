; Allow the Mac to reach MirrorMagic on the private (home) network only.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
  nsExec::Exec 'netsh advfirewall firewall add rule name="MirrorMagic" dir=in action=allow program="$INSTDIR\mirrormagic.exe" enable=yes profile=private'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="MirrorMagic"'
!macroend
