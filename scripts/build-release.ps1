param(
  [ValidateSet("cpu", "cuda")][string]$Backend = "cpu",
  [switch]$SkipTests
)
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if (-not $SkipTests) {
  cargo test --workspace
  & "$repo\server\gateway\.venv\Scripts\python.exe" -m pytest
}
$env:WHISPR_GATEWAY_BACKEND = $Backend
& "$PSScriptRoot\build-gateway.ps1"
Push-Location "$repo\apps\desktop"
try { cargo tauri build --config src-tauri/tauri.windows.conf.json } finally { Pop-Location }
Write-Host "Installers are under target\release\bundle\nsis and target\release\bundle\msi"
