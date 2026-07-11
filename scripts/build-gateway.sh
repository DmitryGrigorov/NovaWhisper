#!/usr/bin/env bash
# Build the self-contained gateway sidecar (PyInstaller, onedir) and stage it
# where the Tauri bundler and the desktop app expect it:
#   apps/desktop/src-tauri/gateway/whispr-gateway/whispr-gateway
# Run from anywhere; then `cargo tauri build` produces installers (deb/dmg)
# that start the gateway automatically — no Python needed on the target box.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
gw="$repo/server/gateway"
stage="$repo/apps/desktop/src-tauri/gateway"
venv="$gw/.venv-build"
work="${TMPDIR:-/tmp}/whispr-gateway-build"

python3 -m venv "$venv"
"$venv/bin/pip" install --upgrade pip
"$venv/bin/pip" install -r "$gw/requirements.txt" -r "$gw/requirements-local.txt" pyinstaller

rm -rf "$stage/whispr-gateway"
"$venv/bin/pyinstaller" --noconfirm --clean --onedir --name whispr-gateway \
  --distpath "$stage" \
  --workpath "$work" \
  --specpath "$work" \
  --paths "$gw" \
  --collect-all faster_whisper \
  --collect-all ctranslate2 \
  "$gw/run_gateway.py"

echo
echo "Sidecar staged at $stage/whispr-gateway/"
echo "Smoke test: $stage/whispr-gateway/whispr-gateway --port 8765  (then GET /healthz)"
echo "Now run: cargo tauri build"
