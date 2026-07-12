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

# WHISPR_GATEWAY_BACKEND can force mlx or cpu. Auto uses MLX only on Apple
# Silicon and faster-whisper CPU everywhere else. Windows CUDA has its own
# PowerShell build script.
backend="${WHISPR_GATEWAY_BACKEND:-auto}"
if [[ "$backend" == "auto" ]]; then
  if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
    backend="mlx"
  else
    backend="cpu"
  fi
fi

case "$backend" in
  mlx)
    requirements="$gw/requirements-mlx.txt"
    collect=(--collect-all mlx_whisper --collect-all mlx)
    ;;
  cpu)
    requirements="$gw/requirements-faster.txt"
    collect=(--collect-all faster_whisper --collect-all ctranslate2)
    ;;
  *)
    echo "Unsupported backend '$backend' (expected auto, mlx, or cpu)" >&2
    exit 2
    ;;
esac

python3 -m venv "$venv"
"$venv/bin/pip" install --upgrade pip
"$venv/bin/pip" install -r "$gw/requirements.txt" -r "$requirements" pyinstaller

rm -rf "$stage/whispr-gateway"
"$venv/bin/pyinstaller" --noconfirm --clean --onedir --name whispr-gateway \
  --distpath "$stage" \
  --workpath "$work" \
  --specpath "$work" \
  --paths "$gw" \
  "${collect[@]}" \
  "$gw/run_gateway.py"

echo
echo "Backend: $backend"
echo "Sidecar staged at $stage/whispr-gateway/"
echo "Smoke test: $stage/whispr-gateway/whispr-gateway --port 8765  (then GET /healthz)"
echo "Now run: cargo tauri build"
