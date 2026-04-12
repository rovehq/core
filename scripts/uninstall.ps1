# ─────────────────────────────────────────────
# 🗑️  Rove Uninstaller (Windows)
# Usage: irm https://roveai.co/uninstall.ps1 | iex
# ─────────────────────────────────────────────

$ErrorActionPreference = "Stop"

$Binary = "rove"

Write-Host ""
Write-Host "  ╭──────────────────────────╮" -ForegroundColor Red
Write-Host "  │    Rove Uninstaller       │" -ForegroundColor Red
Write-Host "  ╰──────────────────────────╯" -ForegroundColor Red
Write-Host ""

# ── Find installations ──

$InstallDir = Join-Path $env:LOCALAPPDATA "Rove\bin"
$BinaryPath = Join-Path $InstallDir "$Binary.exe"
$Found = $false

if (Test-Path $BinaryPath) {
    Write-Host "  Found: $BinaryPath" -ForegroundColor Cyan
    $Found = $true
}

if (-not $Found) {
    Write-Host "No Rove installation found." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Checked: $BinaryPath"
    exit 0
}

# ── Confirm ──

$Confirm = Read-Host "  Remove Rove completely? [y/N]"
if ($Confirm -ne "y" -and $Confirm -ne "Y") {
    Write-Host "Aborted."
    exit 0
}

Write-Host ""

# ── Stop process if running ──

$Procs = Get-Process -Name $Binary -ErrorAction SilentlyContinue
if ($Procs) {
    Write-Host "  Stopping Rove..." -NoNewline
    Stop-Process -Name $Binary -Force -ErrorAction SilentlyContinue
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove binary ──

if (Test-Path $BinaryPath) {
    Write-Host "  Removing $BinaryPath..." -NoNewline
    Remove-Item -Path $BinaryPath -Force
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove install directory if empty ──

$RoveDir = Join-Path $env:LOCALAPPDATA "Rove"
if ((Test-Path $RoveDir) -and ((Get-ChildItem $RoveDir -Recurse -File).Count -eq 0)) {
    Remove-Item -Path $RoveDir -Recurse -Force
}

# ── Remove config ──

$ConfigDir = Join-Path $env:APPDATA "rove"
if (Test-Path $ConfigDir) {
    Write-Host "  Removing config $ConfigDir..." -NoNewline
    Remove-Item -Path $ConfigDir -Recurse -Force
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove data ──

$DataDir = Join-Path $env:LOCALAPPDATA "rove\data"
if (Test-Path $DataDir) {
    Write-Host "  Removing data $DataDir..." -NoNewline
    Remove-Item -Path $DataDir -Recurse -Force
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove cache ──

$CacheDir = Join-Path $env:LOCALAPPDATA "rove\cache"
if (Test-Path $CacheDir) {
    Write-Host "  Removing cache $CacheDir..." -NoNewline
    Remove-Item -Path $CacheDir -Recurse -Force
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove plugins ──

$PluginDir = Join-Path $env:USERPROFILE ".rove"
if (Test-Path $PluginDir) {
    Write-Host "  Removing plugins $PluginDir..." -NoNewline
    Remove-Item -Path $PluginDir -Recurse -Force
    Write-Host " ✓" -ForegroundColor Green
}

# ── Remove from PATH ──

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -like "*$InstallDir*") {
    Write-Host "  Removing from PATH..." -NoNewline
    $NewPath = ($UserPath.Split(";") | Where-Object { $_ -ne $InstallDir }) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host " ✓" -ForegroundColor Green
}

# ── Clean up Rove directory entirely ──

if (Test-Path $RoveDir) {
    Write-Host "  Removing $RoveDir..." -NoNewline
    Remove-Item -Path $RoveDir -Recurse -Force
    Write-Host " ✓" -ForegroundColor Green
}

Write-Host ""
Write-Host "Rove has been completely uninstalled." -ForegroundColor Green
Write-Host ""
