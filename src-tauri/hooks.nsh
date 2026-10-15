; Deck's uninstaller: before files go, take out what Deck put elsewhere (Claude Code hooks from
; Claude Sync and their hook program). Not on an update: the new Deck keeps them.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\deck.exe" --uninstall-cleanup'
  ${EndIf}
!macroend
