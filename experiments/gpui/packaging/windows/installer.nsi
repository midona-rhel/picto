; The Windows installer of Picto, `Picto-Setup-<version>.exe`. `package.sh`
; compiles this script with makensis and passes these values:
;
;   VERSION            the version in the repository's package.json
;   VERSION_NUMBERS    the same version as four numbers, for example 0.6.19.0
;   SOURCE             the folder with the files to install
;   OUTPUT             the installer to write
;   ICON               build/icon.ico
;   ESTIMATED_SIZE_KB  the size of SOURCE in kilobytes
;
; The installer takes the place of the installer that electron-builder made
; for the Electron app, and it upgrades an installation made by that
; installer in place. For that it uses the same registry keys, the same
; installation folder, the same shortcuts and the same command-line arguments
; as electron-builder's installer. Where a value below has to match, the
; comment says so. The script uses only what NSIS 3 ships with: no plugin
; has to be installed.
;
; The installer and the uninstaller never delete the user's application data.
; They are not signed, because the project has no Windows certificate.

Unicode true
ManifestDPIAware true
; The installer starts without administrator rights. An installation for all
; users starts the installer a second time with administrator rights
; (`ElevateForAllUsers`).
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!include "nsDialogs.nsh"

!ifndef VERSION
  !error "Pass -DVERSION, -DVERSION_NUMBERS, -DSOURCE, -DOUTPUT, -DICON and -DESTIMATED_SIZE_KB."
!endif

!define PRODUCT_NAME "Picto"
!define PRODUCT_DESCRIPTION "Media library manager"
!define APP_EXE "Picto.exe"
!define UNINSTALLER "Uninstall Picto.exe"
; The id that the app gives its process (`APP_ID` in `src/desktop/mod.rs`).
; Windows shows a window of the app on the taskbar button of a shortcut only
; when the shortcut carries the same id.
!define APP_ID "com.picto.desktop"
; electron-builder derives this value from the app id `com.picto.desktop` (a
; version 5 UUID in its own namespace) and names both registry keys after it.
; The value must not change, or an existing installation is not found.
!define APP_GUID "7dbe711a-2259-5c88-af70-ded36653dd46"
!define INSTALL_KEY "Software\${APP_GUID}"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_GUID}"
; The file class of `.picto-pack` files, as electron-builder's installer
; registers it.
!define PACK_CLASS "Picto Pack"
!define PACK_DESCRIPTION "Portable Picto media package"

Name "${PRODUCT_NAME}"
OutFile "${OUTPUT}"
BrandingText "${PRODUCT_NAME} ${VERSION}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION_NUMBERS}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey "CompanyName" "${PRODUCT_NAME}"
VIAddVersionKey "LegalCopyright" "${PRODUCT_NAME}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"

; "all" for an installation for all users, "CurrentUser" for one for the
; current user only.
Var installMode
; "1" when the command line or an existing installation has decided
; `installMode`, so that the installer does not ask.
Var modeDecided
; The folder of an existing installation for the current user, or "".
Var userFolder
; The folder of an existing installation for all users, or "".
Var machineFolder
; The folder given on the command line with `/D=`, or "".
Var givenFolder
; "1" when the installer was started with `--updated`, which the app's
; updater and the Electron app's updater pass.
Var isUpdated
; "1" when the installer was started with `--force-run`: start Picto after a
; silent installation.
Var isForceRun
; "1" when the installer was started with `--no-desktop-shortcut`.
Var noDesktopShortcut
; "1" when the shortcuts of the previous version are kept as they are.
Var keepShortcuts
Var modeDialog
Var modeUserRadio
Var modeAllRadio

; Sets RESULT to "1" when the command line holds FLAG, and to "0" otherwise.
!macro HasFlag FLAG RESULT
  ${GetParameters} $R8
  ClearErrors
  ${GetOptions} $R8 "${FLAG}" $R9
  ${If} ${Errors}
    StrCpy ${RESULT} "0"
  ${Else}
    StrCpy ${RESULT} "1"
  ${EndIf}
  ClearErrors
!macroend

; Defines the function `CloseRunningApp` (PREFIX "") or `un.CloseRunningApp`
; (PREFIX "un."). The function waits until no program is running from the
; installation folder. When Picto was started by its updater it is already
; quitting, so the function waits ten seconds for it before it ends the
; process. Otherwise it asks the user first, unless the installer is silent.
!macro CloseRunningAppFunction PREFIX
Function ${PREFIX}CloseRunningApp
  StrCpy $R1 0
  check:
    ; PowerShell ends with code 0 when a process runs from the installation
    ; folder and with code 1 when none does. Any other answer means that
    ; PowerShell could not be used, and the installer then goes on.
    nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "if (@(Get-CimInstance -ClassName Win32_Process | Where-Object { $$_.Path -and $$_.Path.StartsWith('$INSTDIR\', 'CurrentCultureIgnoreCase') }).Count -gt 0) { exit 0 } else { exit 1 }"`
    Pop $R0
    ${If} $R0 != 0
      Return
    ${EndIf}
    IntOp $R1 $R1 + 1
    ${If} $R1 == 1
    ${AndIf} $isUpdated != "1"
      MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "${PRODUCT_NAME} is running. Choose OK to close it and continue." /SD IDOK IDOK wait
      Quit
    ${EndIf}
    ${If} $R1 > 30
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${PRODUCT_NAME} could not be closed. Close it yourself, then choose Retry." /SD IDCANCEL IDRETRY again
      SetErrorLevel 2
      Quit
      again:
        StrCpy $R1 1
    ${EndIf}
    ${If} $isUpdated != "1"
    ${OrIf} $R1 > 10
      nsExec::Exec `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Get-CimInstance -ClassName Win32_Process | Where-Object { $$_.Path -and $$_.Path.StartsWith('$INSTDIR\', 'CurrentCultureIgnoreCase') } | ForEach-Object { Stop-Process -Id $$_.ProcessId -Force }"`
      Pop $R0
    ${EndIf}
  wait:
    Sleep 1000
    Goto check
FunctionEnd
!macroend

!insertmacro CloseRunningAppFunction ""
!insertmacro CloseRunningAppFunction "un."

; The pages of the installer. The first two are shown only for a new
; installation that the user started. An upgrade keeps the kind and the
; folder of the installation it finds.
Page custom ModePageShow ModePageLeave
!define MUI_PAGE_CUSTOMFUNCTION_PRE DirectoryPagePre
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE DirectoryPageLeave
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION StartApp
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Sets the shell folders and the installation folder for `installMode`. The
; folder is the one given with `/D=`, else the folder of the existing
; installation, else the folder electron-builder's installer uses: `Picto` in
; the user's `Programs` folder, or `Picto` in `Program Files`.
Function ApplyMode
  ${If} $installMode == "all"
    SetShellVarContext all
    ${If} $machineFolder != ""
      StrCpy $INSTDIR $machineFolder
    ${Else}
      StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCT_NAME}"
    ${EndIf}
  ${Else}
    SetShellVarContext current
    ${If} $userFolder != ""
      StrCpy $INSTDIR $userFolder
    ${Else}
      StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"
    ${EndIf}
  ${EndIf}
  ${If} $givenFolder != ""
    StrCpy $INSTDIR $givenFolder
  ${EndIf}
FunctionEnd

; An installation for all users writes to `Program Files` and to the
; registry of the machine, which needs administrator rights. Without them,
; this function starts the installer again with administrator rights and the
; argument `/allusers`, waits for it, and ends this installer. When the user
; does not give the permission, the installer stays on the page it is on, or
; ends when it has no page yet.
Function ElevateForAllUsers
  ${If} $installMode != "all"
    Return
  ${EndIf}
  UserInfo::GetAccountType
  Pop $R0
  ${If} $R0 == "Admin"
    Return
  ${EndIf}
  ${GetParameters} $R1
  ClearErrors
  ExecShellWait "runas" "$EXEPATH" "/allusers $R1"
  ${IfNot} ${Errors}
    Quit
  ${EndIf}
  MessageBox MB_OK|MB_ICONEXCLAMATION "Installing ${PRODUCT_NAME} for all users needs an administrator's permission, which was not given." /SD IDOK
  Abort
FunctionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "${PRODUCT_NAME} needs a 64-bit version of Windows." /SD IDOK
    Abort
  ${EndIf}
  ; The 64-bit registry, which is where electron-builder's installer writes.
  SetRegView 64
  StrCpy $givenFolder $INSTDIR
  !insertmacro HasFlag "--updated" $isUpdated
  !insertmacro HasFlag "--force-run" $isForceRun
  !insertmacro HasFlag "--no-desktop-shortcut" $noDesktopShortcut
  ReadRegStr $userFolder HKCU "${INSTALL_KEY}" "InstallLocation"
  ReadRegStr $machineFolder HKLM "${INSTALL_KEY}" "InstallLocation"
  !insertmacro HasFlag "/allusers" $R0
  !insertmacro HasFlag "/currentuser" $R1
  StrCpy $modeDecided "1"
  ${If} $R0 == "1"
    StrCpy $installMode "all"
  ${ElseIf} $R1 == "1"
    StrCpy $installMode "CurrentUser"
  ${ElseIf} $userFolder != ""
    StrCpy $installMode "CurrentUser"
  ${ElseIf} $machineFolder != ""
    StrCpy $installMode "all"
  ${Else}
    StrCpy $installMode "CurrentUser"
    ; A silent installation cannot ask, so it installs for the current user.
    ${IfNot} ${Silent}
      StrCpy $modeDecided "0"
    ${EndIf}
  ${EndIf}
  ${If} $modeDecided == "1"
    Call ElevateForAllUsers
  ${EndIf}
  Call ApplyMode
FunctionEnd

; The page that asks whether Picto is installed for the current user or for
; all users.
Function ModePageShow
  ${If} $modeDecided == "1"
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Choose Installation Options" "Who should ${PRODUCT_NAME} be installed for?"
  nsDialogs::Create 1018
  Pop $modeDialog
  ${If} $modeDialog == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 20u "Select whether ${PRODUCT_NAME} is installed for you only or for everyone who uses this computer."
  Pop $R0
  ${NSD_CreateRadioButton} 10u 30u 280u 12u "Only for me"
  Pop $modeUserRadio
  ${NSD_CreateRadioButton} 10u 48u 280u 12u "Anyone who uses this computer (needs an administrator)"
  Pop $modeAllRadio
  ${If} $installMode == "all"
    ${NSD_Check} $modeAllRadio
  ${Else}
    ${NSD_Check} $modeUserRadio
  ${EndIf}
  nsDialogs::Show
FunctionEnd

Function ModePageLeave
  ${NSD_GetState} $modeAllRadio $R0
  ${If} $R0 == ${BST_CHECKED}
    StrCpy $installMode "all"
  ${Else}
    StrCpy $installMode "CurrentUser"
  ${EndIf}
  Call ElevateForAllUsers
  Call ApplyMode
FunctionEnd

; The page that asks for the folder is skipped for an upgrade.
Function DirectoryPagePre
  ${If} $isUpdated == "1"
    Abort
  ${EndIf}
  ${If} $installMode == "all"
  ${AndIf} $machineFolder != ""
    Abort
  ${EndIf}
  ${If} $installMode != "all"
  ${AndIf} $userFolder != ""
    Abort
  ${EndIf}
FunctionEnd

; The uninstaller deletes Picto's files from the installation folder, so the
; folder must be Picto's own. A folder the user chose that is not named
; `Picto` gets a `Picto` folder inside it.
Function DirectoryPageLeave
  ${GetFileName} "$INSTDIR" $R0
  ${If} $R0 != "${PRODUCT_NAME}"
    StrCpy $INSTDIR "$INSTDIR\${PRODUCT_NAME}"
  ${EndIf}
FunctionEnd

; Starts Picto. An installer for all users runs with administrator rights,
; and a program it starts would have them too, so it asks Explorer to start
; Picto, which starts it with the user's own rights.
Function StartApp
  ${If} $installMode == "all"
    Exec '"$WINDIR\explorer.exe" "$INSTDIR\${APP_EXE}"'
  ${Else}
    Exec '"$INSTDIR\${APP_EXE}"'
  ${EndIf}
FunctionEnd

; Writes the app's id (`System.AppUserModel.ID`) into the shortcut whose path
; is on the stack, through the shell's own interfaces: the shortcut is
; loaded (`IPersistFile::Load`), the value is set in its property store
; (`IPropertyStore::SetValue`, `Commit`), and the shortcut is saved
; (`IPersistFile::Save`). electron-builder's installer does the same with a
; plugin.
Function SetShortcutAppId
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  ; $1: the shortcut object (CLSID_ShellLink, as IShellLinkW).
  System::Call 'ole32::CoCreateInstance(g "{00021401-0000-0000-C000-000000000046}", p 0, i 1, g "{000214F9-0000-0000-C000-000000000046}", *p .r1) i .r7'
  ${If} $1 P<> 0
    ; $2: its IPersistFile. Method 5 is Load; the 2 is STGM_READWRITE.
    System::Call '$1->0(g "{0000010B-0000-0000-C000-000000000046}", *p .r2) i .r7'
    ${If} $2 P<> 0
      System::Call '$2->5(w r0, i 2) i .r7'
      ${If} $7 = 0
        ; $3: its IPropertyStore.
        System::Call '$1->0(g "{886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}", *p .r3) i .r7'
        ${If} $3 P<> 0
          ; $4: the property key of System.AppUserModel.ID, which is a GUID
          ; (16 bytes) followed by the number 5.
          System::Alloc 20
          Pop $4
          System::Call 'ole32::CLSIDFromString(w "{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}", p r4) i .r7'
          IntOp $7 $4 + 16
          System::Call '*$7(i 5)'
          ; $5: the id as a string the shell may keep. $6: a PROPVARIANT (16
          ; bytes, all zero) whose type is VT_LPWSTR (31) and whose value, at
          ; byte 8, is that string.
          System::Call 'shlwapi::SHStrDupW(w "${APP_ID}", *p .r5) i .r7'
          System::Alloc 16
          Pop $6
          System::Call '*$6(&i2 31)'
          IntOp $7 $6 + 8
          System::Call '*$7(p r5)'
          ; Method 6 is SetValue and method 7 is Commit.
          System::Call '$3->6(p r4, p r6) i .r7'
          System::Call '$3->7() i .r7'
          ; Method 6 of IPersistFile is Save, here to the file it was loaded
          ; from.
          System::Call '$2->6(p 0, i 1) i .r7'
          System::Call 'ole32::CoTaskMemFree(p r5)'
          System::Free $6
          System::Free $4
          System::Call '$3->2()'
        ${EndIf}
      ${EndIf}
      System::Call '$2->2()'
    ${EndIf}
    System::Call '$1->2()'
  ${EndIf}
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; Removes the version that is installed now, by running its own uninstaller
; in the way electron-builder's installer runs it: silently, from a copy
; outside the installation folder, and with `--updated`, which tells the
; uninstaller that this is an upgrade. The uninstaller of the Electron app
; removes the Electron app's files, and the uninstaller of this app removes
; this app's. Neither deletes the user's application data.
Function RemovePreviousVersion
  StrCpy $keepShortcuts "0"
  ReadRegStr $R0 SHCTX "${UNINSTALL_KEY}" "UninstallString"
  ${If} $R0 == ""
    Return
  ${EndIf}
  ; The value is `"<path of the uninstaller>" /currentuser` or the same with
  ; `/allusers`. $R1 becomes the path between the two quotation marks.
  StrCpy $R1 $R0 "" 1
  StrCpy $R2 0
  find:
    StrCpy $R3 $R1 1 $R2
    ${If} $R3 != '"'
    ${AndIf} $R3 != ""
      IntOp $R2 $R2 + 1
      Goto find
    ${EndIf}
  StrCpy $R1 $R1 $R2
  ${IfNot} ${FileExists} "$R1"
    Return
  ${EndIf}
  ReadRegStr $R4 SHCTX "${INSTALL_KEY}" "InstallLocation"
  ${If} $R4 == ""
    ${GetParent} "$R1" $R4
  ${EndIf}
  ${If} $installMode == "all"
    StrCpy $R5 "/allusers"
  ${Else}
    StrCpy $R5 "/currentuser"
  ${EndIf}
  ; An upgrade started by the updater leaves the shortcuts alone, so that a
  ; shortcut the user moved, renamed or deleted stays as the user left it. An
  ; installation the user started makes the shortcuts again.
  ReadRegStr $R6 SHCTX "${INSTALL_KEY}" "KeepShortcuts"
  ${If} $isUpdated == "1"
  ${AndIf} $R6 == "true"
  ${AndIf} ${FileExists} "$R4\${APP_EXE}"
    StrCpy $keepShortcuts "1"
    StrCpy $R5 "$R5 --keep-shortcuts"
  ${EndIf}
  InitPluginsDir
  CopyFiles /SILENT "$R1" "$PLUGINSDIR\old-uninstaller.exe"
  ClearErrors
  ; `_?=` gives the uninstaller its installation folder and makes it run in
  ; place, so that this installer can wait for it.
  ExecWait '"$PLUGINSDIR\old-uninstaller.exe" /S /KEEP_APP_DATA $R5 --updated _?=$R4' $R7
  ${If} ${Errors}
  ${OrIf} $R7 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "The installed version of ${PRODUCT_NAME} could not be removed (code $R7). Close ${PRODUCT_NAME} and run this installer again." /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
FunctionEnd

Section "Install"
  Call CloseRunningApp
  Call RemovePreviousVersion

  ; The files that `package.sh` put into SOURCE. The uninstaller's section
  ; below deletes the same files and must be changed with that list.
  SetOutPath "$INSTDIR"
  File /r "${SOURCE}\*.*"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"

  ; The registry values that electron-builder's installer writes. This
  ; installer, the next one and the uninstaller find the installation
  ; through them, and Windows lists Picto under "Installed apps" from the
  ; values under UNINSTALL_KEY.
  WriteRegStr SHCTX "${INSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr SHCTX "${INSTALL_KEY}" "KeepShortcuts" "true"
  WriteRegStr SHCTX "${INSTALL_KEY}" "ShortcutName" "${PRODUCT_NAME}"
  ${If} $installMode == "all"
    StrCpy $R0 "/allusers"
  ${Else}
    StrCpy $R0 "/currentuser"
  ${EndIf}
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME} ${VERSION}"
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}" $R0'
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" $R0 /S'
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "Publisher" "${PRODUCT_NAME}"
  WriteRegStr SHCTX "${UNINSTALL_KEY}" "Comments" "${PRODUCT_DESCRIPTION}"
  WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "EstimatedSize" ${ESTIMATED_SIZE_KB}

  ; The shortcuts in the Start menu and on the desktop, with the names and
  ; places electron-builder's installer gives them.
  ${If} $keepShortcuts != "1"
    CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0 "" "" "${PRODUCT_DESCRIPTION}"
    Push "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
    Call SetShortcutAppId
    ${If} $noDesktopShortcut != "1"
      CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0 "" "" "${PRODUCT_DESCRIPTION}"
      Push "$DESKTOP\${PRODUCT_NAME}.lnk"
      Call SetShortcutAppId
    ${EndIf}
  ${EndIf}

  ; `.picto-pack` files open with Picto and show the pack icon, as
  ; electron-builder's installer registers them.
  WriteRegStr SHCTX "Software\Classes\.picto-pack" "" "${PACK_CLASS}"
  WriteRegNone SHCTX "Software\Classes\.picto-pack\OpenWithProgids" "${PACK_CLASS}"
  WriteRegStr SHCTX "Software\Classes\${PACK_CLASS}" "" "${PACK_DESCRIPTION}"
  WriteRegStr SHCTX "Software\Classes\${PACK_CLASS}\DefaultIcon" "" "$INSTDIR\resources\picto-pack.ico"
  WriteRegStr SHCTX "Software\Classes\${PACK_CLASS}\shell" "" "open"
  WriteRegStr SHCTX "Software\Classes\${PACK_CLASS}\shell\open" "" "Open with ${PRODUCT_NAME}"
  WriteRegStr SHCTX "Software\Classes\${PACK_CLASS}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  ; Tells Explorer that file associations and shortcuts changed.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'

  ; After a silent installation the updater asks for Picto to be started
  ; again. An installation with pages offers that on its last page.
  ${If} $isForceRun == "1"
  ${AndIf} ${Silent}
    Call StartApp
  ${EndIf}
SectionEnd

; The uninstaller accepts the arguments that electron-builder's uninstaller
; accepts, because the next version's installer runs it with them:
; `/currentuser` or `/allusers`, `/S`, `--updated`, `--keep-shortcuts`, and
; `/KEEP_APP_DATA`, which changes nothing here because the uninstaller never
; deletes application data.
Function un.onInit
  ${If} ${RunningX64}
    SetRegView 64
  ${EndIf}
  !insertmacro HasFlag "--updated" $isUpdated
  !insertmacro HasFlag "--keep-shortcuts" $keepShortcuts
  !insertmacro HasFlag "/allusers" $R0
  !insertmacro HasFlag "/currentuser" $R1
  ${If} $R0 == "1"
    StrCpy $installMode "all"
  ${ElseIf} $R1 == "1"
    StrCpy $installMode "CurrentUser"
  ${Else}
    ; Without either argument, the installation for the current user is the
    ; one meant when it is in the uninstaller's folder.
    ReadRegStr $R2 HKCU "${INSTALL_KEY}" "InstallLocation"
    ${If} $R2 == $INSTDIR
      StrCpy $installMode "CurrentUser"
    ${Else}
      StrCpy $installMode "all"
    ${EndIf}
  ${EndIf}
  ${If} $installMode == "all"
    SetShellVarContext all
    UserInfo::GetAccountType
    Pop $R0
    ${If} $R0 != "Admin"
      MessageBox MB_OK|MB_ICONEXCLAMATION "${PRODUCT_NAME} is installed for all users. Remove it from the Windows settings, or start this program as an administrator." /SD IDOK
      Abort
    ${EndIf}
  ${Else}
    SetShellVarContext current
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Call un.CloseRunningApp

  ; The files that the installer's section put there, and nothing else. The
  ; folders are removed only when they are empty.
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\ffmpeg.exe"
  Delete "$INSTDIR\ffprobe.exe"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\resources\picto-pack.ico"
  RMDir "$INSTDIR\resources"
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir "$INSTDIR"

  ${If} $keepShortcuts != "1"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
    Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
  ${EndIf}

  DeleteRegValue SHCTX "Software\Classes\.picto-pack\OpenWithProgids" "${PACK_CLASS}"
  DeleteRegKey SHCTX "Software\Classes\${PACK_CLASS}"
  DeleteRegKey SHCTX "${UNINSTALL_KEY}"
  DeleteRegKey SHCTX "${INSTALL_KEY}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
SectionEnd
