//! Managed local gateway: start the Python STT gateway with the app and stop
//! it when the app exits, so users of packaged builds never touch Python.
//!
//! Behavior:
//! - Only manages loopback gateways (`gateway_url` host 127.0.0.1/localhost/::1)
//!   and only when `AppConfig.manage_gateway` is true.
//! - If something already answers `/healthz` on the port, it is treated as an
//!   externally started gateway: used as-is, never killed.
//! - Launch discovery order: `WHISPR_GATEWAY_BIN` env override → bundled
//!   `whispr-gateway` binary next to the app executable or in the Tauri
//!   resource dir (`gateway/…`, PyInstaller onedir layout supported) → repo
//!   checkout (`server/gateway` + its `.venv`) → `python`/`python3` on PATH.
//! - Gateway stdout/stderr go to `config_dir/whispr/gateway.log` (truncated on
//!   each spawn).

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use whispr_core::AppConfig;

use crate::dictation::EVENT;

/// Total time we wait for `/healthz` after spawning. The first run of a large
/// Whisper model downloads several GB, so this is deliberately generous.
const READY_TIMEOUT_S: u64 = 600;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Default)]
pub struct GatewayState {
    child: Mutex<Option<Child>>,
    /// Serializes ensure() runs (app start, config save, tray restart).
    ensure_lock: tokio::sync::Mutex<()>,
}

enum Launch {
    /// Self-contained gateway executable (PyInstaller sidecar).
    Binary(PathBuf),
    /// `python -m uvicorn app.main:app` inside a gateway source checkout.
    Uvicorn {
        python: PathBuf,
        gateway_dir: PathBuf,
    },
}

impl Launch {
    fn describe(&self) -> String {
        match self {
            Launch::Binary(p) => format!("bundled gateway {}", p.display()),
            Launch::Uvicorn {
                python,
                gateway_dir,
            } => {
                format!(
                    "{} -m uvicorn in {}",
                    python.display(),
                    gateway_dir.display()
                )
            }
        }
    }
}

fn emit(app: &AppHandle, state: &str, message: String) {
    tracing::info!("gateway: {message}");
    let _ = app.emit(
        EVENT,
        json!({"kind": "gateway", "state": state, "message": message}),
    );
}

/// Start the managed gateway in the background if needed.
pub fn ensure(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<GatewayState>();
        let _guard = state.ensure_lock.lock().await;
        ensure_inner(&app).await;
    });
}

/// Stop the managed gateway (if we own one) and start it again with the
/// current config. Used after settings changes and from the tray menu.
pub fn restart(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<GatewayState>();
        let _guard = state.ensure_lock.lock().await;
        if shutdown(&app) {
            emit(&app, "stopped", "managed gateway stopped".into());
        }
        ensure_inner(&app).await;
    });
}

/// Kill the gateway we spawned, if any. Never touches an external gateway.
/// Returns true when a managed process was actually stopped. Synchronous so
/// it can run in the `RunEvent::Exit` handler.
pub fn shutdown(app: &AppHandle) -> bool {
    let state = app.state::<GatewayState>();
    let child = state.child.lock().unwrap().take();
    let Some(mut child) = child else {
        return false;
    };
    kill_tree(&mut child);
    let _ = child.wait();
    tracing::info!("gateway: managed process stopped");
    true
}

/// Notify UI consumers after an explicit Settings-page stop.
pub fn emit_stopped(app: &AppHandle) {
    emit(app, "stopped", "managed gateway stopped".into());
}

/// Kill the process and any children (PyInstaller bootloaders and uvicorn can
/// have their own subprocesses).
fn kill_tree(child: &mut Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(unix)]
    {
        // The child was spawned as its own process group leader.
        let _ = Command::new("kill")
            .args(["-9", "--", &format!("-{pid}")])
            .status();
    }
    let _ = child.kill();
}

async fn ensure_inner(app: &AppHandle) {
    let cfg = AppConfig::load();
    if !cfg.manage_gateway {
        return;
    }
    let Some((host, port)) = endpoint(&cfg.gateway_url) else {
        emit(
            app,
            "error",
            format!(
                "cannot parse gateway URL {:?}; not managing the gateway",
                cfg.gateway_url
            ),
        );
        return;
    };
    if !is_loopback(&host) {
        tracing::info!("gateway: {host} is not loopback; not managing the gateway");
        return;
    }

    // Already running and owned by us?
    {
        let state = app.state::<GatewayState>();
        let mut guard = state.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => return, // still running
                _ => {
                    guard.take();
                }
            }
        }
    }

    if healthz(&host, port).await {
        emit(
            app,
            "external",
            format!("gateway already running on {host}:{port} — using it (it will not be stopped on quit)"),
        );
        return;
    }

    let Some(launch) = find_launch(app) else {
        emit(
            app,
            "error",
            "no gateway found: bundle whispr-gateway next to the app (scripts/build-gateway), \
             keep the repo's server/gateway/.venv, or start the gateway manually"
                .into(),
        );
        return;
    };

    emit(
        app,
        "starting",
        format!("starting gateway on {host}:{port} ({})", launch.describe()),
    );
    let child = match spawn_gateway(&launch, &host, port, &cfg) {
        Ok(child) => child,
        Err(e) => {
            emit(app, "error", format!("failed to start gateway: {e}"));
            return;
        }
    };
    app.state::<GatewayState>()
        .child
        .lock()
        .unwrap()
        .replace(child);

    // Wait for /healthz. Model download/load happens before uvicorn serves.
    for elapsed in 1..=READY_TIMEOUT_S {
        tokio::time::sleep(Duration::from_secs(1)).await;
        {
            let state = app.state::<GatewayState>();
            let mut guard = state.child.lock().unwrap();
            match guard.as_mut() {
                None => return, // shut down while we were waiting
                Some(child) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        guard.take();
                        drop(guard);
                        emit(
                            app,
                            "error",
                            format!(
                                "gateway exited during startup ({status}); see {}",
                                log_path_display()
                            ),
                        );
                        return;
                    }
                }
            }
        }
        if healthz(&host, port).await {
            emit(app, "ready", format!("gateway ready on {host}:{port}"));
            return;
        }
        if elapsed == 20 {
            emit(
                app,
                "starting",
                "gateway still starting — the first run downloads the Whisper model, which can take a while"
                    .into(),
            );
        }
    }
    emit(
        app,
        "error",
        format!(
            "gateway did not become ready within {READY_TIMEOUT_S}s; see {}",
            log_path_display()
        ),
    );
}

fn spawn_gateway(
    launch: &Launch,
    host: &str,
    port: u16,
    cfg: &AppConfig,
) -> std::io::Result<Child> {
    let port_s = port.to_string();
    let mut cmd = match launch {
        Launch::Binary(path) => {
            #[cfg(unix)]
            {
                make_executable(path);
                let mut cmd = Command::new(path);
                cmd.args(["--host", host, "--port", &port_s]);
                cmd
            }
            #[cfg(windows)]
            {
                let mut cmd = powershell_command(&format!(
                    "& {} --host {} --port {}",
                    powershell_literal(path),
                    powershell_literal(host),
                    powershell_literal(&port_s),
                ));
                if let Some(parent) = path.parent() {
                    cmd.current_dir(parent);
                }
                cmd
            }
        }
        Launch::Uvicorn {
            python,
            gateway_dir,
        } => {
            #[cfg(unix)]
            {
                let mut cmd = Command::new(python);
                cmd.current_dir(gateway_dir).args([
                    "-m",
                    "uvicorn",
                    "app.main:app",
                    "--host",
                    host,
                    "--port",
                    &port_s,
                    "--log-level",
                    "info",
                ]);
                cmd
            }
            #[cfg(windows)]
            {
                let mut cmd = powershell_command(&format!(
                    "& {} -m uvicorn app.main:app --host {} --port {} --log-level info",
                    powershell_literal(python),
                    powershell_literal(host),
                    powershell_literal(&port_s),
                ));
                cmd.current_dir(gateway_dir);
                cmd
            }
        }
    };

    // Hugging Face Xet downloads are known to stall on some Windows setups.
    cmd.env("HF_HUB_DISABLE_XET", "1");
    if !cfg.stt_provider.is_empty() && cfg.stt_provider != "auto" {
        cmd.env("WHISPR_STT_PROVIDER", &cfg.stt_provider);
    }
    if !cfg.whisper_model.is_empty() {
        cmd.env("WHISPR_WHISPER_MODEL", &cfg.whisper_model);
    }
    if !cfg.whisper_device.is_empty() {
        cmd.env("WHISPR_WHISPER_DEVICE", &cfg.whisper_device);
    }
    if !cfg.whisper_compute.is_empty() {
        cmd.env("WHISPR_WHISPER_COMPUTE", &cfg.whisper_compute);
    }

    cmd.stdin(Stdio::null());
    match log_file() {
        Some((out, err)) => {
            cmd.stdout(out).stderr(err);
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
}

#[cfg(windows)]
fn powershell_command(script: &str) -> Command {
    let mut cmd = Command::new("powershell.exe");
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    cmd
}

#[cfg(windows)]
fn powershell_literal(value: impl AsRef<std::ffi::OsStr>) -> String {
    let value = value.as_ref().to_string_lossy();
    format!("'{}'", value.replace('\'', "''"))
}

fn log_path() -> Option<PathBuf> {
    Some(AppConfig::path().ok()?.parent()?.join("gateway.log"))
}

fn log_path_display() -> String {
    log_path().map_or_else(|| "the gateway log".into(), |p| p.display().to_string())
}

fn log_file() -> Option<(Stdio, Stdio)> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .ok()?;
    let clone = file.try_clone().ok()?;
    Some((Stdio::from(file), Stdio::from(clone)))
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.mode() & 0o111 == 0 {
            perms.set_mode(perms.mode() | 0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

fn find_launch(app: &AppHandle) -> Option<Launch> {
    // 1. Explicit override for power users and tests.
    if let Ok(bin) = std::env::var("WHISPR_GATEWAY_BIN") {
        let path = PathBuf::from(bin);
        if path.is_file() {
            return Some(Launch::Binary(path));
        }
    }

    // 2. Bundled sidecar binary (portable zip: next to the exe; installed
    //    bundle: in the resource dir).
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        roots.push(res);
    }
    for root in &roots {
        for candidate in gateway_binary_candidates(root) {
            if candidate.is_file() {
                return Some(Launch::Binary(candidate));
            }
        }
    }

    // 3. Repo checkout (dev): walk up from the exe and the cwd.
    let mut starts = roots;
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    let gateway_dir = find_repo_gateway(&starts)?;
    Some(Launch::Uvicorn {
        python: python_for(&gateway_dir),
        gateway_dir,
    })
}

fn gateway_binary_candidates(dir: &Path) -> [PathBuf; 4] {
    let exe = if cfg!(windows) {
        "whispr-gateway.exe"
    } else {
        "whispr-gateway"
    };
    [
        dir.join(exe),
        dir.join("whispr-gateway").join(exe), // PyInstaller onedir
        dir.join("gateway").join(exe),
        dir.join("gateway").join("whispr-gateway").join(exe), // bundled resources
    ]
}

fn find_repo_gateway(starts: &[PathBuf]) -> Option<PathBuf> {
    for start in starts {
        let mut dir: Option<&Path> = Some(start.as_path());
        for _ in 0..8 {
            let d = dir?;
            let gateway = d.join("server").join("gateway");
            if gateway.join("app").join("main.py").is_file() {
                return Some(gateway);
            }
            dir = d.parent();
        }
    }
    None
}

/// Prefer the gateway's own venv; fall back to a PATH interpreter (spawn
/// errors surface in the event log).
fn python_for(gateway_dir: &Path) -> PathBuf {
    let venv = if cfg!(windows) {
        gateway_dir.join(".venv").join("Scripts").join("python.exe")
    } else {
        gateway_dir.join(".venv").join("bin").join("python")
    };
    if venv.is_file() {
        venv
    } else if cfg!(windows) {
        PathBuf::from("python")
    } else {
        PathBuf::from("python3")
    }
}

/// Extract (host, port) from a ws:// or http:// URL.
fn endpoint(url: &str) -> Option<(String, u16)> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split('/').next()?;
    if authority.is_empty() {
        return None;
    }
    match authority.rfind(':') {
        Some(idx) if authority[idx + 1..].chars().all(|c| c.is_ascii_digit()) => {
            let host = authority[..idx]
                .trim_start_matches('[')
                .trim_end_matches(']');
            let port: u16 = authority[idx + 1..].parse().ok()?;
            Some((host.to_string(), port))
        }
        _ => Some((
            authority
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
            80,
        )),
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Minimal HTTP GET /healthz — avoids pulling an HTTP client into the app.
async fn healthz(host: &str, port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let addr = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let connect = tokio::net::TcpStream::connect(&addr);
    let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(2), connect).await else {
        return false;
    };
    let request = format!("GET /healthz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut response = Vec::new();
    let read = stream.read_to_end(&mut response);
    let _ = tokio::time::timeout(Duration::from_secs(3), read).await;
    let head = String::from_utf8_lossy(&response);
    head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parses_default_gateway_url() {
        assert_eq!(
            endpoint("ws://127.0.0.1:8765/v1/stream"),
            Some(("127.0.0.1".into(), 8765))
        );
    }

    #[test]
    fn endpoint_parses_hosts_without_port_and_ipv6() {
        assert_eq!(
            endpoint("ws://localhost/v1/stream"),
            Some(("localhost".into(), 80))
        );
        assert_eq!(
            endpoint("ws://[::1]:8765/v1/stream"),
            Some(("::1".into(), 8765))
        );
        assert_eq!(endpoint("not a url"), None);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("localhost"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("192.168.1.20"));
        assert!(!is_loopback("example.com"));
    }
}
