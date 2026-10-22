; Windows installer for RimWorld Profile Manager: per-user, no admin.
; Built by scripts/installer.sh: makensis -DVERSION=<x.y.z> -DSRC=<bundle dir> -DOUT=<setup.exe>
; Profiles, saves and settings live in %LOCALAPPDATA%\RimWorld Profile Manager and are
; never touched here; neither is the game folder.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!ifndef VERSION | SRC | OUT
  !error "pass -DVERSION, -DSRC and -DOUT"
!endif

!define APP "RimWorld Profile Manager"
!define EXE "rwpm.exe"
; The Steam helper and Valve's library sit beside rwpm.exe (modpack mod fetching).
!define STEAM_EXE "rwpm-steam.exe"
!define STEAM_DLL "steam_api64.dll"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\rwpm"
; `.rimrim` backups and `.rimpack` modpacks open in this app. Per user (HKCU), like the
; rest of the install.
!define CLASSES "Software\Classes"
!define BACKUP_CLASS "rwpm.backup"
!define PACK_CLASS "rwpm.modpack"
; Tells Explorer that file associations changed, so icons and "open with" update at once.
!define SHCNE_ASSOCCHANGED 0x08000000
!define ICON "${__FILEDIR__}\..\..\crates\rwpm\assets\icon\rwpm.ico"

Name "${APP}"
OutFile "${OUT}"
; Not $LOCALAPPDATA\${APP}: that is the data folder.
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "LegalCopyright" "keyz182"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!insertmacro GetParameters
!insertmacro GetOptions
!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"
!define MUI_UNCONFIRMPAGE_TEXT_TOP "Your profiles and saves stay in $LOCALAPPDATA\${APP}.$\r$\n$\r$\nBefore you uninstall, select More, then Remove from game folder in ${APP}, and clear RimWorld's launch options in Steam. If you do not, Steam cannot start the game."

!insertmacro MUI_PAGE_LICENSE "${SRC}\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Sets $R8 to 1 when FILE exists and cannot be opened for writing (rwpm, or the game
; under `rwpm run`, has it open).
!macro CheckFree FILE
  !define ID ${__LINE__}
  IfFileExists "${FILE}" 0 free_${ID}
  ClearErrors
  FileOpen $R9 "${FILE}" a
  IfErrors busy_${ID}
  FileClose $R9
  Goto free_${ID}
  busy_${ID}:
    StrCpy $R8 1
  free_${ID}:
  !undef ID
!macroend

; The in-app updater starts this installer with `/S /RELAUNCH` and then exits. With
; /RELAUNCH, rwpm is started again when the install ends, whether or not it worked, so the
; player is never left without the app. It sees it is still the old version and says so.
Function RelaunchIfAsked
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/RELAUNCH" $R1
  IfErrors done
  Exec '"$INSTDIR\${EXE}"'
  done:
FunctionEnd

; Stops before any file is changed while rwpm or the game is running. A silent run first
; waits up to 30 seconds, because the rwpm that started it is still closing. If the files stay
; busy: exit code 2. GIVE_UP runs just before that exit.
!macro EnsureNotInUse UN GIVE_UP
  Function ${UN}EnsureNotInUse
    StrCpy $R7 0
    retry:
      StrCpy $R8 0
      !insertmacro CheckFree "$INSTDIR\${EXE}"
      !insertmacro CheckFree "$INSTDIR\${STEAM_EXE}"
      !insertmacro CheckFree "$INSTDIR\${STEAM_DLL}"
      !insertmacro CheckFree "$INSTDIR\payload\Rwpm.Payload.dll"
      !insertmacro CheckFree "$INSTDIR\payload\Rwpm.Payload.Hook.dll"
      StrCmp $R8 0 done
      IfSilent 0 ask
        IntCmp $R7 30 give_up
        Sleep 1000
        IntOp $R7 $R7 + 1
        Goto retry
      ask:
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Close RimWorld and ${APP}, then press Retry." IDRETRY retry
    give_up:
      ${GIVE_UP}
      SetErrorLevel 2
      Quit
    done:
  FunctionEnd
!macroend
!insertmacro EnsureNotInUse "" "Call RelaunchIfAsked"
!insertmacro EnsureNotInUse "un." ""

Section "Install"
  Call EnsureNotInUse
  ; Upgrade only (rwpm.exe already there): files an older version shipped but this one
  ; does not must not linger. A folder that is not an rwpm install is never emptied.
  IfFileExists "$INSTDIR\${EXE}" 0 +4
    RMDir /r "$INSTDIR\payload"
    RMDir /r "$INSTDIR\doorstop"
    RMDir /r "$INSTDIR\licenses"
  SetOutPath "$INSTDIR"
  File /r "${SRC}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "keyz182"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  WriteRegStr HKCU "${CLASSES}\.rimrim" "" "${BACKUP_CLASS}"
  WriteRegStr HKCU "${CLASSES}\${BACKUP_CLASS}" "" "${APP} backup"
  WriteRegStr HKCU "${CLASSES}\${BACKUP_CLASS}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${CLASSES}\${BACKUP_CLASS}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
  WriteRegStr HKCU "${CLASSES}\.rimpack" "" "${PACK_CLASS}"
  WriteRegStr HKCU "${CLASSES}\${PACK_CLASS}" "" "${APP} modpack"
  WriteRegStr HKCU "${CLASSES}\${PACK_CLASS}\DefaultIcon" "" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${CLASSES}\${PACK_CLASS}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
  System::Call 'shell32.dll::SHChangeNotify(i, i, i, i) v (${SHCNE_ASSOCCHANGED}, 0, 0, 0)'
  Call RelaunchIfAsked
SectionEnd

Section "Uninstall"
  Call un.EnsureNotInUse
  ; Only what the installer put there; the folder goes only if it is then empty.
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\${STEAM_EXE}"
  Delete "$INSTDIR\${STEAM_DLL}"
  Delete "$INSTDIR\LICENSE"
  IfFileExists "$INSTDIR\uninstall.exe" 0 +4
    RMDir /r "$INSTDIR\payload"
    RMDir /r "$INSTDIR\doorstop"
    RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  ; Only our own association: another program may have taken `.rimrim` since.
  ReadRegStr $0 HKCU "${CLASSES}\.rimrim" ""
  StrCmp $0 "${BACKUP_CLASS}" 0 +2
    DeleteRegKey HKCU "${CLASSES}\.rimrim"
  DeleteRegKey HKCU "${CLASSES}\${BACKUP_CLASS}"
  ReadRegStr $0 HKCU "${CLASSES}\.rimpack" ""
  StrCmp $0 "${PACK_CLASS}" 0 +2
    DeleteRegKey HKCU "${CLASSES}\.rimpack"
  DeleteRegKey HKCU "${CLASSES}\${PACK_CLASS}"
  System::Call 'shell32.dll::SHChangeNotify(i, i, i, i) v (${SHCNE_ASSOCCHANGED}, 0, 0, 0)'
SectionEnd
