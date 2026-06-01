# ==============================================================================
# FluxDM Native Messaging Host — Windows Installer
# ==============================================================================
# Registers the native messaging host so Chrome and Firefox can communicate
# with the FluxDM desktop app.
#
# Usage (run from the FluxDM installation directory):
#   .\extension\install.ps1
#
# Or with explicit binary path:
#   .\extension\install.ps1 -BinaryPath "C:\Program Files\FluxDM\fluxdm-host.exe"
#
# Uninstall:
#   .\extension\install.ps1 -Uninstall
# ==============================================================================

param(
    [string]  $BinaryPath = "",
    [switch]  $Uninstall,
    [switch]  $Firefox,
    [switch]  $Chrome = $true
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$HostName       = "com.fluxdm.host"
$ChromeRegPath  = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"
$EdgeRegPath    = "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$HostName"
$FirefoxAppData = "$env:APPDATA\Mozilla\NativeMessagingHosts"

# ── Locate binary ──────────────────────────────────────────────────────────────

if (-not $BinaryPath) {
    # Look next to this script, or next to common install paths
    $candidates = @(
        (Join-Path $PSScriptRoot "..\fluxdm-host.exe"),
        (Join-Path $PSScriptRoot "..\target\release\fluxdm-host.exe"),
        "C:\Program Files\FluxDM\fluxdm-host.exe",
        "$env:LOCALAPPDATA\FluxDM\fluxdm-host.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { $BinaryPath = (Resolve-Path $c).Path; break }
    }
}

if (-not $Uninstall -and -not $BinaryPath) {
    Write-Error @"
Could not find fluxdm-host.exe automatically.
Please specify the path explicitly:
  .\install.ps1 -BinaryPath "C:\path\to\fluxdm-host.exe"
"@
    exit 1
}

# ── Uninstall ──────────────────────────────────────────────────────────────────

if ($Uninstall) {
    Write-Host "Removing FluxDM native messaging host registration..." -ForegroundColor Yellow
    foreach ($reg in @($ChromeRegPath, $EdgeRegPath)) {
        if (Test-Path $reg) { Remove-Item $reg -Recurse -Force; Write-Host "  Removed $reg" }
    }
    $ffManifest = Join-Path $FirefoxAppData "$HostName.json"
    if (Test-Path $ffManifest) { Remove-Item $ffManifest -Force; Write-Host "  Removed $ffManifest" }
    Write-Host "Done. FluxDM native host unregistered." -ForegroundColor Green
    exit 0
}

$BinaryPath = (Resolve-Path $BinaryPath).Path
Write-Host "Installing FluxDM native messaging host..." -ForegroundColor Cyan
Write-Host "  Binary: $BinaryPath"

# ── Chrome / Edge manifest (registry) ─────────────────────────────────────────

# The manifest JSON lives next to the binary for easy relocation
$ManifestDir  = Split-Path $BinaryPath -Parent
$ManifestPath = Join-Path $ManifestDir "com.fluxdm.host.json"

# Read the extension ID from storage or use the placeholder
$ExtensionId = "YOUR_CHROME_EXTENSION_ID"
try {
    $stored = Get-ItemPropertyValue "HKCU:\Software\FluxDM" "ChromeExtensionId" -ErrorAction SilentlyContinue
    if ($stored) { $ExtensionId = $stored }
} catch {}

$ChromeManifest = @{
    name            = $HostName
    description     = "FluxDM native messaging host"
    path            = $BinaryPath
    type            = "stdio"
    allowed_origins = @("chrome-extension://$ExtensionId/")
} | ConvertTo-Json -Depth 3

$ChromeManifest | Set-Content -Path $ManifestPath -Encoding utf8
Write-Host "  Manifest: $ManifestPath"

# Register in Chrome registry
if (-not (Test-Path $ChromeRegPath)) { New-Item -Path $ChromeRegPath -Force | Out-Null }
Set-ItemProperty -Path $ChromeRegPath -Name "(Default)" -Value $ManifestPath
Write-Host "  Chrome registry: $ChromeRegPath"

# Register in Edge registry (same manifest works)
if (-not (Test-Path $EdgeRegPath)) { New-Item -Path $EdgeRegPath -Force | Out-Null }
Set-ItemProperty -Path $EdgeRegPath -Name "(Default)" -Value $ManifestPath
Write-Host "  Edge registry:   $EdgeRegPath"

# ── Firefox manifest (AppData file) ───────────────────────────────────────────

if ($Firefox) {
    if (-not (Test-Path $FirefoxAppData)) { New-Item -ItemType Directory -Path $FirefoxAppData | Out-Null }

    $FirefoxManifest = @{
        name               = $HostName
        description        = "FluxDM native messaging host"
        path               = $BinaryPath
        type               = "stdio"
        allowed_extensions = @("fluxdm@fluxdev.app")
    } | ConvertTo-Json -Depth 3

    $ffManifestPath = Join-Path $FirefoxAppData "$HostName.json"
    $FirefoxManifest | Set-Content -Path $ffManifestPath -Encoding utf8
    Write-Host "  Firefox manifest: $ffManifestPath"
}

# ── Done ───────────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "FluxDM native messaging host installed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Install the FluxDM Chrome extension from the Chrome Web Store"
Write-Host "     (or load unpacked from: $(Split-Path $PSScriptRoot -Parent)\extension)"
Write-Host "  2. Open FluxDM and start downloading!"
Write-Host ""
Write-Host "To update the Chrome extension ID after publishing:" -ForegroundColor DarkGray
Write-Host "  Set-ItemProperty 'HKCU:\Software\FluxDM' ChromeExtensionId '<your-id>'"
Write-Host "  Then re-run this script."
