# FluxDM — Browser Extension Native Host Installer (Windows)
# Run as Administrator for HKLM, or normal user for HKCU
# Usage: .\scripts\install-extension.ps1 -ExtensionId YOUR_CHROME_EXTENSION_ID

param(
    [Parameter(Mandatory=$true)]
    [string]$ExtensionId,

    [Parameter()]
    [string]$FluxDMBinaryPath = "$env:LOCALAPPDATA\FluxDM\fluxdm.exe"
)

$ManifestDir  = "$env:LOCALAPPDATA\FluxDM\native-host"
$ManifestFile = "$ManifestDir\com.fluxdm.host.json"

Write-Host "Installing FluxDM native messaging host..." -ForegroundColor Cyan

# Create manifest directory
New-Item -ItemType Directory -Force $ManifestDir | Out-Null

# Write the native host manifest
$manifest = @{
    name              = "com.fluxdm.host"
    description       = "FluxDM native messaging host"
    path              = $FluxDMBinaryPath
    type              = "stdio"
    allowed_origins   = @("chrome-extension://$ExtensionId/")
} | ConvertTo-Json

$manifest | Out-File -FilePath $ManifestFile -Encoding utf8

Write-Host "  Manifest written to: $ManifestFile" -ForegroundColor Green

# Register in Windows Registry for Chrome
$ChromeRegKey = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.fluxdm.host"
New-Item -Path $ChromeRegKey -Force | Out-Null
Set-ItemProperty -Path $ChromeRegKey -Name "(Default)" -Value $ManifestFile
Write-Host "  Chrome registry key set: $ChromeRegKey" -ForegroundColor Green

# Register for Edge (Chromium-based)
$EdgeRegKey = "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.fluxdm.host"
New-Item -Path $EdgeRegKey -Force | Out-Null
Set-ItemProperty -Path $EdgeRegKey -Name "(Default)" -Value $ManifestFile
Write-Host "  Edge registry key set:   $EdgeRegKey" -ForegroundColor Green

Write-Host ""
Write-Host "Done! Restart Chrome/Edge and reload the extension." -ForegroundColor Green
