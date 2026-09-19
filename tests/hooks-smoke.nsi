; Compile-only test of all installer/uninstaller hooks using standard NSIS/MUI.
; Even if accidentally run, this test executable exits immediately.
Unicode true
!include "MUI2.nsh"
!define ARCH "x64"
!ifndef HOOKS_FILE
  !define HOOKS_FILE "..\src-tauri\windows\hooks.nsh"
!endif
!include "${HOOKS_FILE}"
Name "MinerDesk hook compilation test (inert)"
OutFile "hooks-smoke-test.exe"
RequestExecutionLevel user
InstallDir "$TEMP\MinerDesk-hook-test"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "French"
Function .onInit
  Quit
FunctionEnd
Function un.onInit
  Quit
FunctionEnd
Section "CompileInstall"
  !insertmacro NSIS_HOOK_PREINSTALL
  !insertmacro NSIS_HOOK_POSTINSTALL
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd
Section "Uninstall"
  !insertmacro NSIS_HOOK_PREUNINSTALL
SectionEnd
