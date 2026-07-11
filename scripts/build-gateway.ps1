# Build the self-contained gateway sidecar (PyInstaller, onedir) and stage it
# where the Tauri bundler and the desktop app expect it:
#   apps\desktop\src-tauri\gateway\whispr-gateway\whispr-gateway.exe
# Then `cargo tauri build` produces a setup .exe that starts the gateway
# automatically — no Python needed on the target PC.
#
# Note: the CUDA build bundles cuBLAS/cuDNN and is several GB. If the NSIS
# installer build fails on size, ship the portable layout instead: put the
# whispr-gateway folder next to whispr-desktop.exe (the app finds it there).
$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$gw = Join-Path $repo "server\gateway"
$stage = Join-Path $repo "apps\desktop\src-tauri\gateway"
$venv = Join-Path $gw ".venv-build"
$work = Join-Path $env:TEMP "whispr-gateway-build"

py -m venv $venv
& "$venv\Scripts\python.exe" -m pip install --upgrade pip
& "$venv\Scripts\pip.exe" install -r "$gw\requirements.txt" -r "$gw\requirements-local.txt" pyinstaller

$collect = @(
  "--collect-all", "faster_whisper",
  "--collect-all", "ctranslate2"
)
# CUDA DLL packages exist only when requirements-local installed them (Windows).
& "$venv\Scripts\pip.exe" show nvidia-cublas-cu12 *> $null
if ($LASTEXITCODE -eq 0) { $collect += @("--collect-all", "nvidia") }

if (Test-Path "$stage\whispr-gateway") { Remove-Item -Recurse -Force "$stage\whispr-gateway" }
& "$venv\Scripts\pyinstaller.exe" --noconfirm --clean --onedir --name whispr-gateway `
  --distpath $stage `
  --workpath $work `
  --specpath $work `
  --paths $gw `
  @collect `
  (Join-Path $gw "run_gateway.py")

Write-Host ""
Write-Host "Sidecar staged at $stage\whispr-gateway\"
Write-Host "Smoke test: $stage\whispr-gateway\whispr-gateway.exe --port 8765  (then GET /healthz)"
Write-Host "Now run: cargo tauri build"
