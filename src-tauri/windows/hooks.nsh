; MinerDesk 0.7.22 -- shared, logged maintenance for install AND uninstall.
; NO inline PowerShell -Command strings. Arguments travel in a UTF-16 request
; file so apostrophes, spaces and Unicode paths are data, never executable code.
!include "LogicLib.nsh"
!include "x64.nsh"
!define MD_HOOK_DIR "${__FILEDIR__}"
!define MD_UNINST_KEY "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MinerDesk"
!define MD_SECURITY_KEY "SOFTWARE\MinerDesk\Security"

Var MdDataRoot
Var MdLogFile
Var MdLogMode
Var MdLogMessage
Var MdHandle
Var MdLogOffset
Var MdPowerShell
Var MdInstallerPid
Var MdTargetDir
Var MdRequestFile
Var MdAction
Var MdNextAction
Var MdApproved
Var MdCode
Var MdRawCode
Var MdActionCode
Var MdOutput
Var MdErrorText
Var MdLegacyVersion
Var MdLegacyDir
Var MdRegValue
Var MdRegCode

!macro MdRestoreRegView
  ${If} ${RunningX64}
    !if "${ARCH}" == "x64"
      SetRegView 64
    !else if "${ARCH}" == "arm64"
      SetRegView 64
    !else
      SetRegView 32
    !endif
  ${Else}
    SetRegView 32
  ${EndIf}
!macroend

; Generate the same helper functions in both NSIS execution contexts.
!macro MdMaintenanceFunctions PREFIX MODE
Function ${PREFIX}MdInit
  StrCpy $MdCode 0
  StrCpy $MdLogMode "${MODE}"
  ; perMachine installation: APPDATA with all-users context is ProgramData.
  ; COMMONAPPDATA is NOT an NSIS variable.
  SetShellVarContext all
  StrCpy $MdDataRoot "$APPDATA\MinerDesk"
  ${If} $MdLogFile == ""
    CreateDirectory "$MdDataRoot\logs"
    StrCpy $MdLogFile "$MdDataRoot\logs\${MODE}-maintenance.log"
    ClearErrors
    FileOpen $MdHandle "$MdLogFile" a
    ${If} ${Errors}
      StrCpy $MdLogFile "$TEMP\MinerDesk-${MODE}-maintenance.log"
      ClearErrors
      FileOpen $MdHandle "$MdLogFile" a
      ${If} ${Errors}
        StrCpy $MdCode 9002
        StrCpy $MdLogFile ""
        Return
      ${EndIf}
    ${EndIf}
    ; NSIS append mode preserves the file but initially positions at byte zero.
    ; Seek explicitly, otherwise the next phase overwrites earlier diagnostics.
    ClearErrors
    FileSeek $MdHandle 0 END $MdLogOffset
    ${If} ${Errors}
      FileClose $MdHandle
      StrCpy $MdCode 9002
      Return
    ${EndIf}
    ${If} $MdLogOffset == 0
      FileWriteUTF16LE /BOM $MdHandle "MinerDesk 0.7.22 ${MODE} maintenance session.$\r$\nSetup executable: $EXEPATH$\r$\n"
    ${Else}
      FileWriteUTF16LE $MdHandle "MinerDesk 0.7.22 ${MODE} maintenance session.$\r$\nSetup executable: $EXEPATH$\r$\n"
    ${EndIf}
    FileClose $MdHandle
    ${If} ${Errors}
      StrCpy $MdCode 9002
      Return
    ${EndIf}
  ${EndIf}

  Push $0
  System::Call 'kernel32::GetCurrentProcessId() i .r0'
  StrCpy $MdInstallerPid $0
  Pop $0
  StrCpy $MdPowerShell "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${If} ${RunningX64}
    IfFileExists "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" 0 md_native_done
      StrCpy $MdPowerShell "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
md_native_done:
  IfFileExists "$MdPowerShell" md_powershell_ok 0
    StrCpy $MdCode 9001
    Return
md_powershell_ok:
  InitPluginsDir
  ClearErrors
  File /oname=$PLUGINSDIR\maintenance.ps1 "${MD_HOOK_DIR}\maintenance.ps1"
  File /oname=$PLUGINSDIR\maintenance-common.ps1 "${MD_HOOK_DIR}\maintenance-common.ps1"
  ${If} ${Errors}
    StrCpy $MdCode 9000
    Return
  ${EndIf}
  StrCpy $MdRequestFile "$PLUGINSDIR\MinerDeskMaintenance.ini"
FunctionEnd

Function ${PREFIX}MdLog
  ClearErrors
  FileOpen $MdHandle "$MdLogFile" a
  ${If} ${Errors}
    StrCpy $MdCode 9002
    Return
  ${EndIf}
  FileSeek $MdHandle 0 END $MdLogOffset
  ${If} ${Errors}
    FileClose $MdHandle
    StrCpy $MdCode 9002
    Return
  ${EndIf}
  ${If} $MdLogOffset == 0
    FileWriteUTF16LE /BOM $MdHandle "$MdLogMessage$\r$\n"
  ${Else}
    FileWriteUTF16LE $MdHandle "$MdLogMessage$\r$\n"
  ${EndIf}
  FileClose $MdHandle
  ${If} ${Errors}
    StrCpy $MdCode 9002
  ${EndIf}
FunctionEnd

Function ${PREFIX}MdRun
  Call ${PREFIX}MdInit
  ${If} $MdCode != 0
    Return
  ${EndIf}
  StrCpy $MdOutput ""
  ClearErrors
  FileOpen $MdHandle "$MdRequestFile" w
  ${If} ${Errors}
    StrCpy $MdCode 9000
    Return
  ${EndIf}
  FileWriteUTF16LE /BOM $MdHandle "[Maintenance]$\r$\n"
  FileWriteUTF16LE $MdHandle "Action=$MdAction$\r$\nInstallDir=$MdTargetDir$\r$\n"
  FileWriteUTF16LE $MdHandle "LogPath=$MdLogFile$\r$\nApproved=$MdApproved$\r$\n"
  FileWriteUTF16LE $MdHandle "InstallerProcessId=$MdInstallerPid$\r$\nInstallerPath=$EXEPATH$\r$\n"
  FileWriteUTF16LE $MdHandle "InstalledVersion=$MdLegacyVersion$\r$\n"
  FileClose $MdHandle
  ${If} ${Errors}
    StrCpy $MdCode 9000
    Return
  ${EndIf}
  StrCpy $MdLogMessage "Launching action=$MdAction; target=$MdTargetDir; PowerShell=$MdPowerShell; request=$MdRequestFile"
  Call ${PREFIX}MdLog
  ${If} $MdCode != 0
    Return
  ${EndIf}
  DetailPrint "MinerDesk 0.7.22: $MdAction. Log: $MdLogFile"
  ; One short command, no injected paths inside PowerShell source code.
  nsExec::ExecToStack /TIMEOUT=120000 '"$MdPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\maintenance.ps1" -RequestPath "$MdRequestFile"'
  Pop $MdRawCode
  Pop $MdOutput
  StrCpy $MdActionCode $MdRawCode
  ${If} $MdRawCode == "error"
    StrCpy $MdActionCode 9001
  ${ElseIf} $MdRawCode == "timeout"
    StrCpy $MdActionCode 9003
  ${EndIf}
  StrCpy $MdLogMessage "Completed action=$MdAction; exit=$MdRawCode$\r$\nHelper output: $MdOutput"
  StrCpy $MdCode 0
  Call ${PREFIX}MdLog
  ${If} $MdCode == 0
    StrCpy $MdCode $MdActionCode
  ${EndIf}
FunctionEnd

Function ${PREFIX}MdReportFailure
  StrCpy $MdErrorText "The maintenance helper failed. This does NOT establish that MinerDesk is running."
  ${If} $MdCode == 20
    StrCpy $MdErrorText "Confirmed MinerDesk/miner processes remain. Their PIDs and executable paths are recorded in the log."
  ${ElseIf} $MdCode == 21
    StrCpy $MdErrorText "The backend Scheduled Task could not be safely stopped/removed or targets a different installation."
  ${ElseIf} $MdCode == 30
    StrCpy $MdErrorText "A known legacy MinerDesk binary could not be removed. The log identifies the file."
  ${ElseIf} $MdCode == 31
    StrCpy $MdErrorText "The old uninstall registration could not be removed."
  ${ElseIf} $MdCode == 41
    StrCpy $MdErrorText "Windows process inspection failed. No inference about running miners was made."
  ${ElseIf} $MdCode == 42
    StrCpy $MdErrorText "Windows did not expose a candidate executable path. The log identifies its PID."
  ${ElseIf} $MdCode == 43
    StrCpy $MdErrorText "Task Scheduler could not be queried. This is a diagnostic error, not proof of a running miner."
  ${ElseIf} $MdCode == 61
    StrCpy $MdErrorText "Windows could not register the backend Scheduled Task. See the logged Task Scheduler error and policy; this is not a running-process error."
  ${ElseIf} $MdCode == 62
    StrCpy $MdErrorText "The backend task was registered, but Windows could not start it. Its registration is retained for diagnosis."
  ${ElseIf} $MdCode == 9000
    StrCpy $MdErrorText "Setup could not extract its helper or write the maintenance request."
  ${ElseIf} $MdCode == 9001
    StrCpy $MdErrorText "Windows could not start Windows PowerShell. Check the executable path in the log."
  ${ElseIf} $MdCode == 9002
    StrCpy $MdErrorText "Setup could not create or append to its diagnostic log. Check ProgramData and TEMP write permissions."
  ${ElseIf} $MdCode == 9003
    StrCpy $MdErrorText "The maintenance helper timed out. Setup has stopped rather than replace potentially locked files."
  ${EndIf}
  MessageBox MB_OK|MB_ICONSTOP "MinerDesk 0.7.22: $MdAction failed (code $MdCode).$\r$\n$\r$\n$MdErrorText$\r$\n$\r$\nDiagnostic log (when writable):$\r$\n$MdLogFile" /SD IDOK
FunctionEnd

Function ${PREFIX}MdGuard
  StrCpy $MdApproved 0
  StrCpy $MdAction "Inspect"
  Call ${PREFIX}MdRun
  ${If} $MdCode == 10
    Goto md_guard_prompt
  ${ElseIf} $MdCode != 0
    Call ${PREFIX}MdReportFailure
    Return
  ${EndIf}
  Goto md_guard_execute
md_guard_prompt:
  ; Silent maintenance never acquires implicit permission to kill processes.
  IfSilent md_guard_cancel 0
  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "MinerDesk or its managed mining is active.$\r$\n$\r$\nStop the installed Desktop, its backend and managed miners to continue?$\r$\n$\r$\nThe Setup/uninstall process is excluded." IDYES md_guard_approve IDNO md_guard_cancel
md_guard_cancel:
  StrCpy $MdCode 1602
  Return
md_guard_approve:
  StrCpy $MdApproved 1
md_guard_execute:
  StrCpy $MdAction $MdNextAction
  Call ${PREFIX}MdRun
  ; Catch a process started between inspection and shutdown; ask before killing.
  ${If} $MdCode == 10
    Goto md_guard_prompt
  ${ElseIf} $MdCode != 0
    Call ${PREFIX}MdReportFailure
  ${EndIf}
FunctionEnd
!macroend

!insertmacro MdMaintenanceFunctions "" "setup"
!insertmacro MdMaintenanceFunctions "un." "uninstall"

Function MdRemoveLegacyRegistration
  StrCpy $MdRegCode 0
  ${If} ${RunningX64}
    SetRegView 64
    DeleteRegKey HKLM "${MD_UNINST_KEY}"
    DeleteRegKey HKLM "${MD_SECURITY_KEY}"
    StrCpy $MdRegValue ""
    ReadRegStr $MdRegValue HKLM "${MD_UNINST_KEY}" "UninstallString"
    ${If} $MdRegValue != ""
      StrCpy $MdRegCode 31
      StrCpy $MdLogMessage "64-bit uninstall registration remains: $MdRegValue"
      Call MdLog
    ${EndIf}
  ${EndIf}
  SetRegView 32
  DeleteRegKey HKLM "${MD_UNINST_KEY}"
  DeleteRegKey HKLM "${MD_SECURITY_KEY}"
  StrCpy $MdRegValue ""
  ReadRegStr $MdRegValue HKLM "${MD_UNINST_KEY}" "UninstallString"
  ${If} $MdRegValue != ""
    StrCpy $MdRegCode 31
    StrCpy $MdLogMessage "32-bit uninstall registration remains: $MdRegValue"
    Call MdLog
  ${EndIf}
  !insertmacro MdRestoreRegView
  StrCpy $MdCode $MdRegCode
FunctionEnd

Function MdLegacyCleanup
  StrCpy $MdLegacyVersion ""
  StrCpy $MdLegacyDir ""
  ${If} ${RunningX64}
    SetRegView 64
    ReadRegStr $MdLegacyVersion HKLM "${MD_UNINST_KEY}" "DisplayVersion"
    ReadRegStr $MdLegacyDir HKLM "${MD_UNINST_KEY}" "InstallLocation"
  ${EndIf}
  ${If} $MdLegacyVersion == ""
    SetRegView 32
    ReadRegStr $MdLegacyVersion HKLM "${MD_UNINST_KEY}" "DisplayVersion"
    ReadRegStr $MdLegacyDir HKLM "${MD_UNINST_KEY}" "InstallLocation"
  ${EndIf}
  !insertmacro MdRestoreRegView
  StrCpy $MdCode 0
  StrCmp $MdLegacyVersion "0.7.3" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.4" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.5" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.6" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.7" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.8" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.9" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.10" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.11" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.12" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.13" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.14" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.15" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.16" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.17" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.18" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.19" md_legacy_found 0
  StrCmp $MdLegacyVersion "0.7.20" md_legacy_found 0
  Return
md_legacy_found:
  ; Strip InstallLocation's optional surrounding quotes without interpolating
  ; the resulting directory into PowerShell source.
  StrCpy $MdRegValue $MdLegacyDir 1
  ${If} $MdRegValue == '"'
    StrCpy $MdLegacyDir $MdLegacyDir -1 1
  ${EndIf}
  ${If} $MdLegacyDir == ""
    StrCpy $MdLegacyDir $INSTDIR
  ${EndIf}
  IfSilent md_legacy_cancel 0
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "MinerDesk $MdLegacyVersion is installed.$\r$\n$\r$\nClean up this previous installation using the 0.7.22 maintenance helper?$\r$\n$\r$\nThis stops its runtime/miners and removes only known old MinerDesk binaries and registration. AppData settings and downloaded miners are preserved.$\r$\n$\r$\nContinue?" IDYES md_legacy_continue IDNO md_legacy_cancel
md_legacy_cancel:
  StrCpy $MdCode 1602
  Return
md_legacy_continue:
  StrCpy $MdTargetDir $MdLegacyDir
  StrCpy $MdAction "LegacyCleanup"
  StrCpy $MdApproved 1
  Call MdRun
  ${If} $MdCode != 0
    Call MdReportFailure
    Return
  ${EndIf}
  Call MdRemoveLegacyRegistration
  ${If} $MdCode != 0
    Call MdReportFailure
    Return
  ${EndIf}
  StrCpy $MdLogMessage "Legacy registration removed in both registry views. User data preserved."
  Call MdLog
FunctionEnd

; MUI owns .onGUIInit. Never define a second reserved callback.
!define MUI_CUSTOMFUNCTION_GUIINIT MdGuiInit
Function MdGuiInit
  Call MdInit
  ${If} $MdCode != 0
    Call MdReportFailure
    SetErrorLevel $MdCode
    Quit
  ${EndIf}
  Call MdLegacyCleanup
  ${If} $MdCode != 0
    SetErrorLevel $MdCode
    Quit
  ${EndIf}
  StrCpy $MdTargetDir $INSTDIR
  StrCpy $MdNextAction "Stop"
  Call MdGuard
  ${If} $MdCode != 0
    SetErrorLevel $MdCode
    Quit
  ${EndIf}
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  StrCpy $MdTargetDir $INSTDIR
  StrCpy $MdNextAction "PrepareInstall"
  Call MdGuard
  ${If} $MdCode != 0
    SetErrorLevel $MdCode
    Abort "MinerDesk maintenance did not complete. See the diagnostic log."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$MdDataRoot\miners"
  ClearErrors
  CopyFiles /SILENT "$INSTDIR\resources\minerdesk-headless.exe" "$INSTDIR\minerdesk-headless.exe"
  ${If} ${Errors}
    StrCpy $MdAction "CopyHeadless"
    StrCpy $MdCode 60
    Call MdReportFailure
    Abort "The installed headless resource could not be copied."
  ${EndIf}
  ClearErrors
  CopyFiles /SILENT "$INSTDIR\resources\minerdesk-backend.exe" "$INSTDIR\minerdesk-backend.exe"
  ${If} ${Errors}
    StrCpy $MdAction "CopyWindowlessBackend"
    StrCpy $MdCode 60
    Call MdReportFailure
    Abort "The windowless backend resource could not be copied."
  ${EndIf}
  StrCpy $MdTargetDir $INSTDIR
  StrCpy $MdApproved 1
  WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "BackendTaskAdded" 0
  MessageBox MB_YESNO|MB_ICONQUESTION "Install the privileged MinerDesk backend? (Recommended)$\r$\n$\r$\nA Scheduled Task runs minerdesk-backend.exe --desktop-owned without a terminal window, with highest privileges. The desktop stays non-administrator." /SD IDNO IDNO md_backend_done
    StrCpy $MdAction "InstallBackend"
    Call MdRun
    ${If} $MdCode != 0
      Call MdReportFailure
      Abort "Backend registration failed. See the diagnostic log."
    ${EndIf}
    WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "BackendTaskAdded" 1
md_backend_done:
  WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "CliPathAdded" 0
  MessageBox MB_YESNO|MB_ICONQUESTION "Add MinerDesk CLI to the system PATH?" /SD IDNO IDNO md_path_done
    StrCpy $MdAction "AddPath"
    Call MdRun
    ${If} $MdCode == 0
      WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "CliPathAdded" 1
    ${Else}
      Call MdReportFailure
    ${EndIf}
md_path_done:
  WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "FirewallRulesAdded" 0
  MessageBox MB_YESNO|MB_ICONQUESTION "Allow outbound Windows Firewall rules for MinerDesk-managed mining engines?$\r$\n$\r$\nMinerDesk adds a separate outbound rule for each downloaded miner. No inbound port is opened by this option." /SD IDNO IDNO md_firewall_done
    WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "FirewallRulesAdded" 1
md_firewall_done:
  WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "DefenderExclusionAdded" 0
  MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "Add only $MdDataRoot\miners to Microsoft Defender exclusions?$\r$\n$\r$\nThis reduces antivirus protection for that folder. Do not put unrelated files there." /SD IDNO IDNO md_defender_done
    StrCpy $MdAction "AddDefender"
    Call MdRun
    ${If} $MdCode == 0
      WriteRegDWORD HKLM "${MD_SECURITY_KEY}" "DefenderExclusionAdded" 1
    ${Else}
      Call MdReportFailure
    ${EndIf}
md_defender_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $MdTargetDir $INSTDIR
  StrCpy $MdNextAction "PrepareUninstall"
  Call un.MdGuard
  ${If} $MdCode != 0
    SetErrorLevel $MdCode
    Abort "MinerDesk maintenance did not complete. See the diagnostic log."
  ${EndIf}
  Delete "$INSTDIR\minerdesk-headless.exe"
  Delete "$INSTDIR\minerdesk-backend.exe"
  DeleteRegKey HKLM "${MD_SECURITY_KEY}"
!macroend
