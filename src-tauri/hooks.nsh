; Deck usually runs elevated (its logon task), while this setup / uninstaller runs as the normal
; user, so the template's own "kill Deck" step is refused ("Failed to kill Deck"). Before it
; runs, ask Deck to quit through its elevated \Deck\Quit task (Deck.exe --quit): a normal user
; may start that task, and a clean quit hands the fans back to the BIOS.
!macro DeckQuitIfRunning
  nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq deck.exe" /NH | find /I "deck.exe"'
  Pop $0
  Pop $1
  ${If} $0 = 0
    nsExec::Exec 'schtasks.exe /run /tn "\Deck\Quit"'
    Pop $0
    StrCpy $2 0
    ${DoWhile} $2 < 16
      Sleep 500
      IntOp $2 $2 + 1
      nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq deck.exe" /NH | find /I "deck.exe"'
      Pop $0
      Pop $1
      ${If} $0 <> 0
        ${ExitDo}
      ${EndIf}
    ${Loop}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro DeckQuitIfRunning
!macroend

; Also takes out what Deck put elsewhere (Claude Code hooks from Claude Sync and their hook
; program). Not on an update: the new Deck keeps them.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro DeckQuitIfRunning
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\deck.exe" --uninstall-cleanup'
  ${EndIf}
!macroend
