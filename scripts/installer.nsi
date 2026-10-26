; mDm Media Download Manager — Self-Contained Windows NSIS Installer Script
Unicode True

!define PRODUCT_NAME "mDm Media Download Manager"
!define PRODUCT_SHORT_NAME "mDm"
!define PRODUCT_VERSION "0.1.0-alpha.1"
!define PRODUCT_PUBLISHER "mDm Project"
!define PRODUCT_WEB_SITE "https://github.com/habitual69/mdm"
!define MAIN_EXE "mDm Media Download Manager.exe"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

; Installer Attributes
Name "${PRODUCT_NAME}"
OutFile "..\dist\mDm_Setup_${PRODUCT_VERSION}_x64.exe"
InstallDir "$LOCALAPPDATA\Programs\mDm Media Download Manager"
InstallDirRegKey HKCU "Software\mDm" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; Interface Settings
!define MUI_ABORTWARNING
!define MUI_ICON "..\mdm-app\src-tauri\icons\icon.ico"
!define MUI_UNICON "..\mdm-app\src-tauri\icons\icon.ico"

; Installer Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

; Finish Page options
!define MUI_FINISHPAGE_RUN "$INSTDIR\${MAIN_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${PRODUCT_NAME}"
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_CHECKED
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Open Browser Extension folder to install"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION OpenExtensionFolder
!insertmacro MUI_PAGE_FINISH

; Uninstaller Pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ---------------------------------------------------------------------------
; Helper function to open extension folder on completion if requested
; ---------------------------------------------------------------------------
Function OpenExtensionFolder
    ${If} ${FileExists} "$INSTDIR\extension\manifest.json"
        ExecShell "explore" "$INSTDIR\extension"
    ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------
; Helper function to check and install WebView2 Runtime if missing
; ---------------------------------------------------------------------------
Function EnsureWebView2
    DetailPrint "Checking Microsoft Edge WebView2 Runtime..."
    
    ; Check 64-bit and 32-bit registry paths for WebView2
    ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-F552-44E6-B609-64643E1682A0}" "pv"
    ${If} $0 == ""
        ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-F552-44E6-B609-64643E1682A0}" "pv"
    ${EndIf}
    ${If} $0 == ""
        ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\{F3017226-F552-44E6-B609-64643E1682A0}" "pv"
    ${EndIf}

    ${If} $0 != ""
        DetailPrint "Microsoft Edge WebView2 Runtime is already installed ($0)."
    ${Else}
        DetailPrint "WebView2 Runtime not found. Installing via bundled bootstrapper..."
        ${If} ${FileExists} "$INSTDIR\bin\MicrosoftEdgeWebview2Setup.exe"
            ExecWait '"$INSTDIR\bin\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
            DetailPrint "WebView2 installer finished with exit code $1."
        ${Else}
            DetailPrint "MicrosoftEdgeWebview2Setup.exe not found; proceeding..."
        ${EndIf}
    ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------
; Helper function to check and update yt-dlp and aria2c if network is available
; ---------------------------------------------------------------------------
Function UpdateEnginesIfOnline
    DetailPrint "Checking network connectivity for engine updates..."
    ; Quick network probe (1 packet, 1500ms max timeout)
    nsExec::Exec 'ping -n 1 -w 1500 1.1.1.1'
    Pop $0
    ${If} $0 == 0
        DetailPrint "Network available. Checking for yt-dlp updates..."
        ${If} ${FileExists} "$INSTDIR\bin\yt-dlp.exe"
            nsExec::Exec '"$INSTDIR\bin\yt-dlp.exe" -U'
            Pop $1
            DetailPrint "yt-dlp update check completed ($1)."
        ${EndIf}

        DetailPrint "Checking for aria2c updates..."
        ${If} ${FileExists} "$INSTDIR\bin\mdm.exe"
            nsExec::Exec '"$INSTDIR\bin\mdm.exe" setup --force --only aria2c'
            Pop $2
            DetailPrint "aria2c update check completed ($2)."
        ${EndIf}
    ${Else}
        DetailPrint "No internet connectivity detected. Offline bundled binaries preserved."
    ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------
; Main Installation Section
; ---------------------------------------------------------------------------
Section "MainSection" SEC01
    SetShellVarContext current

    ; Terminate any existing running instances before upgrading / installing
    DetailPrint "Checking for running instances of ${PRODUCT_NAME}..."
    nsExec::Exec 'taskkill /F /IM "${MAIN_EXE}" /T'
    nsExec::Exec 'taskkill /F /IM "mdm-app.exe" /T'
    nsExec::Exec 'taskkill /F /IM "mdm.exe" /T'
    nsExec::Exec 'taskkill /F /IM "aria2c.exe" /T'
    nsExec::Exec 'taskkill /F /IM "yt-dlp.exe" /T'
    Sleep 800

    SetOutPath "$INSTDIR"
    SetOverwrite on

    ; 1. Root application files
    File "..\dist\package\${MAIN_EXE}"
    File "..\dist\package\WebView2Loader.dll"
    File /oname=app-icon.ico "..\mdm-app\src-tauri\icons\icon.ico"

    ; 2. Bundled dependencies
    SetOutPath "$INSTDIR\bin"
    File "..\dist\package\bin\yt-dlp.exe"
    File "..\dist\package\bin\aria2c.exe"
    File /nonfatal "..\dist\package\bin\mdm.exe"
    File /nonfatal "..\dist\package\bin\ffmpeg.exe"
    File /nonfatal "..\dist\package\bin\ffprobe.exe"
    File /nonfatal "..\dist\package\bin\MicrosoftEdgeWebview2Setup.exe"
    File /nonfatal "..\dist\package\bin\WebView2Loader.dll"

    ; 3. Bundled browser extension
    SetOutPath "$INSTDIR\extension"
    File /r "..\dist\package\extension\*.*"

    ; 4. Run WebView2 validation
    Call EnsureWebView2

    ; 5. Check for updated yt-dlp and aria2 if network available
    Call UpdateEnginesIfOnline

    ; 5. Create Start Menu Shortcuts
    SetOutPath "$INSTDIR"
    CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
    CreateShortcut "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk" "$INSTDIR\${MAIN_EXE}" "" "$INSTDIR\app-icon.ico" 0
    CreateShortcut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\uninstall.exe" 0

    ; 5. Create Desktop Shortcut
    CreateShortcut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${MAIN_EXE}" "" "$INSTDIR\app-icon.ico" 0

    ; 6. Write Registry entries for Add/Remove Programs
    WriteRegStr HKCU "Software\mDm" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "DisplayName" "${PRODUCT_NAME}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "DisplayIcon" "$INSTDIR\${MAIN_EXE},0"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "DisplayVersion" "${PRODUCT_VERSION}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "Publisher" "${PRODUCT_PUBLISHER}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "InstallLocation" "$INSTDIR"
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "NoRepair" 1

    ; Estimate installed size
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm" "EstimatedSize" "$0"

    ; 7. Write Uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

; ---------------------------------------------------------------------------
; Uninstaller Section
; ---------------------------------------------------------------------------
Section "Uninstall"
    SetShellVarContext current

    ; 1. Terminate running application and background engines
    DetailPrint "Closing running instances of ${PRODUCT_NAME}..."
    nsExec::Exec 'taskkill /F /IM "${MAIN_EXE}" /T'
    nsExec::Exec 'taskkill /F /IM "mdm-app.exe" /T'
    nsExec::Exec 'taskkill /F /IM "mdm.exe" /T'
    nsExec::Exec 'taskkill /F /IM "aria2c.exe" /T'
    nsExec::Exec 'taskkill /F /IM "yt-dlp.exe" /T'
    Sleep 1200

    ; 2. Remove shortcuts
    Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk"
    RMDir "$SMPROGRAMS\${PRODUCT_NAME}"

    ; 3. Clean up application directories completely
    RMDir /r "$INSTDIR\bin"
    RMDir /r "$INSTDIR\extension"
    Delete "$INSTDIR\${MAIN_EXE}"
    Delete "$INSTDIR\WebView2Loader.dll"
    Delete "$INSTDIR\app-icon.ico"
    Delete "$INSTDIR\uninstall.exe"
    RMDir /r "$INSTDIR"

    ; Schedule background self-cleanup for uninstall.exe and $INSTDIR if anything remains
    IfFileExists "$INSTDIR" 0 +2
        Exec '"$SYSDIR\cmd.exe" /c ping 127.0.0.1 -n 2 >nul & rmdir /s /q "$INSTDIR"'

    ; 4. Remove registry keys
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\mDm"
    DeleteRegKey HKCU "Software\mDm"
SectionEnd
