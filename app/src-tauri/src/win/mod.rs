//! Windows platform layer: hooks, cursor, monitors, webcam.
pub mod camera;
pub mod hook;

use mm_core::primary::Rect;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// All monitors in physical pixels (the process is per-monitor DPI aware via Tauri's manifest).
pub fn monitors() -> Vec<Rect> {
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut info) != 0 {
            let r = info.rcMonitor;
            (*(data as *mut Vec<Rect>)).push(Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 });
        }
        TRUE
    }
    let mut v: Vec<Rect> = Vec::new();
    unsafe { EnumDisplayMonitors(null_mut(), std::ptr::null(), Some(cb), &mut v as *mut _ as LPARAM) };
    v
}

pub fn warp(x: f64, y: f64) {
    unsafe { SetCursorPos(x.round() as i32, y.round() as i32) };
}

const CURSORS: [u32; 13] = [
    OCR_NORMAL, OCR_IBEAM, OCR_WAIT, OCR_CROSS, OCR_UP, OCR_SIZENWSE, OCR_SIZENESW, OCR_SIZEWE, OCR_SIZENS, OCR_SIZEALL, OCR_NO, OCR_HAND, OCR_APPSTARTING,
];

/// Hide the pointer system-wide while the Mac has control (it is parked in the screen centre).
/// SPI_SETCURSORS reloads the user's scheme, so a crash is healed by the next start (see `restore_cursors`).
pub fn hide_cursor(hide: bool) {
    unsafe {
        if hide {
            let and = [0xFFu8; 128];
            let xor = [0u8; 128];
            let blank = CreateCursor(null_mut(), 0, 0, 32, 32, and.as_ptr() as _, xor.as_ptr() as _);
            if blank.is_null() {
                return;
            }
            for id in CURSORS {
                SetSystemCursor(CopyIcon(blank), id); // takes ownership of the copy
            }
            DestroyCursor(blank);
        } else {
            restore_cursors();
        }
    }
}

/// Hide/show requests come from the hook thread and the network thread; one worker applies
/// the latest wanted state, so a quick enter/leave can never end with a hidden cursor.
pub fn set_cursor_hidden(hide: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};
    static WANT: AtomicBool = AtomicBool::new(false);
    static TX: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();
    WANT.store(hide, Ordering::SeqCst);
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let mut hidden = false;
            while rx.recv().is_ok() {
                let want = WANT.load(Ordering::SeqCst);
                if want != hidden {
                    hide_cursor(want);
                    hidden = want;
                }
            }
        });
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(());
}

pub fn cursor_pos() -> Option<(f64, f64)> {
    let mut p = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f64, p.y as f64))
}

/// Is the key with this scan code physically down right now?
pub fn key_down(scan: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, MapVirtualKeyW, MAPVK_VSC_TO_VK_EX};
    let code = if scan & 0xE000 == 0xE000 { 0xE000 | (scan & 0xFF) as u32 } else { scan as u32 };
    let vk = unsafe { MapVirtualKeyW(code, MAPVK_VSC_TO_VK_EX) };
    // unknown keys (e.g. the fake AltGr Ctrl) can't be queried: treat as released
    vk != 0 && unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0
}

pub fn button_down(b: mm_core::proto::Button) -> bool {
    use mm_core::proto::Button;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2};
    let vk = match b {
        Button::Left => VK_LBUTTON,
        Button::Right => VK_RBUTTON,
        Button::Middle => VK_MBUTTON,
        Button::Back => VK_XBUTTON1,
        Button::Forward => VK_XBUTTON2,
    };
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

pub fn restore_cursors() {
    unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0) };
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}
