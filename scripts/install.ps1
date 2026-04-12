# Rove installer script for Windows
# Usage: irm https://raw.githubusercontent.com/orvislab/rove/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "orvislab/rove"
$Binary = "rove"

Write-Host "Rove Installer" -ForegroundColor Cyan
Write-Host "==============" -ForegroundColor Cyan

# Detect architecture
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($Arch) {
    "X64"   { $Target = "x86_64-pc-windows-msvc" }
    "Arm64" { $Target = "aarch64-pc-windows-msvc" }
    default {
        Write-Host "Error: Unsupported architecture: $Arch" -ForegroundColor Red
        exit 1
    }
}

$AssetName = "$Binary-$Target.exe"

Write-Host "  Arch:   $Arch ($Target)"
Write-Host "  Asset:  $AssetName"
Write-Host ""

# Fetch latest release
Write-Host "Fetching latest release..."
$Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "Accept" = "application/vnd.github+json" }
$Tag = $Release.tag_name

if (-not $Tag) {
    Write-Host "Error: Could not determine latest release" -ForegroundColor Red
    exit 1
}

Write-Host "  Latest version: $Tag"
Write-Host ""

# Find asset URL
$Asset = $Release.assets | Where-Object { $_.name -eq $AssetName }
if (-not $Asset) {
    $Available = ($Release.assets | ForEach-Object { $_.name }) -join ", "
    Write-Host "Error: No asset found for $AssetName" -ForegroundColor Red
    Write-Host "Available: $Available"
    exit 1
}

# Download
$TempFile = Join-Path $env:TEMP $AssetName
Write-Host "Downloading $AssetName..."
Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $TempFile -UseBasicParsing

# Install to user local bin
$InstallDir = Join-Path $env:LOCALAPPDATA "Rove\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$InstallPath = Join-Path $InstallDir "$Binary.exe"
Move-Item -Path $TempFile -Destination $InstallPath -Force

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
Write-Host "Run 'rove setup' to configure." -ForegroundColor Cyan
Write-Host "Run 'rove doctor' to verify installation." -ForegroundColor Cyan
