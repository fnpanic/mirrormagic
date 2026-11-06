//! Global low-level keyboard/mouse hooks. Events are fed to the PC state machine (crate::pc),
//! which decides per event whether Windows still sees it (return 1 = swallowed).
//! The callbacks must return fast: Windows silently removes a hook that times out.
use mm_core::proto::Button;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC, VK_PAUSE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// A panic must never unwind out of a hook callback: Rust would abort the whole app without a
/// trace. Log it and let the event through instead.
fn guarded(f: impl FnOnce() -> bool) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        crate::status::log("input hook: recovered from a panic (event passed through)");
        false
    })
}

unsafe extern "system" fn keyboard_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(l as *const KBDLLHOOKSTRUCT);
        if k.flags & LLKHF_INJECTED == 0 {
            let down = matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let mut scan = if k.scanCode == 0 { MapVirtualKeyW(k.vkCode, MAPVK_VK_TO_VSC) } else { k.scanCode } as u16;
            if k.vkCode == VK_PAUSE as u32 {
                scan = 0xE045;
            } else if k.flags & LLKHF_EXTENDED != 0 && !matches!(scan, 0x36 | 0x45) {
                // Right Shift and Num Lock report the extended bit but have their own codes
                scan |= 0xE000;
            }
            if guarded(|| crate::pc::on_key(scan, down)) {
                return 1;
            }
        }
    }
    CallNextHookEx(null_mut(), code, w, l)
}

unsafe extern "system" fn mouse_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let m = &*(l as *const MSLLHOOKSTRUCT);
        if m.flags & LLMHF_INJECTED == 0 {
            let hi = (m.mouseData >> 16) as u16;
            let block = guarded(|| match w as u32 {
                WM_MOUSEMOVE => crate::pc::on_move(m.pt.x as f64, m.pt.y as f64),
                WM_LBUTTONDOWN => crate::pc::on_button(Button::Left, true),
                WM_LBUTTONUP => crate::pc::on_button(Button::Left, false),
                WM_RBUTTONDOWN => crate::pc::on_button(Button::Right, true),
                WM_RBUTTONUP => crate::pc::on_button(Button::Right, false),
                WM_MBUTTONDOWN => crate::pc::on_button(Button::Middle, true),
                WM_MBUTTONUP => crate::pc::on_button(Button::Middle, false),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let b = if hi == XBUTTON1 { Button::Back } else { Button::Forward };
                    crate::pc::on_button(b, w as u32 == WM_XBUTTONDOWN)
                }
                WM_MOUSEWHEEL => crate::pc::on_wheel(0, hi as i16 as i32),
                WM_MOUSEHWHEEL => crate::pc::on_wheel(hi as i16 as i32, 0),
                _ => false,
            });
            if block {
                return 1;
            }
        }
    }
    CallNextHookEx(null_mut(), code, w, l)
}

/// Installs both hooks and pumps messages on the calling thread (never returns normally).
pub fn run() {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
    unsafe {
        // Windows silently drops a low-level hook whose callback is late (e.g. under heavy CPU
        // load): run at top priority, and re-install both hooks every 2 s so a dropped hook heals.
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let module = GetModuleHandleW(std::ptr::null());
        let install = || (SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0), SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0));
        let (mut kb, mut ms) = install();
        if kb.is_null() || ms.is_null() {
            crate::status::update(|s| s.last_error = "could not install the input hooks".into());
            return;
        }
        SetTimer(null_mut(), 0, 2000, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_TIMER {
                // unhook first: two live copies would process every event twice
                UnhookWindowsHookEx(kb);
                UnhookWindowsHookEx(ms);
                (kb, ms) = install();
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(kb);
        UnhookWindowsHookEx(ms);
    }
}
