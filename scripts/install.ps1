# Rove installer (Windows / PowerShell).
# Usage:
#   irm https://get.roveai.co/install.ps1 | iex
#   $env:ROVE_CHANNEL='dev'; irm https://get.roveai.co/install.ps1 | iex
#
# Env:
#   ROVE_CHANNEL       stable (default) | dev
#   ROVE_REGISTRY_BASE override registry URL
#   ROVE_INSTALL_DIR   override install dir (default %LOCALAPPDATA%\Rove)

$ErrorActionPreference = 'Stop'

$Channel = if ($env:ROVE_CHANNEL) { $env:ROVE_CHANNEL } else { 'stable' }
if ($Channel -notin @('stable', 'dev')) {
    throw "ROVE_CHANNEL must be 'stable' or 'dev' (got '$Channel')"
}

$RegistryBase = if ($env:ROVE_REGISTRY_BASE) { $env:ROVE_REGISTRY_BASE.TrimEnd('/') } else { 'https://registry.roveai.co' }
$BinName = if ($Channel -eq 'dev') { 'rove-dev.exe' } else { 'rove.exe' }

# --- platform detect ---
$arch = if ([Environment]::Is64BitOperatingSystem) { 'x86_64' } else { throw 'unsupported: 32-bit Windows' }
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { $arch = 'aarch64' }
$target = "windows-$arch"
$asset = switch ($target) {
    'windows-x86_64'  { 'rove-x86_64-pc-windows-msvc.exe' }
    'windows-aarch64' { 'rove-aarch64-pc-windows-msvc.exe' }
    default { throw "no published build for $target" }
}

$tmp = Join-Path $env:TEMP "rove-install-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

try {
    # --- fetch manifest ---
    $manifestUrl = "$RegistryBase/$Channel/engine/manifest.json"
    $sigUrl      = "$RegistryBase/$Channel/engine/manifest.sig"
    Write-Host "Fetching manifest: $manifestUrl"
    $manifestJson = Invoke-WebRequest -UseBasicParsing -Uri $manifestUrl | Select-Object -ExpandProperty Content
    try { Invoke-WebRequest -UseBasicParsing -Uri $sigUrl -OutFile (Join-Path $tmp 'manifest.sig') | Out-Null } catch { Write-Warning "signature fetch failed" }

    $manifest = $manifestJson | ConvertFrom-Json
    if ($manifest.channel -ne $Channel) {
        throw "manifest channel mismatch (expected $Channel got '$($manifest.channel)')"
    }

    $latest = $manifest.entries.latest
    $plat = $latest.platforms.$target
    if (-not $plat) { throw "no build for target $target in manifest" }

    $url = $plat.url
    $fallback = $plat.fallback_url
    $expectedHash = $plat.blake3

    # --- download ---
    $payload = Join-Path $tmp $asset
    Write-Host "Downloading $asset ($($latest.version), $Channel channel)..."
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $payload
    } catch {
        if ($fallback) {
            Write-Warning "primary download failed, trying fallback"
            Invoke-WebRequest -UseBasicParsing -Uri $fallback -OutFile $payload
        } else { throw }
    }

    # --- verify (best-effort; BLAKE3 requires external tool) ---
    if ($expectedHash -and (Get-Command b3sum -ErrorAction SilentlyContinue)) {
        $actual = (& b3sum $payload).Split(' ')[0]
        if ($actual -ne $expectedHash) {
            throw "BLAKE3 mismatch. expected=$expectedHash actual=$actual"
        }
        Write-Host "BLAKE3 verified."
    } else {
        Write-Warning "b3sum not available, skipping payload hash verification"
    }

    # --- install ---
    $destDir = if ($env:ROVE_INSTALL_DIR) { $env:ROVE_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Rove' }
    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    $dest = Join-Path $destDir $BinName
    Copy-Item -Path $payload -Destination $dest -Force

    # --- PATH hint ---
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$destDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$destDir", 'User')
        Write-Host "Added $destDir to user PATH (restart shell to pick up)."
    }

    $dataDir = if ($Channel -eq 'dev') { Join-Path $env:USERPROFILE '.rove-dev' } else { Join-Path $env:USERPROFILE '.rove' }
    Write-Host ""
    Write-Host "Installed: $dest (v$($latest.version), $Channel)"
    Write-Host "Data dir:  $dataDir"
    Write-Host ""
    Write-Host "Next: $BinName init"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
