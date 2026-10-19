; Deck usually runs elevated (its logon task). Before the template's own "kill Deck" step, ask
; Deck to quit (Deck.exe --quit): a clean quit hands the fans back to the BIOS, a kill doesn't.
;   1. through the elevated \Deck\Quit task (a normal user may start a task), no prompt;
;   2. if that task doesn't exist yet (Deck older than 2.5.1 that never started with admin
;      rights since), by running deck.exe --quit as admin: one UAC prompt.

; $0 = 0 when deck.exe is running
!macro DeckRunning
  nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq deck.exe" /NH | find /I "deck.exe"'
  Pop $0
  Pop $1
!macroend

; waits up to 8 s for deck.exe to exit
!macro DeckWait
  StrCpy $2 0
  ${DoWhile} $2 < 16
    Sleep 500
    IntOp $2 $2 + 1
    !insertmacro DeckRunning
    ${If} $0 <> 0
      ${ExitDo}
    ${EndIf}
  ${Loop}
!macroend

!macro DeckQuitIfRunning
  !insertmacro DeckRunning
  ${If} $0 = 0
    nsExec::Exec 'schtasks.exe /run /tn "\Deck\Quit"'
    Pop $0
    ${If} $0 = 0
      !insertmacro DeckWait
    ${EndIf}
    !insertmacro DeckRunning
    ${If} $0 = 0
      ${If} ${FileExists} "$INSTDIR\deck.exe"
        ExecShell "runas" "$INSTDIR\deck.exe" "--quit" SW_HIDE
        !insertmacro DeckWait
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; Setups up to 2.5.5 installed Deck for the current user only (%LOCALAPPDATA%\Deck), a folder
; any program of the user can change, while Deck's tasks start it with admin rights. This setup
; installs to Program Files; the old copy is uninstalled first, quietly. Settings and the Claude
; Code hook program (which shares that folder) stay; Claude Sync puts its hooks back by itself.
!macro DeckRemovePerUserCopy
  ReadEnvStr $R9 LOCALAPPDATA
  ${If} $R9 != ""
  ${AndIf} ${FileExists} "$R9\Deck\uninstall.exe"
  ${AndIf} ${FileExists} "$R9\Deck\deck.exe"
  ${AndIf} "$R9\Deck" != "$INSTDIR"
    ; _?= runs it in place and waits for it
    ExecWait '"$R9\Deck\uninstall.exe" /S _?=$R9\Deck'
    Delete "$R9\Deck\uninstall.exe"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro DeckQuitIfRunning
  !insertmacro DeckRemovePerUserCopy
!macroend

; Also takes out what Deck put elsewhere (Claude Code hooks from Claude Sync and their hook
; program). Not on an update: the new Deck keeps them.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro DeckQuitIfRunning
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\deck.exe" --uninstall-cleanup'
  ${EndIf}
!macroend

; The setup is elevated (per-machine), so it is the place for the one UAC prompt: it makes Deck's
; admin tasks itself (Deck.exe --setup-task), and Deck never shows "Set up" after an install or an
; update. Silent, passive and update installs (the updater, scripts) also start Deck again, hidden,
; through the admin \Deck\Background task. A GUI install's "Run Deck" box starts it as the user;
; Deck hands that start over to the admin task by itself (see elevation::handover).
; --from-setup: when the setup runs as another account than the one signed in (a standard user typed
; an administrator's password), no tasks are made (exit code 2) and Deck is started as the signed-in
; user instead; it then shows Set up.
!macro NSIS_HOOK_POSTINSTALL
  ExecWait '"$INSTDIR\deck.exe" --setup-task --from-setup' $0
  ${If} ${Silent}
  ${OrIf} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    ${If} $0 = 0
      nsExec::Exec 'schtasks.exe /run /tn "\Deck\Background"'
      Pop $0
    ${Else}
      nsis_tauri_utils::RunAsUser "$INSTDIR\deck.exe" "--minimized"
    ${EndIf}
  ${EndIf}
!macroend

; Deck writes SDL3.dll (the controller overlay's controller library) next to deck.exe itself,
; so the template's file list doesn't know it: take it out, then the folder can go too.
; An uninstall (not an update) also takes Deck's admin tasks away: they would point at a missing exe.
; The updater's downloads (updates) go too, but not during an update: the update's setup runs from
; there (the new Deck clears it when it starts).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::Exec 'schtasks.exe /delete /tn "\Deck\Deck" /f'
    Pop $0
    nsExec::Exec 'schtasks.exe /delete /tn "\Deck\Open" /f'
    Pop $0
    nsExec::Exec 'schtasks.exe /delete /tn "\Deck\Quit" /f'
    Pop $0
    nsExec::Exec 'schtasks.exe /delete /tn "\Deck\Background" /f'
    Pop $0
    RMDir /r "$INSTDIR\updates"
  ${EndIf}
  Delete "$INSTDIR\SDL3.dll"
  RMDir "$INSTDIR"
!macroend
