; Datenportal Connector: installer hooks for the ms-word: and ms-excel: links.
;
; The installer writes HKCU\Software\Classes\ms-word and \ms-excel itself (from the deep-link
; schemes in tauri.conf.json), overwriting what was there. Before that, the pre-install hook
; saves what it finds, once, for "datenportal-connector.exe --uninstall" to put back. The
; rest is done by the app: --setup after installing (registration for Settings > Default
; apps, lodav.py's leftovers), --uninstall before removing (links and LibreOffice's settings
; back as they were). Neither runs for updates.

!define DC_HANDOVER "Software\Datenportal Connector\Handover"

!macro DC_SAVE_HANDLER SCHEME
  ClearErrors
  ReadRegStr $R0 HKCU "${DC_HANDOVER}\${SCHEME}" "Saved"
  ${If} ${Errors}
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "Saved" "1"
    ReadRegStr $R0 HKCU "Software\Classes\${SCHEME}" ""
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "Name" "$R0"
    ReadRegStr $R0 HKCU "Software\Classes\${SCHEME}\shell\open\command" ""
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "Command" "$R0"
    ; lodav.py marked the keys it took over and kept what it found there
    ReadRegStr $R0 HKCU "Software\Classes\${SCHEME}" "lodav"
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "LodavMarker" "$R0"
    ReadRegStr $R0 HKCU "Software\Classes\${SCHEME}" "lodavPreviousName"
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "LodavPreviousName" "$R0"
    ReadRegStr $R0 HKCU "Software\Classes\${SCHEME}" "lodavPreviousCommand"
    WriteRegStr HKCU "${DC_HANDOVER}\${SCHEME}" "LodavPreviousCommand" "$R0"
    ClearErrors
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro DC_SAVE_HANDLER "ms-word"
  !insertmacro DC_SAVE_HANDLER "ms-excel"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --setup'
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    dc_uninstall:
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstall' $R0
    ; 3: LibreOffice runs, so its settings can't be put back yet
    ${If} $R0 = 3
      ${If} $LANGUAGE = 1031
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "LibreOffice läuft noch. Schließen Sie es, damit seine Einstellungen wiederhergestellt werden können, und klicken Sie dann auf Wiederholen." IDRETRY dc_uninstall
      ${Else}
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "LibreOffice is still running. Close it so that its settings can be put back, then click Retry." IDRETRY dc_uninstall
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
!macroend
