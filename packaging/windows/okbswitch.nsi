; Installer for Own Keyboard Switch.
;
; Built by tools/build-installer.sh, which passes the paths and the version:
;   makensis -DVERSION=0.1.0-beta -DVERSION_NUMERIC=0.1.0.0 \
;            -DSOURCE_EXE=... -DROOT=... -DOUTPUT=... okbswitch.nsi
;
; The user chooses between installing for everyone (the default, into
; C:\Program Files\Okbswitch) and for the current account only (into
; %LOCALAPPDATA%\Programs\Okbswitch, no administrator rights needed).
;
; Administrator rights are requested only when the all-users variant is
; actually chosen: the manifest asks for an ordinary token, and this script
; restarts itself through the `runas` verb at the moment it needs more.

Unicode true
ManifestDPIAware true

!ifndef VERSION
    !error "VERSION is required; build through tools/build-installer.sh"
!endif
!ifndef VERSION_NUMERIC
    !define VERSION_NUMERIC "0.0.0.0"
!endif
!ifndef SOURCE_EXE
    !error "SOURCE_EXE is required; build through tools/build-installer.sh"
!endif
!ifndef ROOT
    !error "ROOT is required; build through tools/build-installer.sh"
!endif
!ifndef OUTPUT
    !define OUTPUT "okbswitch-setup.exe"
!endif

!define NAME "Own Keyboard Switch"
!define SHORT_NAME "Okbswitch"
!define PUBLISHER "coinman-dev"
!define EXE "okbswitch.exe"
; Matches okbs_core::APP_ID: the lock file and the old profile directories.
!define APP_ID "okbswitch"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${SHORT_NAME}"
; The value the program itself writes and reads for «Запускаться при старте».
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define RUN_VALUE "OwnKeyboardSwitch"
; The logon task the program creates for «Запускать с правами Администратора».
!define LOGON_TASK "Own Keyboard Switch"

; ---------------------------------------------------------------- MultiUser

; `Standard` leaves `RequestExecutionLevel user` in the manifest, so Windows
; does not raise the installer before the user has chosen anything. The
; all-users half of MultiUser.nsh is normally tied to an elevating manifest, so
; it is switched on by hand and this script does the elevating itself.
!define MULTIUSER_EXECUTIONLEVEL Standard
!define MULTIUSER_EXECUTIONLEVEL_ALLUSERS
!define MULTIUSER_MUI
!define MULTIUSER_INSTALLMODE_COMMANDLINE
!define MULTIUSER_INSTALLMODE_INSTDIR "${SHORT_NAME}"
!define MULTIUSER_INSTALLMODE_INSTDIR_REGISTRY_KEY "Software\${SHORT_NAME}"
!define MULTIUSER_INSTALLMODE_INSTDIR_REGISTRY_VALUENAME "InstallDir"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTALL_KEY}"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "UninstallString"
!define MULTIUSER_INSTALLMODE_FUNCTION InstallModeChanged
!define MULTIUSER_USE_PROGRAMFILES64

!include "MUI2.nsh"
!include "MultiUser.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

!insertmacro GetSize

Name "${NAME}"
OutFile "${OUTPUT}"
; Empty until MultiUser sets a default, or NSIS parses an explicit /D= path.
InstallDir ""
BrandingText "${NAME} ${VERSION}"
ShowInstDetails show
ShowUninstDetails show
SetCompressor /SOLID lzma

VIProductVersion "${VERSION_NUMERIC}"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${NAME} installer"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 coinman-dev"

!define MUI_ICON "${ROOT}\images\okbswitch.ico"
!define MUI_UNICON "${ROOT}\images\okbswitch.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION RunProgram
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_LANGDLL_ALLLANGUAGES
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "Software\${SHORT_NAME}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Language"

!define MUI_PAGE_CUSTOMFUNCTION_PRE ResumeBeforeComponents
!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_PRE ResumeBeforeComponents
!insertmacro MUI_PAGE_LICENSE "${ROOT}\LICENSE"
; Checked when the install mode page is left, that is as soon as the choice
; between the two variants is known.
!define MULTIUSER_PAGE_CUSTOMFUNCTION_PRE ResumeBeforeComponents
!define MULTIUSER_PAGE_CUSTOMFUNCTION_LEAVE ElevateForAllUsers
!insertmacro MULTIUSER_PAGE_INSTALLMODE
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

; MultiUser.nsh has no install-mode page for the uninstaller: the mode comes
; from the `/AllUsers` or `/CurrentUser` argument written into UninstallString,
; and from the registry when the uninstaller is started by hand.
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Russian first, so it is the default of the language dialog.
!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"

; ----------------------------------------------------------------- strings

LangString DescMain ${LANG_RUSSIAN} "Программа и её файлы. Обязательный компонент."
LangString DescMain ${LANG_ENGLISH} "The program and its files. Required."
LangString NameDesktop ${LANG_RUSSIAN} "Ярлык на рабочем столе"
LangString NameDesktop ${LANG_ENGLISH} "Desktop shortcut"
LangString DescDesktop ${LANG_RUSSIAN} "Создать ярлык на рабочем столе."
LangString DescDesktop ${LANG_ENGLISH} "Create a shortcut on the desktop."
LangString NameAutostart ${LANG_RUSSIAN} "Запускать при входе в систему"
LangString NameAutostart ${LANG_ENGLISH} "Start at logon"
LangString DescAutostart ${LANG_RUSSIAN} "Запускать программу при входе в систему. Потом это можно изменить в её настройках."
LangString DescAutostart ${LANG_ENGLISH} "Start the program when you log in. You can change this later in its settings."
LangString Need64 ${LANG_RUSSIAN} "Программа работает только на 64-разрядной Windows."
LangString Need64 ${LANG_ENGLISH} "The program only runs on 64-bit Windows."
LangString AskRemoveData ${LANG_RUSSIAN} "Удалить также настройки, словари автозамены и историю буфера обмена?$\r$\n$\r$\nЕсли вы собираетесь установить программу заново, выберите «Нет»."
LangString AskRemoveData ${LANG_ENGLISH} "Also delete the settings, autoreplace entries and clipboard history?$\r$\n$\r$\nChoose No if you are going to install the program again."
LangString StoppingProgram ${LANG_RUSSIAN} "Завершение работающей программы..."
LangString StoppingProgram ${LANG_ENGLISH} "Stopping the running program..."
LangString GrantingLog ${LANG_RUSSIAN} "Разрешение записи настроек и журнала в папке программы..."
LangString GrantingLog ${LANG_ENGLISH} "Allowing the settings and log folders to be written..."
LangString KeptData ${LANG_RUSSIAN} "Настройки оставлены в папке:"
LangString KeptData ${LANG_ENGLISH} "The settings were left in:"
LangString NeedAdmin ${LANG_RUSSIAN} "Для установки сразу для всех пользователей нужны права администратора.$\r$\n$\r$\nРазрешите запрос Windows или выберите «Только для меня»."
LangString NeedAdmin ${LANG_ENGLISH} "Installing for all users needs administrator rights.$\r$\n$\r$\nAllow the Windows prompt, or choose “Only for me”."
LangString NeedAdminUninstall ${LANG_RUSSIAN} "Для удаления программы, установленной для всех пользователей, нужны права администратора."
LangString NeedAdminUninstall ${LANG_ENGLISH} "Removing a program installed for all users needs administrator rights."

; ---------------------------------------------------------------- helpers

; The program keeps a global keyboard hook, so its files are locked while it
; runs. A close request first, then force, so nothing is left half-updated.
; Windows utilities write localized output in the OEM code page. Never send
; that raw output to NSIS's Unicode details pane: it would be mojibake. The
; translated DetailPrint text above is the user-facing progress message.
!macro StopProgram UN
Function ${UN}StopProgram
    DetailPrint "$(StoppingProgram)"
    nsExec::Exec '"$SYSDIR\taskkill.exe" /IM "${EXE}"'
    Pop $0
    nsExec::Exec '"$SYSDIR\taskkill.exe" /IM "okbswitch-portable.exe"'
    Pop $0
    Sleep 1500
    nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${EXE}"'
    Pop $0
    nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "okbswitch-portable.exe"'
    Pop $0
    Sleep 500
FunctionEnd
!macroend
!insertmacro StopProgram ""
!insertmacro StopProgram "un."

; With an `asInvoker` manifest the token of an administrator is filtered, and
; `UserInfo::GetAccountType` answers "User" — which makes MultiUser.nsh skip the
; install mode page and silently pick the per-user variant. The original account
; type sees through the filter, and the choice between the two variants comes
; back. Rights themselves are still requested only when the all-users variant is
; chosen; this only restores the offer.
Function RestoreAdminPrivileges
    ${If} $MultiUser.Privileges == "Admin"
    ${OrIf} $MultiUser.Privileges == "Power"
        Return
    ${EndIf}
    UserInfo::GetOriginalAccountType
    Pop $0
    ${If} $0 != "Admin"
        Return
    ${EndIf}
    StrCpy $MultiUser.Privileges "Admin"

    ; Repeat the choice MULTIUSER_INIT_CHECKS makes for an administrator:
    ; all users, unless only a per-user installation exists or the command
    ; line asks for one.
    Call MultiUser.InstallMode.AllUsers
    ReadRegStr $1 HKLM "${UNINSTALL_KEY}" "UninstallString"
    ${If} $1 == ""
        ReadRegStr $1 HKCU "${UNINSTALL_KEY}" "UninstallString"
        ${If} $1 != ""
            Call MultiUser.InstallMode.CurrentUser
        ${EndIf}
    ${EndIf}
    ${GetParameters} $R1
    ${StrStr} $R2 $R1 "/CurrentUser"
    ${If} $R2 != ""
        Call MultiUser.InstallMode.CurrentUser
    ${EndIf}
    ${StrStr} $R2 $R1 "/AllUsers"
    ${If} $R2 != ""
        Call MultiUser.InstallMode.AllUsers
    ${EndIf}
FunctionEnd

; Marks the copy that was started with raised rights, so a directory that
; stays unwritable cannot send the installer round in circles.
!define ELEVATED_FLAG "/elevated"
; Set only by MaybeElevate after the user confirms the all-users choice. The
; raised copy resumes on the Components page instead of asking the completed
; welcome, license and installation-mode questions again.
!define RESUME_COMPONENTS_FLAG "/resume-components"

Var Writable
Var CommandLineInstallDir
Var ResumeComponents

; Whether this process may create and write in $INSTDIR. A standard account,
; and an administrator who has not been elevated, cannot write under Program
; Files — which is exactly the case that needs a UAC request.
!macro ProbeInstallDir UN
Function ${UN}ProbeInstallDir
    StrCpy $Writable "no"
    ; A directory the probe itself makes is removed again, so cancelling the
    ; wizard leaves nothing behind under Program Files.
    ${If} ${FileExists} "$INSTDIR\*.*"
        StrCpy $R3 "kept"
    ${Else}
        StrCpy $R3 "probed"
    ${EndIf}
    ClearErrors
    CreateDirectory "$INSTDIR"
    ${If} ${Errors}
        Return
    ${EndIf}
    ClearErrors
    FileOpen $R0 "$INSTDIR\.okbswitch-write-test" w
    ${If} ${Errors}
        ${If} $R3 == "probed"
            RMDir "$INSTDIR"
        ${EndIf}
        Return
    ${EndIf}
    FileClose $R0
    Delete "$INSTDIR\.okbswitch-write-test"
    StrCpy $Writable "yes"
    ${If} $R3 == "probed"
        ; Only ever removes it while it is still empty; the install section
        ; creates it again for real.
        RMDir "$INSTDIR"
    ${EndIf}
FunctionEnd
!macroend
!insertmacro ProbeInstallDir ""

; Restarts with a raised token when the all-users directory turns out to be
; read-only, carrying the current command line over so a silent run stays
; silent. Leaves $Writable telling the caller whether installing can go on:
; ShellExecute reports failure — a declined prompt included — with a value of
; 32 or less.
Function MaybeElevate
    StrCpy $Writable "yes"
    ${If} $MultiUser.InstallMode != "AllUsers"
        Return
    ${EndIf}
    ; All-users installation also writes HKLM and common shortcuts; a writable
    ; custom destination alone is not proof of administrator rights.
    UserInfo::GetAccountType
    Pop $0
    ${If} $0 == "Admin"
        Call ProbeInstallDir
        Return
    ${EndIf}
    StrCpy $Writable "no"
    ${GetParameters} $R1
    ${StrStr} $R2 $R1 "${ELEVATED_FLAG}"
    ${If} $R2 != ""
        ; Already the raised copy: the directory is unwritable for some other
        ; reason, and asking again would only loop.
        Return
    ${EndIf}
    StrCpy $R3 "/AllUsers ${ELEVATED_FLAG} ${RESUME_COMPONENTS_FLAG} $R1"
    ; NSIS consumes /D= before .onInit, so restore it as the final argument
    ; for the raised copy. This preserves an explicit custom destination.
    ${If} $CommandLineInstallDir != ""
        StrCpy $R3 "$R3 /D=$CommandLineInstallDir"
    ${EndIf}
    System::Call 'shell32::ShellExecuteW(p 0, t "runas", t "$EXEPATH", \
        t "$R3", t "", i 1) i .r0'
    ${If} $0 > 32
        Quit
    ${EndIf}
FunctionEnd

; Leaving the install mode page: the choice between the two variants is known
; here, and this is the first moment that may need administrator rights.
Function ElevateForAllUsers
    Call MaybeElevate
    ${If} $Writable != "yes"
        MessageBox MB_OK|MB_ICONEXCLAMATION "$(NeedAdmin)"
        ; Back to the page, where «Только для меня» still works.
        Abort
    ${EndIf}
FunctionEnd

; Started through Explorer so that an elevated installer does not hand its
; administrator rights to the program: that is what «Запускать с правами
; Администратора» is for, and it has to stay the user's own choice.
Function RunProgram
    Exec '"$WINDIR\explorer.exe" "$INSTDIR\${EXE}"'
FunctionEnd

Function InstallModeChanged
    ${If} $CommandLineInstallDir != ""
        StrCpy $INSTDIR $CommandLineInstallDir
        Return
    ${EndIf}
    ; Keep a directory chosen during an earlier installation; only replace the
    ; plain defaults of MultiUser.nsh with the ones this program wants.
    ${If} $MultiUser.InstallMode == "AllUsers"
        ${If} $INSTDIR == "$PROGRAMFILES64\${SHORT_NAME}"
        ${OrIf} $INSTDIR == "$PROGRAMFILES32\${SHORT_NAME}"
            StrCpy $INSTDIR "$PROGRAMFILES64\${SHORT_NAME}"
        ${EndIf}
    ${Else}
        ${If} $INSTDIR == "$LOCALAPPDATA\${SHORT_NAME}"
            StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${SHORT_NAME}"
        ${EndIf}
    ${EndIf}
FunctionEnd

; The all-users choice is made on the page immediately before Components.
; After UAC starts the elevated copy, the preceding pages are already
; complete. Aborting their pre-callbacks moves the wizard directly forward
; without resetting the user's place.
Function ResumeBeforeComponents
    ${If} $ResumeComponents == "yes"
        Abort
    ${EndIf}
FunctionEnd

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_OK|MB_ICONSTOP "$(Need64)"
        Abort
    ${EndIf}
    SetRegView 64
    ; NSIS has already parsed the special, last /D= argument (including spaces).
    ; Preserve that result rather than re-parsing the path as ordinary options.
    ; /D= is consumed and removed from $CMDLINE by NSIS before .onInit.
    StrCpy $CommandLineInstallDir $INSTDIR
    StrCpy $ResumeComponents "no"
    ${GetParameters} $R1
    ${StrStr} $R2 $R1 "${ELEVATED_FLAG}"
    ${If} $R2 != ""
        ${StrStr} $R2 $R1 "${RESUME_COMPONENTS_FLAG}"
        ${If} $R2 != ""
            StrCpy $ResumeComponents "yes"
        ${EndIf}
    ${EndIf}
    !insertmacro MUI_LANGDLL_DISPLAY
    !insertmacro MULTIUSER_INIT
    Call RestoreAdminPrivileges
    ; Respect the documented NSIS /D= override after MultiUser chooses defaults.
    ${If} $CommandLineInstallDir != ""
        StrCpy $INSTDIR $CommandLineInstallDir
    ${EndIf}
    ; A silent run shows no pages, so the all-users check has to happen here.
    ${If} ${Silent}
        Call MaybeElevate
        ${If} $Writable != "yes"
            SetErrorLevel 740 ; ERROR_ELEVATION_REQUIRED
            Quit
        ${EndIf}
    ${EndIf}
FunctionEnd

Function un.onInit
    SetRegView 64
    !insertmacro MUI_UNGETLANGUAGE
    !insertmacro MULTIUSER_UNINIT
    ; Removing an all-users installation needs the same rights its files do.
    ${If} $MultiUser.Privileges != "Admin"
    ${AndIf} $MultiUser.Privileges != "Power"
        UserInfo::GetOriginalAccountType
        Pop $0
        ${If} $0 == "Admin"
            StrCpy $MultiUser.Privileges "Admin"
        ${EndIf}
    ${EndIf}
    ${If} $MultiUser.InstallMode == "AllUsers"
        ${un.GetParameters} $R1
        ${UnStrStr} $R2 $R1 "${ELEVATED_FLAG}"
        ${If} $R2 == ""
            UserInfo::GetAccountType
            Pop $0
            ${If} $0 != "Admin"
                System::Call 'shell32::ShellExecuteW(p 0, t "runas", \
                    t "$INSTDIR\uninstall.exe", t "/AllUsers ${ELEVATED_FLAG} $R1", \
                    t "", i 1) i .r0'
                ${If} $0 > 32
                    Quit
                ${EndIf}
                MessageBox MB_OK|MB_ICONEXCLAMATION "$(NeedAdminUninstall)"
                Quit
            ${EndIf}
        ${EndIf}
    ${EndIf}
FunctionEnd

; --------------------------------------------------------------- sections

Section "!${NAME}" SecMain
    SectionIn RO
    ${GetRoot} "$INSTDIR" $0
    ${If} $0 == ""
    ${OrIf} $INSTDIR == "$0"
    ${OrIf} $INSTDIR == "$0\"
        SetErrorLevel 87
        Abort "Choose a program subdirectory, not a drive root."
    ${EndIf}
    SetOutPath "$INSTDIR"
    Call StopProgram

    File "/oname=${EXE}" "${SOURCE_EXE}"
    File "/oname=LICENSE.txt" "${ROOT}\LICENSE"
    File "/oname=NOTICE.txt" "${ROOT}\NOTICE"
    File "/oname=LANGUAGE-LICENSES.txt" "${ROOT}\data\LICENSES.md"
    File "/oname=DICTIONARY-LICENSES.txt" "${ROOT}\data\hunspell\README_en_US.txt"
    File "/oname=THIRD-PARTY-NOTICES.txt" "${ROOT}\THIRD-PARTY-NOTICES.txt"

    ; The program keeps its settings, the clipboard history and the lock in
    ; `data`, and «Диагностика» writes the log in `log`. Under Program Files
    ; both belong to administrators, so ordinary accounts are given the right
    ; to write there — and only there, because a writable program file would
    ; let any account replace what the next administrator runs. Without this
    ; startup reports an error rather than creating data elsewhere.
    CreateDirectory "$INSTDIR\data"
    CreateDirectory "$INSTDIR\log"
    ${If} $MultiUser.InstallMode == "AllUsers"
        DetailPrint "$(GrantingLog)"
        nsExec::Exec '"$SYSDIR\icacls.exe" "$INSTDIR\data" /grant *S-1-5-32-545:(OI)(CI)M'
        Pop $0
        nsExec::Exec '"$SYSDIR\icacls.exe" "$INSTDIR\log" /grant *S-1-5-32-545:(OI)(CI)M'
        Pop $0
    ${EndIf}

    CreateDirectory "$SMPROGRAMS\${NAME}"
    CreateShortcut "$SMPROGRAMS\${NAME}\${NAME}.lnk" "$INSTDIR\${EXE}"

    WriteRegStr SHCTX "Software\${SHORT_NAME}" "InstallDir" "$INSTDIR"
    WriteUninstaller "$INSTDIR\uninstall.exe"

    WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe" /$MultiUser.InstallMode'
    WriteRegStr SHCTX "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /$MultiUser.InstallMode /S'
    WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "NoRepair" 1
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD SHCTX "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section /o "$(NameDesktop)" SecDesktop
    CreateShortcut "$DESKTOP\${NAME}.lnk" "$INSTDIR\${EXE}"
SectionEnd

Section "$(NameAutostart)" SecAutostart
    ; The same value the program writes itself, so its «Запускаться при
    ; старте» checkbox shows the real state after the first run.
    WriteRegStr HKCU "${RUN_KEY}" "${RUN_VALUE}" '"$INSTDIR\${EXE}"'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "$(DescMain)"
    !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "$(DescDesktop)"
    !insertmacro MUI_DESCRIPTION_TEXT ${SecAutostart} "$(DescAutostart)"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ------------------------------------------------------------- uninstall

Section "Uninstall"
    ${un.GetRoot} "$INSTDIR" $0
    ${If} $0 == ""
    ${OrIf} $INSTDIR == "$0"
    ${OrIf} $INSTDIR == "$0\"
        SetErrorLevel 87
        Abort "Invalid installation directory."
    ${EndIf}
    Call un.StopProgram

    ; Leave another installation's autostart alone.
    ReadRegStr $0 HKCU "${RUN_KEY}" "${RUN_VALUE}"
    ${If} $0 == '"$INSTDIR\${EXE}"'
        DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"
    ${EndIf}
    ; «Запускать с правами Администратора» may have replaced it with a task.
    ${If} $MultiUser.InstallMode == "AllUsers"
        nsExec::Exec '"$SYSDIR\schtasks.exe" /Delete /TN "${LOGON_TASK}" /F'
        Pop $0
    ${EndIf}

    Delete "$SMPROGRAMS\${NAME}\${NAME}.lnk"
    RMDir "$SMPROGRAMS\${NAME}"
    Delete "$DESKTOP\${NAME}.lnk"

    Delete "$INSTDIR\${EXE}"
    Delete "$INSTDIR\LICENSE.txt"
    Delete "$INSTDIR\NOTICE.txt"
    Delete "$INSTDIR\LANGUAGE-LICENSES.txt"
    Delete "$INSTDIR\DICTIONARY-LICENSES.txt"
    Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
    RMDir /r "$INSTDIR\log"
    Delete "$INSTDIR\data\${APP_ID}.lock"

    DeleteRegKey SHCTX "${UNINSTALL_KEY}"
    DeleteRegKey SHCTX "Software\${SHORT_NAME}"

    Call un.AskAboutUserData
    ; Last, so a failure above still leaves a way to remove the program.
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR\data"
    RMDir "$INSTDIR"
SectionEnd

; The settings and the clipboard history are worth keeping across a reinstall,
; so removing them is a separate answer; a silent uninstall keeps them. The
; only this installation's data is removed. Older profile files are handled
; by the program's migration code, with backups and no recursive deletion.
Function un.AskAboutUserData
    ${If} ${Silent}
        Return
    ${EndIf}
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "$(AskRemoveData)" /SD IDNO IDYES remove
    ${If} ${FileExists} "$INSTDIR\data\config.toml"
        MessageBox MB_OK|MB_ICONINFORMATION "$(KeptData)$\r$\n$INSTDIR\data"
    ${EndIf}
    Return
    remove:
    RMDir /r "$INSTDIR\data"
FunctionEnd
