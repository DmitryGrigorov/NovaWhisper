# NovaWhisper code and release audit

Audit date: 2026-08-08. Scope: the Rust workspace, Tauri UI/IPC, Python gateway,
provider lifecycle, tests, and native packaging metadata.

## Architecture

The Tauri process captures and resamples microphone input to 16 kHz mono PCM,
streams one utterance per WebSocket, expands snippets, and inserts the final
text. The loopback FastAPI sidecar owns STT and optional LLM polishing. Tauri
commands are available only to the bundled `main` and `hud` webviews. Static UI
files are compiled into the Tauri application; the PyInstaller `onedir` gateway
is copied as a Tauri resource and includes its Python interpreter and packages.

## Findings and disposition

### High

- **Unbounded gateway input (fixed):** a client could send an unlimited
  dictionary, oversized PCM frames, or an indefinitely long utterance. The
  gateway now validates session types and sizes, limits individual frames, and
  caps an utterance at 30 minutes.
- **Capture leak after transport failure (fixed):** connection/protocol errors
  set the tray idle but left the capture thread and active session alive. Error
  and close paths now call the common session finalizer.
- **Zero-sized audio chunks (fixed):** a corrupt/hand-edited config with
  `chunk_ms = 0` caused a non-terminating loop in the audio callback. Capture
  now accepts only 10–1000 ms.
- **Accidental network exposure (fixed):** the frozen gateway CLI accepted
  `--host 0.0.0.0` despite having no authentication. Non-loopback binds now
  require the explicit `--allow-remote` risk acknowledgement. Remote use still
  needs TLS, authentication, and a reverse proxy; it is not a supported secure
  mode by itself.

### Medium

- **Webview policy was disabled (fixed):** the Tauri CSP is now explicit and
  permits only packaged content plus Tauri IPC. Inline CSS/JS remains allowed
  because the current UI is two self-contained HTML files; extracting scripts
  and styles would allow removing `unsafe-inline`.
- **Dependencies are not reproducible (open):** Cargo is locked, but Python
  requirement files use lower bounds. Release builds should generate and
  review per-backend hash-locked constraints (for example with `pip-compile
  --generate-hashes`) and install with `--require-hashes`. ML/CUDA wheels are
  platform-specific, so one universal lock file is inappropriate.
- **Blocking work around mutexes (open):** microphone startup and capture-thread
  joining occur while `DictationState` can be locked. This is bounded today but
  can stall UI commands for seconds. A future state-machine refactor should
  transition under the lock and start/stop outside it.
- **Provider memory scales with utterance length (mitigated):** local providers
  retain PCM for final decoding. The new gateway duration cap bounds this, but
  long recordings still use meaningful RAM. A chunked/temp-file finalization
  design is preferable for meeting transcription.
- **Clipboard fallback is observable (open):** insertion temporarily replaces
  the system clipboard before restoring it. Concurrent clipboard managers can
  observe the text. Native UI Automation/Accessibility/AT-SPI insertion is
  still stubbed.

### Low

- Production `unwrap` use is limited mainly to poisoned mutex handling and the
  required top-level Tauri fatal error. Poison recovery with `into_inner()`
  would improve resilience but does not create memory unsafety.
- VAD is tested but not wired into dictation, so the `auto_stop` setting does
  not currently provide its advertised behavior.
- Deepgram and Anthropic paths require live credentialed integration tests.
- Linux global shortcuts and synthetic insertion depend on compositor/portal
  support; Wayland behavior varies by distribution.
- The base Python test dependency produces a Starlette/httpx compatibility
  deprecation warning. Keep test tooling separate from runtime dependencies
  when locks are introduced.

## Verification

- `cargo test --workspace`: 20 passed before changes.
- Gateway test suite: 19 passed before changes.
- Post-change commands and results belong in the release checklist. Installer
  production must run natively on each target OS; PyInstaller is not a
  cross-compiler.

## Release gates

Run Rust tests, gateway tests, `cargo clippy --workspace --all-targets -- -D
warnings`, frontend smoke tests, the no-microphone WebSocket E2E procedure in
`docs/SKILLS.md`, sidecar `/healthz`, then install/uninstall the generated
package in a clean VM. Sign and notarize only artifacts that passed those gates.
