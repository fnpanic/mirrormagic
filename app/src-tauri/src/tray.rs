//! Tray / menu-bar icon: status line, monitor switch, open, quit.
use crate::status::Status;
use std::sync::OnceLock;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

static STATUS_ITEM: OnceLock<MenuItem<Wry>> = OnceLock::new();
static SWITCH_ITEM: OnceLock<MenuItem<Wry>> = OnceLock::new();

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn create(app: &tauri::App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Starting…", false, None::<&str>)?;
    let switch = MenuItem::with_id(app, "switch", "Show the Mac on the monitor", cfg!(windows), None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open MirrorMagic", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = if cfg!(windows) { Menu::with_items(app, &[&status, &sep, &switch, &open, &quit])? } else { Menu::with_items(app, &[&status, &sep, &open, &quit])? };
    let _ = STATUS_ITEM.set(status);
    let _ = SWITCH_ITEM.set(switch);
    // macOS menu bar wants a monochrome template image; Windows shows the colour app icon
    let icon = if cfg!(target_os = "macos") {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?
    } else {
        app.default_window_icon().cloned().expect("bundle icon")
    };
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("MirrorMagic")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "quit" => {
                #[cfg(windows)]
                crate::win::restore_cursors();
                app.exit(0)
            }
            #[cfg(windows)]
            "switch" => crate::pc::toggle_monitor(),
            _ => {}
        })
        .on_tray_icon_event(|t, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                if cfg!(windows) {
                    show_main(t.app_handle());
                }
            }
        })
        .build(app)?;
    Ok(())
}

pub fn line(s: &Status) -> String {
    if !s.connected {
        return if cfg!(windows) { "Waiting for the Mac".into() } else { "Looking for the PC".into() };
    }
    let peer = if s.peer.is_empty() { "peer" } else { s.peer.as_str() };
    if cfg!(windows) && !s.can_inject {
        return format!("{peer} connected: needs Accessibility");
    }
    match s.control.as_str() {
        "edge" => format!("Controlling {peer}"),
        "monitor" => format!("Monitor shows {}", if cfg!(windows) { peer } else { "this Mac" }),
        _ => format!("Connected to {peer}"),
    }
}

pub fn refresh(app: &AppHandle, s: &Status) {
    let text = line(s);
    if let Some(i) = STATUS_ITEM.get() {
        let _ = i.set_text(&text);
    }
    if let Some(i) = SWITCH_ITEM.get() {
        let _ = i.set_text(if s.monitor_on_mac { "Show the PC on the monitor" } else { "Show the Mac on the monitor" });
    }
    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_tooltip(Some(format!("MirrorMagic · {text}")));
    }
}
