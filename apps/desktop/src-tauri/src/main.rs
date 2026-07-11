#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dictation;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use dictation::DictationState;
use whispr_core::AppConfig;

#[tauri::command]
fn get_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
fn list_microphones() -> Result<Vec<String>, String> {
    whispr_core::audio::AudioCapture::input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    let previous = AppConfig::load();
    config.save().map_err(|e| e.to_string())?;
    if previous.hotkey != config.hotkey {
        app.global_shortcut()
            .unregister_all()
            .map_err(|e| e.to_string())?;
        register_hotkey(&app, &config.hotkey)?;
    }
    Ok(())
}

#[tauri::command]
fn toggle_dictation(app: AppHandle) -> Result<bool, String> {
    dictation::toggle(&app)
}

/// Settings-page helper: waits so the user can focus a target field, then
/// inserts sample text through the real insertion path.
#[tauri::command]
async fn test_insert(app: AppHandle, text: String, method: String) -> Result<String, String> {
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    dictation::insert_text(&app, text, whispr_insert::InsertMethod::parse(&method))
        .await
        .map(|via| format!("{via:?}"))
}

fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("invalid hotkey {hotkey:?}: {e}"))?;
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| format!("failed to register hotkey {hotkey:?}: {e}"))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        // Capture start can block briefly; keep it off the
                        // input-event thread.
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = dictation::toggle(&app);
                        });
                    }
                })
                .build(),
        )
        .manage(DictationState::default())
        .invoke_handler(tauri::generate_handler![
            get_config,
            list_microphones,
            save_config,
            toggle_dictation,
            test_insert
        ])
        .setup(|app| {
            let handle = app.handle();

            let settings = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App("index.html".into()),
            )
            .title("Whispr")
            .inner_size(600.0, 720.0)
            .visible(true)
            .build()?;

            // Closing settings hides it; the app lives in the tray.
            let settings_handle = settings.clone();
            settings.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = settings_handle.hide();
                }
            });

            WebviewWindowBuilder::new(app, "hud", WebviewUrl::App("hud.html".into()))
                .title("Whispr HUD")
                .inner_size(460.0, 104.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .build()?;

            let toggle_item =
                MenuItem::with_id(app, "toggle", "Start/Stop Dictation", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Whispr", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_item, &settings_item, &quit_item])?;

            TrayIconBuilder::with_id("whispr-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Whispr — voice to text anywhere")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "toggle" => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            let _ = dictation::toggle(&app);
                        });
                    }
                    "settings" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let cfg = AppConfig::load();
            if let Err(e) = register_hotkey(handle, &cfg.hotkey) {
                tracing::warn!("{e}");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Whispr");
}
