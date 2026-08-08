# Standalone installers

The release design is a Tauri native binary plus a PyInstaller `onedir`
sidecar. End users need neither Python nor Rust. `frontendDist` embeds the HTML,
CSS, and JavaScript into the Tauri binary; `bundle.resources` installs the
staged `gateway` directory. Build on the target OS—PyInstaller and native Tauri
bundlers do not cross-compile release packages reliably.

## Prerequisites common to every platform

Install Rust stable, the Tauri CLI (`cargo install tauri-cli --locked`), Python
3.11 or 3.12, and the platform's Tauri system prerequisites. Create the test
environment in `server/gateway/.venv` and install the base plus selected backend
requirements. The release scripts create a separate `.venv-build` for the
frozen sidecar.

## Windows 10/11 (x64)

Install Visual Studio Build Tools with Desktop C++, WebView2, Python x64, and
WiX Toolset prerequisites used by Tauri. From a Developer PowerShell:

```powershell
py -3.12 -m venv server\gateway\.venv
server\gateway\.venv\Scripts\pip install -r server\gateway\requirements.txt
scripts\build-release.ps1 -Backend cpu
# NVIDIA distribution (several GB):
scripts\build-release.ps1 -Backend cuda
```

Outputs are in `target\release\bundle\nsis` (`.exe`) and
`target\release\bundle\msi`. Sign both with an organization code-signing
certificate, then verify with `Get-AuthenticodeSignature`. Test installation,
dictation, gateway shutdown, upgrade, and uninstall in a clean VM.

## Linux x86_64

Install the Tauri packages for the distribution (WebKitGTK 4.1, GTK3,
librsvg2, OpenSSL, build-essential, patchelf, and AppImage tooling), then:

```bash
python3.12 -m venv server/gateway/.venv
server/gateway/.venv/bin/pip install -r server/gateway/requirements.txt
WHISPR_GATEWAY_BACKEND=cpu scripts/build-release.sh
```

The `.deb` and `.AppImage` appear under `target/release/bundle`. Build on the
oldest supported glibc distribution (commonly Ubuntu 22.04) for portability.
For Flatpak, copy the release `whispr-desktop`, staged `gateway`, icon, and a
desktop entry into `packaging/flatpak/payload`, then run:

```bash
flatpak-builder --force-clean build-dir packaging/flatpak/ai.whispr.desktop.yml
flatpak-builder --run build-dir packaging/flatpak/ai.whispr.desktop.yml whispr-desktop
flatpak-builder --repo=repo --force-clean build-dir packaging/flatpak/ai.whispr.desktop.yml
flatpak build-bundle repo NovaWhisper.flatpak ai.whispr.desktop
```

The manifest intentionally requests microphone, graphics, IPC/network, and
global-shortcut portal access. Validate portal behavior on both GNOME and KDE.

## macOS 11+ (Apple Silicon and Intel)

Install Xcode Command Line Tools and matching native Python builds. Produce one
release per architecture because the frozen Python/ML libraries are
architecture-specific:

```bash
# Run on Apple Silicon
arch -arm64 python3.12 -m venv server/gateway/.venv
WHISPR_GATEWAY_BACKEND=mlx scripts/build-release.sh

# Run on Intel hardware (or an x86_64 CI runner)
arch -x86_64 python3.12 -m venv server/gateway/.venv
WHISPR_GATEWAY_BACKEND=cpu scripts/build-release.sh
```

Each run produces `.app` and `.dmg` bundles under `target/release/bundle`.
Publish separate `arm64` and `x86_64` DMGs. Do not use `lipo` only on the Rust
executable: doing so leaves the bundled Python extensions single-architecture.
A true universal bundle requires universal2 wheels for every dependency and a
universal PyInstaller sidecar.

For distribution, configure Apple signing identity and team settings, enable
hardened runtime, sign nested sidecar binaries before the outer app, notarize
the DMG with `xcrun notarytool`, and staple both app/DMG tickets. The user must
grant Microphone and Accessibility permissions; verify those flows on a clean
macOS account.

## Artifact checks

For every OS, inspect the installed resource tree for
`gateway/whispr-gateway/whispr-gateway[.exe]`, run that binary on a temporary
loopback port, verify `/healthz`, and ensure the app never kills a gateway it
did not spawn. Scan the final artifacts, record SHA-256 checksums, and retain
the exact Cargo lockfile and backend Python constraints used for the build.
