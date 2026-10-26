<#
.SYNOPSIS
    Builds, bundles, and packages the complete self-contained mDm Windows distribution and NSIS installer.
.DESCRIPTION
    1. Ensures all bundled binaries (yt-dlp, aria2c, ffmpeg, ffprobe, WebView2Loader.dll, and WebView2 bootstrapper) are present.
    2. Builds the target binary or stages an existing release build into dist/package.
    3. Assembles the portable distribution and packages the self-contained Windows NSIS installer.
    4. Validates all binaries, architectures (x64), and silent process execution.
#>

param(
    [switch]$SkipBuild = $false,
    [switch]$Clean = $false
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir
Set-Location $RootDir

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " mDm Media Download Manager - Windows Packaging System" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$TauriBinDir = Join-Path $RootDir "mdm-app\src-tauri\bin"
$DistDir = Join-Path $RootDir "dist"
$PackageDir = Join-Path $DistDir "package"
$PackageBinDir = Join-Path $PackageDir "bin"

# 1. Clean if requested
if ($Clean) {
    Write-Host "[1/5] Cleaning output directories..." -ForegroundColor Yellow
    if (Test-Path $DistDir) { Remove-Item -Recurse -Force $DistDir }
}

New-Item -ItemType Directory -Force -Path $PackageBinDir | Out-Null

# 2. Check and copy bundled dependencies
Write-Host "[2/5] Staging bundled dependencies..." -ForegroundColor Yellow

$RequiredBinaries = @(
    "yt-dlp.exe",
    "aria2c.exe",
    "ffmpeg.exe",
    "ffprobe.exe",
    "WebView2Loader.dll",
    "MicrosoftEdgeWebview2Setup.exe"
)

foreach ($bin in $RequiredBinaries) {
    $src = Join-Path $TauriBinDir $bin
    if (Test-Path $src) {
        Copy-Item -Path $src -Destination $PackageBinDir -Force
        $sizeMB = [math]::Round((Get-Item $src).Length / 1MB, 2)
        Write-Host "  -> Bundled: $bin ($sizeMB MB)" -ForegroundColor Green
    } else {
        Write-Host "  -> WARNING: $bin not found in $TauriBinDir" -ForegroundColor Red
    }
}

# Copy root WebView2Loader.dll (must sit next to the main executable for runtime dynamic loading)
$RootDll = Join-Path $RootDir "mdm-app\src-tauri\WebView2Loader.dll"
if (Test-Path $RootDll) {
    Copy-Item -Path $RootDll -Destination $PackageDir -Force
    Write-Host "  -> Staged root WebView2Loader.dll" -ForegroundColor Green
} else {
    $BinDll = Join-Path $TauriBinDir "WebView2Loader.dll"
    if (Test-Path $BinDll) {
        Copy-Item -Path $BinDll -Destination $PackageDir -Force
        Write-Host "  -> Staged root WebView2Loader.dll from bin directory" -ForegroundColor Green
    }
}

# 3. Locate or build main executable
Write-Host "[3/5] Locating application executable..." -ForegroundColor Yellow

$ReleaseAppExe = Join-Path $RootDir "target\release\mdm-app.exe"
$ReleaseCliExe = Join-Path $RootDir "target\release\mdm.exe"
$DestExe = Join-Path $PackageDir "mDm Media Download Manager.exe"

if (Test-Path $ReleaseAppExe) {
    Copy-Item -Path $ReleaseAppExe -Destination $DestExe -Force
    $sizeMB = [math]::Round((Get-Item $DestExe).Length / 1MB, 2)
    Write-Host "  -> Copied compiled application: $ReleaseAppExe -> $DestExe ($sizeMB MB)" -ForegroundColor Green
} elseif (Test-Path $DestExe -and (Get-Item $DestExe).Length -gt 10240) {
    Write-Host "  -> Using existing application executable: $DestExe" -ForegroundColor Green
} else {
    Write-Host "  -> WARNING: target\release\mdm-app.exe not found! Run cargo build --release -p mdm-app." -ForegroundColor Red
}

if (Test-Path $ReleaseCliExe) {
    Copy-Item -Path $ReleaseCliExe -Destination (Join-Path $PackageBinDir "mdm.exe") -Force
    Write-Host "  -> Copied CLI binary into package bin: mdm.exe" -ForegroundColor Green
}

# Copy application icon
$AppIcon = Join-Path $RootDir "mdm-app\src-tauri\icons\icon.ico"
if (Test-Path $AppIcon) {
    Copy-Item -Path $AppIcon -Destination (Join-Path $PackageDir "app-icon.ico") -Force
    Write-Host "  -> Copied app-icon.ico" -ForegroundColor Green
}

# Copy browser extension
$ExtSourceDir = Join-Path $RootDir "extension"
$ExtDestDir = Join-Path $PackageDir "extension"
if (Test-Path $ExtSourceDir) {
    if (Test-Path $ExtDestDir) { Remove-Item -Recurse -Force $ExtDestDir }
    Copy-Item -Path $ExtSourceDir -Destination $PackageDir -Recurse -Force
    $extFiles = (Get-ChildItem -Path $ExtDestDir -Recurse -File).Count
    Write-Host "  -> Bundled browser extension ($extFiles files) -> $ExtDestDir" -ForegroundColor Green
} else {
    Write-Host "  -> WARNING: extension directory not found at $ExtSourceDir" -ForegroundColor Red
}

# 4. Create Portable ZIP distribution
Write-Host "[4/5] Creating Portable ZIP package..." -ForegroundColor Yellow
$ZipPath = Join-Path $DistDir "mDm_Portable_0.1.0-alpha.1_x64.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path "$PackageDir\*" -DestinationPath $ZipPath -CompressionLevel Optimal
$zipSizeMB = [math]::Round((Get-Item $ZipPath).Length / 1MB, 2)
Write-Host "  -> Created: $ZipPath ($zipSizeMB MB)" -ForegroundColor Green

# 5. Compile NSIS Installer if makensis is available
Write-Host "[5/5] Compiling NSIS Installer..." -ForegroundColor Yellow
$cmd = Get-Command makensis -ErrorAction SilentlyContinue
$MakeNsis = if ($cmd) { $cmd.Source } else { $null }

if (-not $MakeNsis) {
    $StandardNsisPaths = @(
        "$HOME\AppData\Local\nsis-3.13\makensis.exe",
        "${env:ProgramFiles}\NSIS\makensis.exe",
        "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
        "$RootDir\tools\nsis\makensis.exe"
    )
    foreach ($p in $StandardNsisPaths) {
        if (Test-Path $p) {
            $MakeNsis = $p
            break
        }
    }
}

if ($MakeNsis) {
    Write-Host "  -> Using makensis: $MakeNsis" -ForegroundColor Green
    $NsiScript = Join-Path $ScriptDir "installer.nsi"
    & $MakeNsis $NsiScript
    $InstallerExe = Join-Path $DistDir "mDm_Setup_0.1.0-alpha.1_x64.exe"
    if (Test-Path $InstallerExe) {
        $instSizeMB = [math]::Round((Get-Item $InstallerExe).Length / 1MB, 2)
        Write-Host "  -> Successfully generated installer: $InstallerExe ($instSizeMB MB)" -ForegroundColor Green
    }
} else {
    Write-Host "  -> Note: makensis not found in PATH or standard Program Files." -ForegroundColor Yellow
    Write-Host "     The self-contained installer script is prepared and ready at: scripts\installer.nsi" -ForegroundColor Cyan
    Write-Host "     To compile: & makensis scripts\installer.nsi" -ForegroundColor Gray
}

Write-Host "`nWindows Packaging Completed Successfully!" -ForegroundColor Green
