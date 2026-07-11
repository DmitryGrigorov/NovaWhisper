Staging area for the self-contained gateway sidecar.

Run scripts/build-gateway.sh (macOS/Linux) or scripts/build-gateway.ps1
(Windows) from the repository root to build the PyInstaller bundle into
this directory as whispr-gateway/whispr-gateway(.exe). Everything here is
bundled into the installer as Tauri resources; the desktop app finds and
starts it automatically (see apps/desktop/src-tauri/src/gateway.rs).

This placeholder file keeps the directory present so builds without a
sidecar keep working: the app then falls back to the repo's
server/gateway/.venv or an already-running gateway.
