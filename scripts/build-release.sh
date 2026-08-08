#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
platform="$(uname -s)"
backend="${WHISPR_GATEWAY_BACKEND:-auto}"

if [[ "${SKIP_TESTS:-0}" != "1" ]]; then
  cargo test --workspace
  "$repo/server/gateway/.venv/bin/python" -m pytest
fi
WHISPR_GATEWAY_BACKEND="$backend" "$repo/scripts/build-gateway.sh"
cd "$repo/apps/desktop"

case "$platform" in
  Darwin)
    cargo tauri build --config src-tauri/tauri.macos.conf.json
    echo "Bundles are under target/release/bundle/macos and target/release/bundle/dmg"
    ;;
  Linux)
    cargo tauri build
    echo "Bundles are under target/release/bundle/deb and target/release/bundle/appimage"
    ;;
  *)
    echo "Use scripts/build-release.ps1 on Windows" >&2
    exit 2
    ;;
esac
