//! Small macOS FFI helpers: displays, Accessibility trust, keyboard type/layout, camera-in-use.
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use mm_core::primary::Rect;
use mm_core::proto::DisplayInfo;
use std::ffi::c_void;

// ---------------------------------------------------------------- displays

pub fn displays() -> Vec<DisplayInfo> {
    CGDisplay::active_displays()
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            let d = CGDisplay::new(id);
            let b = d.bounds();
            let builtin = d.is_builtin();
            DisplayInfo {
                name: if builtin { "Built-in display".into() } else { format!("External display {}×{}", d.pixels_wide(), d.pixels_high()) },
                builtin,
                x: b.origin.x,
                y: b.origin.y,
                w: b.size.width,
                h: b.size.height,
            }
        })
        .collect()
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
}

/// All connected displays, including ones currently mirroring another (not in the active list).
fn online_displays() -> Vec<CGDisplay> {
    let mut ids = [0u32; 16];
    let mut n = 0u32;
    if unsafe { CGGetOnlineDisplayList(16, ids.as_mut_ptr(), &mut n) } != 0 {
        return vec![];
    }
    ids[..n as usize].iter().map(|&id| CGDisplay::new(id)).collect()
}

/// Monitor mode: `true` puts the Mac's desktop (windows, menu bar) on the shared monitor at the
/// monitor's own full resolution: the external display becomes the master of a mirror set and
/// the built-in screen mirrors it (so the monitor gets no letterboxing or scaling; the laptop
/// screen shows a scaled copy). `false` restores the extended layout. Session-scoped: a logout
/// restores the user's setting.
pub fn set_mirror(on: bool) -> Result<(), String> {
    use core_graphics::display::{CGConfigureOption, CGDisplay as D};
    let ds = online_displays();
    let Some(builtin) = ds.iter().find(|d| d.is_builtin()) else { return Err("no built-in display (lid closed?)".into()) };
    let external = ds.iter().find(|d| d.id != builtin.id);
    let cfg = builtin.begin_configuration().map_err(|e| format!("display config: {e}"))?;
    let none = D::new(core_graphics::display::kCGNullDirectDisplayID);
    let r = if on {
        let Some(ext) = external else {
            let _ = builtin.cancel_configuration(&cfg);
            return Err("no external display connected".into());
        };
        builtin.configure_display_mirror_of_display(&cfg, ext)
    } else {
        // undo any mirroring among the connected displays
        ds.iter().try_for_each(|d| d.configure_display_mirror_of_display(&cfg, &none))
    };
    if let Err(e) = r {
        let _ = builtin.cancel_configuration(&cfg);
        return Err(format!("mirror: {e}"));
    }
    builtin.complete_configuration(&cfg, CGConfigureOption::ConfigureForSession).map_err(|e| format!("mirror: {e}"))
}

pub fn rect(d: &DisplayInfo) -> Rect {
    Rect { x: d.x, y: d.y, w: d.w, h: d.h }
}

/// (edge display, monitor display): built-in next to the PC; the first external one is the
/// shared monitor. Without a built-in (lid closed) the main display serves both roles.
pub fn layout(ds: &[DisplayInfo]) -> Option<(Rect, Option<Rect>)> {
    let edge = ds.iter().find(|d| d.builtin).or_else(|| ds.first())?;
    let monitor = ds.iter().find(|d| !d.builtin);
    Some((rect(edge), monitor.map(rect)))
}

// ---------------------------------------------------------------- Accessibility

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// `prompt` shows the system "allow Accessibility" dialog when not yet trusted.
pub fn can_inject(prompt: bool) -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let opts = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::from(prompt))]);
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef())
    }
}

// ---------------------------------------------------------------- keyboard

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn LMGetKbdType() -> u8;
    fn KBGetLayoutType(keyboard_type: i16) -> u32;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: CFStringRef) -> CFTypeRef;
    static kTISPropertyInputSourceID: CFStringRef;
}

const KEYBOARD_ISO: u32 = u32::from_be_bytes(*b"ISO ");

pub fn keyboard_is_iso() -> bool {
    unsafe { KBGetLayoutType(LMGetKbdType() as i16) == KEYBOARD_ISO }
}

/// e.g. "com.apple.keylayout.German"
pub fn layout_id() -> String {
    unsafe {
        let src = TISCopyCurrentKeyboardLayoutInputSource();
        if src.is_null() {
            return String::new();
        }
        let id = TISGetInputSourceProperty(src, kTISPropertyInputSourceID);
        let s = if id.is_null() { String::new() } else { CFString::wrap_under_get_rule(id as CFStringRef).to_string() };
        CFRelease(src as CFTypeRef);
        s
    }
}

pub fn hostname() -> String {
    std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".into())
}

// ---------------------------------------------------------------- CoreMediaIO: is a camera in use?

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreMediaIO", kind = "framework")]
extern "C" {
    fn CMIOObjectGetPropertyDataSize(obj: u32, addr: *const Address, qsize: u32, q: *const c_void, size: *mut u32) -> i32;
    fn CMIOObjectGetPropertyData(obj: u32, addr: *const Address, qsize: u32, q: *const c_void, size: u32, used: *mut u32, data: *mut c_void) -> i32;
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
const SYSTEM_OBJECT: u32 = 1;
const GLOBAL: u32 = fourcc(b"glob");
const DEVICES: u32 = fourcc(b"dev#");
const NAME: u32 = fourcc(b"lnam");
const RUNNING_SOMEWHERE: u32 = fourcc(b"gone");

fn prop<T: Copy + Default>(obj: u32, selector: u32) -> Option<T> {
    let a = Address { selector, scope: GLOBAL, element: 0 };
    let mut v = T::default();
    let mut used = 0u32;
    let r = unsafe { CMIOObjectGetPropertyData(obj, &a, 0, std::ptr::null(), std::mem::size_of::<T>() as u32, &mut used, &mut v as *mut T as *mut c_void) };
    (r == 0).then_some(v)
}

/// Camera devices as (name, in use by some app).
pub fn cameras() -> Vec<(String, bool)> {
    unsafe {
        let a = Address { selector: DEVICES, scope: GLOBAL, element: 0 };
        let mut size = 0u32;
        if CMIOObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size) != 0 {
            return vec![];
        }
        let mut ids = vec![0u32; size as usize / 4];
        let mut used = 0u32;
        if CMIOObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), size, &mut used, ids.as_mut_ptr() as *mut c_void) != 0 {
            return vec![];
        }
        ids.into_iter()
            .map(|id| {
                let name = prop::<usize>(id, NAME)
                    .filter(|p| *p != 0)
                    .map(|p| CFString::wrap_under_create_rule(p as CFStringRef).to_string())
                    .unwrap_or_default();
                let running = prop::<u32>(id, RUNNING_SOMEWHERE).unwrap_or(0) != 0;
                (name, running)
            })
            .collect()
    }
}
