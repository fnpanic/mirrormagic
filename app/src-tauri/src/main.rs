#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! MirrorMagic: the Windows PC shares keyboard, mouse, monitor and camera with a Mac.

mod audio;
mod clipboard;
mod commands;
mod config;
mod ddc;
mod mjpeg;
mod status;
mod tray;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod pc;
#[cfg(windows)]
mod win;

use tauri::WindowEvent;

fn main() {
    // release builds have no console: record panics where we can read them
    std::panic::set_hook(Box::new(|info| status::log(format!("PANIC {info}"))));
    status::log(format!("start v{}", env!("CARGO_PKG_VERSION")));
    // networking/camera runtime, independent of Tauri's own
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("mm-net").build().expect("tokio runtime")));
    let hidden = std::env::args().any(|a| a == "--hidden");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| tray::show_main(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::save_config,
            commands::set_autostart,
            commands::set_paused,
            commands::quit,
            commands::new_pairing_key,
            commands::set_input,
            commands::toggle_monitor,
            commands::take_back,
            commands::discover,
            commands::setup_obs,
            commands::camera_now,
            commands::open_privacy,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // menu-bar app, no Dock icon
            status::init(app.handle().clone());
            status::update(|s| s.paused = config::paused());
            tray::create(app)?;
            // runs on the main thread (macOS keyboard-layout APIs require it)
            #[cfg(windows)]
            pc::start(rt.handle());
            #[cfg(target_os = "macos")]
            mac::start(rt.handle());
            let first_run = config::pairing_key().is_none();
            if !hidden || first_run {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|w, e| {
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close(); // keep running in the tray
                let _ = w.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running MirrorMagic")
        .run(|_, e| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Exit = e {
                mac::shutdown();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = e;
        });
}
