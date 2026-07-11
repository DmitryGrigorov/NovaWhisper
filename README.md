# NovaWhisper

**English** | [Русский](README.ru.md)

Cross-platform voice-to-text dictation. Press a global hotkey in any app,
speak, and the polished transcript is inserted at the cursor.

## Product demo

![NovaWhisper voice dictation demo](assets/ScreenCapture.gif)

NovaWhisper runs as two processes:

1. The Python/FastAPI **gateway** performs speech recognition and polishing.
2. The Rust/Tauri **desktop app** captures the microphone, displays the HUD,
   and inserts text.

The desktop `.exe` does not transcribe by itself; the gateway must be running.

## Tested environment

The current Windows/CUDA path was built and tested on:

| Component | Tested value |
|---|---|
| Operating system | Windows version 25H2, build 26200, 64-bit |
| Registry product label | Windows 10 Pro (Windows may retain this label for newer builds) |
| GPU | NVIDIA GeForce RTX 3060, 12 GB VRAM |
| NVIDIA driver | 610.74 |
| Python | 3.14.6 |
| Rust / Cargo | 1.97.0, MSVC toolchain |
| Local STT | faster-whisper, multilingual `small`, CUDA `float16` |

Windows 11 is the primary supported Windows target. Linux builds are supported
with the limitations described below; macOS builds are supported on Apple
Silicon using the steps below.

## Windows installation from a clean PC

### 1. Install prerequisites

- [Git for Windows](https://git-scm.com/download/win)
- [Python 3.11+](https://www.python.org/downloads/) with **Add Python to PATH**
- [Rust via rustup](https://rustup.rs/) using the default MSVC toolchain
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  with **Desktop development with C++**
- A current NVIDIA Game Ready or Studio Driver for CUDA transcription

Windows 11 already includes WebView2. Verify the GPU driver:

```powershell
nvidia-smi
```

### 2. Clone and prepare the gateway

```powershell
git clone https://github.com/DmitryGrigorov/NovaWhisper.git
cd NovaWhisper\server\gateway

py -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\pip.exe install -r requirements-local.txt
```

`requirements-local.txt` installs faster-whisper plus CUDA 12 cuBLAS and
cuDNN 9 DLLs inside the virtual environment. A separate full CUDA Toolkit is
normally unnecessary; a working NVIDIA driver is still required. The gateway
adds these package-local DLL directories automatically on Windows.

### 3. Download a multilingual model

The model downloads on first startup. If background Hugging Face downloads
stall, pre-download the tested multilingual `small` model:

```powershell
$env:HF_HUB_DISABLE_XET = "1"
.\.venv\Scripts\python.exe -c "from huggingface_hub import snapshot_download; print(snapshot_download('Systran/faster-whisper-small'))"
```

The `small` model supports Russian. `large-v3-turbo` can provide higher
quality but needs more disk space, bandwidth, and startup time.

### 4. Start the CUDA gateway

Keep this PowerShell window open:

```powershell
cd NovaWhisper\server\gateway
$env:HF_HUB_DISABLE_XET = "1"
$env:WHISPR_STT_PROVIDER = "whisper_local"
$env:WHISPR_WHISPER_MODEL = "small"
$env:WHISPR_WHISPER_DEVICE = "cuda"
$env:WHISPR_WHISPER_COMPUTE = "float16"
.\.venv\Scripts\uvicorn.exe app.main:app --host 127.0.0.1 --port 8765
```

Check it from another terminal:

```powershell
Invoke-RestMethod http://127.0.0.1:8765/healthz
```

Expected result: `status : ok`.

### 5. Build and run the desktop app

```powershell
cd NovaWhisper
cargo build --release -p whispr-desktop
.\target\release\whispr-desktop.exe
```

The standalone desktop executable is created at:

```text
NovaWhisper\target\release\whispr-desktop.exe
```

It does not require an installer. You can copy it elsewhere, but the Python
gateway must still be installed and running. The development command is
`cargo run -p whispr-desktop`.

### 6. Use dictation

1. Keep the gateway running.
2. In Settings, use `ws://127.0.0.1:8765/v1/stream`.
3. Select **Russian (Русский)**, another language, or **Auto-detect**.
4. Focus a text field and press the configured hotkey.
5. Speak, then press the hotkey again to finish and insert the text.
6. Drag the recording HUD to move it around the screen.

Configuration is stored in `%APPDATA%\whispr\config.json`.

## CPU fallback

```powershell
$env:WHISPR_STT_PROVIDER = "whisper_local"
$env:WHISPR_WHISPER_MODEL = "small"
$env:WHISPR_WHISPER_DEVICE = "cpu"
$env:WHISPR_WHISPER_COMPUTE = "int8"
```

## Linux quick setup

Install Python, Rust, WebKitGTK 4.1, GTK 3, appindicator, ALSA, OpenSSL,
`libxdo`, and `patchelf`. Then run:

```sh
cd server/gateway
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
./.venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8765

# In another terminal, from the repository root
cargo run -p whispr-desktop
```

Global hotkeys and insertion currently need X11/XWayland on Linux.

## macOS build and DMG

Install the Apple Command Line Tools and Rust, then build the desktop app from
the repository root:

```sh
xcode-select --install
brew install rust
cargo install tauri-cli --version '^2' --locked
cargo test --workspace
cargo tauri build --bundles dmg
```

The Apple Silicon installer is created at:

```text
target/release/bundle/dmg/Whispr_0.1.0_aarch64.dmg
```

The app is not code-signed or notarized yet. On first launch, macOS may require
you to approve it in **System Settings → Privacy & Security**. Dictation also
needs **Microphone** and **Accessibility** permission. As on Windows, keep the
Python gateway running and configure `ws://127.0.0.1:8765/v1/stream`.

## Verification

```powershell
cargo test --workspace
cd server\gateway
.\.venv\Scripts\python.exe -m pytest
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| HUD cannot connect | Start the gateway and verify port 8765 and the configured WebSocket URL. |
| The same canned sentence is inserted | The gateway is using `mock`; set `WHISPR_STT_PROVIDER=whisper_local`. |
| `cublas64_12.dll` or `cudnn64_9.dll` is missing | Re-run `pip install -r requirements-local.txt`, then restart the gateway. |
| CUDA still fails | Check `nvidia-smi`, update the NVIDIA driver, or use the CPU fallback. |
| `link.exe` is missing | Install Visual Studio Build Tools with Desktop development with C++. |
| Microphone is unavailable | Select a Windows default input device and close apps using it exclusively. |
| Hotkey is rejected | Use a combination such as `ctrl+shift+space` or `alt+d`. |

## Repository map

| Path | Purpose |
|---|---|
| `core/` | Rust audio, protocol, VAD, configuration, and streaming client |
| `platform/insert/` | Cross-application text insertion |
| `apps/desktop/` | Tauri desktop app and UI |
| `server/gateway/` | FastAPI gateway, STT providers, and transcript polish |
| `shared/protocol.md` | Client/gateway wire protocol |
| `docs/` | Architecture, structure, and development recipes |

See [Architecture](docs/ARCHITECTURE.md), [Project structure](docs/STRUCTURE.md),
and [development recipes](docs/SKILLS.md).
