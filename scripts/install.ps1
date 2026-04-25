# Rove installer script for Windows
# Usage:
#   irm https://raw.githubusercontent.com/orvislab/rove/main/scripts/install.ps1 | iex
#   irm https://raw.githubusercontent.com/orvislab/rove/main/scripts/install.ps1 | iex -Channel dev
param(
    [ValidateSet("stable", "dev")]
    [string]$Channel = "stable"
)

$ErrorActionPreference = "Stop"

$Repo = "orvislab/rove"
$R2Base = "https://registry.roveai.co"

if ($Channel -eq "dev") {
    $Binary = "rove-dev"
    $HomeDir = Join-Path $env:USERPROFILE ".rove-dev"
} else {
    $Binary = "rove"
    $HomeDir = Join-Path $env:USERPROFILE ".rove"
}

Write-Host "Rove Installer" -ForegroundColor Cyan
Write-Host "==============" -ForegroundColor Cyan
Write-Host "  Channel: $Channel"
Write-Host "  Binary:  $Binary.exe"
Write-Host "  Home:    $HomeDir"

# Detect architecture
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($Arch) {
    "X64"   { $Target = "windows-x86_64" }
    "Arm64" { $Target = "windows-aarch64" }
    default {
        Write-Host "Error: Unsupported architecture: $Arch" -ForegroundColor Red
        exit 1
    }
}

Write-Host "  Arch:    $Arch ($Target)"
Write-Host ""

# Fetch channel-scoped manifest from R2.
$ManifestUrl = "$R2Base/$Channel/engine/manifest.json"
Write-Host "Fetching $Channel manifest..."
try {
    $Manifest = Invoke-RestMethod -Uri $ManifestUrl -UseBasicParsing
} catch {
    Write-Host "Error: Failed to fetch $ManifestUrl" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}

$Engine = $Manifest.engines.$Target
if (-not $Engine) {
    Write-Host "Error: manifest has no release for target '$Target'" -ForegroundColor Red
    exit 1
}

$Version = $Engine.version
if (-not $Version) { $Version = $Manifest.engines.latest.version }
$Url = $Engine.url
$Fallback = $Engine.fallback_url
$ExpectedHash = $Engine.sha256

if (-not $Url) {
    Write-Host "Error: manifest entry for '$Target' missing 'url'" -ForegroundColor Red
    exit 1
}

Write-Host "  Latest version: $Version"
Write-Host ""

# Download
$TempFile = Join-Path $env:TEMP "$Binary-$Target.exe"
Write-Host "Downloading from R2: $Url"
try {
    Invoke-WebRequest -Uri $Url -OutFile $TempFile -UseBasicParsing
} catch {
    if ($Fallback) {
        Write-Host "R2 download failed, trying GitHub fallback: $Fallback" -ForegroundColor Yellow
        Invoke-WebRequest -Uri $Fallback -OutFile $TempFile -UseBasicParsing
    } else {
        Write-Host "Error: download failed and manifest has no fallback_url." -ForegroundColor Red
        exit 1
    }
}

# Verify SHA-256 (BLAKE3-hex in practice — matches cli/update.rs)
if ($ExpectedHash) {
    Write-Host "Verifying SHA-256..."
    $Actual = (Get-FileHash -Algorithm SHA256 -Path $TempFile).Hash.ToLower()
    if ($Actual -ne $ExpectedHash.ToLower()) {
        Remove-Item $TempFile -ErrorAction SilentlyContinue
        Write-Host "Error: hash mismatch" -ForegroundColor Red
        Write-Host "  Expected: $ExpectedHash"
        Write-Host "  Got:      $Actual"
        exit 1
    }
    Write-Host "  verified."
}

# Install to user local bin
$InstallDir = Join-Path $env:LOCALAPPDATA "Rove\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$InstallPath = Join-Path $InstallDir "$Binary.exe"
Move-Item -Path $TempFile -Destination $InstallPath -Force

# Channel marker so the binary can detect a hand-configured home dir.
if (-not (Test-Path $HomeDir)) {
    New-Item -ItemType Directory -Path $HomeDir -Force | Out-Null
}
Set-Content -Path (Join-Path $HomeDir "channel") -Value $Channel -NoNewline

Write-Host ""
Write-Host "Installed to $InstallPath" -ForegroundColor Green

# Add to PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to user PATH" -ForegroundColor Green
    Write-Host "Restart your terminal for PATH changes to take effect." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Run '$Binary setup' to configure." -ForegroundColor Cyan
Write-Host "Run '$Binary doctor' to verify installation." -ForegroundColor Cyan
if ($Channel -eq "dev") {
    Write-Host ""
    Write-Host "Dev channel: engine auto-updates daily at UTC 00:00." -ForegroundColor Cyan
}
