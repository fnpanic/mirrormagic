//! Commands for the settings window.
use crate::{config, status};
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
        "autostart": app.autolaunch().is_enabled().unwrap_or(false),
        "status": status::get(),
        "notice": config::load_error(),
    })
}

/// Merges `patch` (a partial Config) into the settings and applies it.
#[tauri::command]
pub fn save_config(patch: Value) -> Result<(), String> {
    let has = |k: &str| patch.get(k).is_some();
    // state the backend keeps for itself, and (PC) the key, which only "New key" replaces
    if ["mirrored", "hdmi_off", "paused", "obs_password", "pc_input"].iter().any(|k| has(k)) || (cfg!(windows) && has("pairing_key")) {
        return Err("that setting cannot be changed here".into());
    }
    if let Some(h) = patch.get("monitor_hotkey").and_then(Value::as_str) {
        let h = mm_core::keymap::Hotkey::parse(h).ok_or("unknown hotkey")?;
        if !h.is_usable() {
            return Err("a hotkey needs Ctrl, Alt or Win (or is a function key)".into());
        }
    }
    if let Some(k) = patch.get("pairing_key").and_then(Value::as_str) {
        if !k.trim().is_empty() {
            mm_core::secure::PairingKey::parse(k).ok_or("that is not a valid pairing key (30 characters)")?;
        }
    }
    if patch.get("speed").is_some_and(|s| !s.as_f64().is_some_and(|s| (0.25..=4.0).contains(&s))) {
        return Err("pointer speed must be between 0.25 and 4".into());
    }
    // merged under the config lock: a value another thread saves meanwhile (learned PC input, HDMI state) survives
    let mut err = None;
    config::update(|c| {
        let merged = serde_json::to_value(&*c).and_then(|mut v| {
            if let (Some(dst), Some(src)) = (v.as_object_mut(), patch.as_object()) {
                for (k, val) in src {
                    dst.insert(k.clone(), val.clone());
                }
            }
            serde_json::from_value::<config::Config>(v)
        });
        match merged {
            Ok(new) => *c = new,
            Err(e) => err = Some(e.to_string()),
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    // applied even when the file could not be written: the running app already has the new values
    let saved = config::take_error();
    #[cfg(windows)]
    {
        crate::pc::reload_config();
        if has("mic") || has("camera") {
            crate::pc::sharing_changed(has("mic"), has("camera")); // a running stream stops (or starts) with the switch
        }
        if has("ddc_monitor") {
            std::thread::spawn(crate::pc::probe_ddc);
        }
    }
    #[cfg(target_os = "macos")]
    crate::mac::refresh(); // speed, scrolling and key options apply now, not at the next crossing
    saved.map_or(Ok(()), Err)
}

/// While the window records a new hotkey, the current one must not fire.
#[tauri::command]
pub fn hotkey_recording(on: bool) {
    #[cfg(windows)]
    crate::pc::hotkey_recording(on);
    let _ = on;
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
    crate::pc::shutdown();
    app.exit(0)
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    let a = app.autolaunch();
    if on { a.enable() } else { a.disable() }.map_err(|e| e.to_string())
}

/// A new key locks the old one out: running connections end too.
#[tauri::command]
pub fn new_pairing_key() -> String {
    let k = mm_core::secure::PairingKey::generate().to_display();
    config::update(|c| c.pairing_key = k.clone());
    #[cfg(windows)]
    crate::pc::revoke_sessions();
    status::log("new pairing key: connections with the old key ended");
    k
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
    return crate::mac::setup_obs(true, true).await; // the button may restart an OBS without remote control
    #[cfg(not(target_os = "macos"))]
    Err("macOS only".into())
}

#[tauri::command]
pub fn camera_now(on: bool) {
    #[cfg(target_os = "macos")]
    crate::mac::CAM_MANUAL.store(on, std::sync::atomic::Ordering::SeqCst);
    let _ = on;
}

/// Screen & System Audio Recording: apps reading a "– PC" microphone must be listed under
/// "System Audio Recording Only", else macOS hands them silence.
#[tauri::command]
pub fn open_audio_privacy() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture").spawn();
}

#[tauri::command]
pub fn open_privacy() {
    #[cfg(target_os = "macos")]
    {
        crate::mac::sys::can_inject(true);
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
    }
}
