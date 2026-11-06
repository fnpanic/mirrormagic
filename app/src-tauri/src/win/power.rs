//! Windows display power events (GUID_CONSOLE_DISPLAY_STATE): 0 = off, 1 = on, 2 = dimmed.
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

static CALLBACK: OnceLock<fn(u32)> = OnceLock::new();

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // only GUID_CONSOLE_DISPLAY_STATE is registered, so every setting change is a display state
    if m == WM_POWERBROADCAST && w as u32 == PBT_POWERSETTINGCHANGE && l != 0 {
        let s = &*(l as *const POWERBROADCAST_SETTING);
        if s.DataLength >= 4 {
            let state = std::ptr::read_unaligned(s.Data.as_ptr() as *const u32);
            if let Some(f) = CALLBACK.get() {
                f(state);
            }
        }
        return 1;
    }
    DefWindowProcW(h, m, w, l)
}

/// Calls `f(state)` on every display state change, on a thread of its own with a window that is
/// never shown (power notifications need one). Windows sends the current state right away.
/// `f` runs on the message loop: it must return quickly.
pub fn watch_display(f: fn(u32)) {
    let _ = CALLBACK.set(f);
    std::thread::Builder::new()
        .name("display-power".into())
        .spawn(|| unsafe {
            let class: Vec<u16> = "MirrorMagicDisplayPower\0".encode_utf16().collect();
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), wc.hInstance, std::ptr::null());
            if hwnd.is_null() || RegisterPowerSettingNotification(hwnd, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE) == 0 {
                crate::status::log("display power notifications unavailable");
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        })
        .unwrap();
}
