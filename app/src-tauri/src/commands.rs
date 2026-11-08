//! Commands for the settings window.
use crate::{config, ddc, status};
use serde_json::{json, Value};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub fn snapshot(app: tauri::AppHandle) -> Value {
    let c = config::get();
    json!({
        "role": if cfg!(windows) { "pc" } else { "mac" },
        "version": env!("CARGO_PKG_VERSION"),
        "config": c,
        "pairingValid": config::pairing_key().is_some(),
        "hotkeyValid": mm_core::keymap::Hotkey::parse(&c.monitor_hotkey).is_some(),
        "autostart": app.autolaunch().is_enabled().unwrap_or(false),
        "status": status::get(),
        "inputs": status::get().ddc.inputs.iter().map(|&v| json!({ "value": v, "name": ddc::input_name(v) })).collect::<Vec<_>>(),
    })
}

/// Merges `patch` (a partial Config) into the settings and applies it.
#[tauri::command]
pub fn save_config(patch: Value) -> Result<(), String> {
    if let Some(h) = patch.get("monitor_hotkey").and_then(Value::as_str) {
        mm_core::keymap::Hotkey::parse(h).ok_or("unknown hotkey")?;
    }
    if let Some(k) = patch.get("pairing_key").and_then(Value::as_str) {
        if !k.trim().is_empty() {
            mm_core::secure::PairingKey::parse(k).ok_or("that is not a valid pairing key (30 characters)")?;
        }
    }
    let mut v = serde_json::to_value(config::get()).map_err(|e| e.to_string())?;
    if let (Some(dst), Some(src)) = (v.as_object_mut(), patch.as_object()) {
        for (k, val) in src {
            dst.insert(k.clone(), val.clone());
        }
    }
    let new: config::Config = serde_json::from_value(v).map_err(|e| e.to_string())?;
    config::update(|c| *c = new);
    #[cfg(windows)]
    crate::pc::reload_config();
    Ok(())
}

/// Pause (no connection, no input sharing) or resume. Shared by the window and the tray.
#[tauri::command]
pub fn set_paused(on: bool) {
    #[cfg(windows)]
    if on {
        crate::pc::release_for_pause(); // input and monitor back to the PC while the Mac still listens
    }
    #[cfg(target_os = "macos")]
    if on {
        crate::mac::monitor_back_for_pause();
    }
    config::update(|c| c.paused = on);
    status::log(if on { "paused" } else { "resumed" });
    status::update(|s| s.paused = on);
}

#[tauri::command]
pub fn quit(app: tauri::AppHandle) {
    #[cfg(windows)]
    crate::win::restore_cursors();
    app.exit(0)
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let a = app.autolaunch();
    if on { a.enable() } else { a.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn new_pairing_key() -> String {
    let k = mm_core::secure::PairingKey::generate().to_display();
    config::update(|c| c.pairing_key = k.clone());
    k
}

#[tauri::command]
pub async fn set_input(value: u16) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || ddc::set_input(value)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn toggle_monitor() {
    #[cfg(windows)]
    crate::pc::toggle_monitor();
}

#[tauri::command]
pub fn take_back() {
    #[cfg(windows)]
    crate::pc::release();
}

#[tauri::command]
pub async fn discover() -> Vec<(String, String)> {
    #[cfg(target_os = "macos")]
    return crate::mac::discover_all().await;
    #[cfg(not(target_os = "macos"))]
    vec![]
}

#[tauri::command]
pub async fn setup_obs() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    return crate::mac::setup_obs(true).await;
    #[cfg(not(target_os = "macos"))]
    Err("macOS only".into())
}

#[tauri::command]
pub fn camera_now(on: bool) {
    #[cfg(target_os = "macos")]
    crate::mac::CAM_MANUAL.store(on, std::sync::atomic::Ordering::SeqCst);
    let _ = on;
}

#[tauri::command]
pub fn open_privacy() {
    #[cfg(target_os = "macos")]
    {
        crate::mac::sys::can_inject(true);
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
    }
}
