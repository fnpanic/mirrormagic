//! Posts mouse and keyboard events into the macOS HID event stream (needs Accessibility).
use core_graphics::display::CGDisplay;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use mm_core::keymap::{KeyTranslator, MacFlags};
use mm_core::proto::Button;
use std::time::{Duration, Instant};

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const LINE_UNITS: u32 = 1;

pub struct Injector {
    keys: KeyTranslator,
    flags: MacFlags,
    /// held buttons: left, right, middle, back, forward
    held: [bool; 5],
    last_click: Option<(Button, Instant, CGPoint)>,
    clicks: i64,
    scroll_rest: (i32, i32),
    pub invert_scroll: bool,
}

fn source() -> Option<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()
}

pub fn cursor() -> Option<(f64, f64)> {
    let p = CGEvent::new(source()?).ok()?.location();
    Some((p.x, p.y))
}

impl Injector {
    pub fn new(iso: bool, german: bool) -> Self {
        Injector { keys: KeyTranslator::new(iso, german), flags: MacFlags::default(), held: [false; 5], last_click: None, clicks: 1, scroll_rest: (0, 0), invert_scroll: false }
    }

    fn flags(&self) -> CGEventFlags {
        CGEventFlags::from_bits_truncate(self.flags.bits())
    }

    fn post_mouse(&self, ty: CGEventType, p: CGPoint, button: CGMouseButton, number: i64, dx: f64, dy: f64) {
        let Some(src) = source() else { return };
        if let Ok(e) = CGEvent::new_mouse_event(src, ty, p, button) {
            e.set_flags(self.flags());
            if number > 1 {
                e.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, number);
            }
            e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx.round() as i64);
            e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy.round() as i64);
            if matches!(ty, CGEventType::LeftMouseDown | CGEventType::LeftMouseUp | CGEventType::RightMouseDown | CGEventType::RightMouseUp | CGEventType::OtherMouseDown | CGEventType::OtherMouseUp) {
                e.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, self.clicks);
            }
            e.post(CGEventTapLocation::HID);
        }
    }

    /// Jump (enter / warp) without deltas.
    pub fn warp(&self, x: f64, y: f64) {
        let p = CGPoint::new(x, y);
        let _ = CGDisplay::warp_mouse_cursor_position(p);
        let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(true);
        self.move_to(x, y, 0.0, 0.0); // so hover state updates under the new position
    }

    pub fn move_to(&self, x: f64, y: f64, dx: f64, dy: f64) {
        let p = CGPoint::new(x, y);
        let (ty, b) = if self.held[0] {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if self.held[1] {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else if self.held[2..].iter().any(|h| *h) {
            (CGEventType::OtherMouseDragged, CGMouseButton::Center)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left)
        };
        self.post_mouse(ty, p, b, 0, dx, dy);
    }

    pub fn button(&mut self, button: Button, down: bool) {
        let Some((x, y)) = cursor() else { return };
        let p = CGPoint::new(x, y);
        if down {
            let now = Instant::now();
            self.clicks = match self.last_click {
                Some((b, t, q)) if b == button && now - t < DOUBLE_CLICK && (q.x - x).abs() < 5.0 && (q.y - y).abs() < 5.0 => self.clicks + 1,
                _ => 1,
            };
            self.last_click = Some((button, now, p));
        }
        let (ty, cg, slot, number) = match (button, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left, 0, 0),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left, 0, 0),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right, 1, 1),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right, 1, 1),
            (b, d) => {
                let n = match b {
                    Button::Middle => 2,
                    Button::Back => 3,
                    _ => 4,
                };
                (if d { CGEventType::OtherMouseDown } else { CGEventType::OtherMouseUp }, CGMouseButton::Center, n as usize, n)
            }
        };
        self.held[slot] = down;
        self.post_mouse(ty, p, cg, number, 0.0, 0.0);
    }

    /// Windows wheel units (120/notch) → 3 lines per notch, remainders kept for smooth wheels.
    pub fn wheel(&mut self, dx: i32, dy: i32) {
        let s = if self.invert_scroll { -1 } else { 1 };
        let (ax, ay) = (self.scroll_rest.0 + dx * 3 * s, self.scroll_rest.1 + dy * 3 * s);
        let (lx, ly) = (ax / 120, ay / 120);
        self.scroll_rest = (ax % 120, ay % 120);
        if lx == 0 && ly == 0 {
            return;
        }
        let Some(src) = source() else { return };
        // Windows: positive dx = right; macOS wheel2: positive = left
        if let Ok(e) = CGEvent::new_scroll_event(src, LINE_UNITS, 2, ly, -lx, 0) {
            e.set_flags(self.flags());
            e.post(CGEventTapLocation::HID);
        }
    }

    pub fn key(&mut self, scan: u16, down: bool) {
        for (code, down) in self.keys.translate(scan, down) {
            self.post_key(code, down);
        }
    }

    fn post_key(&mut self, code: u16, down: bool) {
        if MacFlags::is_modifier(code) {
            self.flags.update(code, down);
        }
        let Some(src) = source() else { return };
        if let Ok(e) = CGEvent::new_keyboard_event(src, code, down) {
            if MacFlags::is_modifier(code) {
                e.set_type(CGEventType::FlagsChanged);
            }
            e.set_flags(self.flags());
            e.post(CGEventTapLocation::HID);
        }
    }

    /// Releases every key and button we pressed (Exit, ReleaseAll, disconnect).
    pub fn release_all(&mut self) {
        for (code, down) in self.keys.release_all() {
            self.post_key(code, down);
        }
        for (i, b) in [Button::Left, Button::Right, Button::Middle, Button::Back, Button::Forward].into_iter().enumerate() {
            if self.held[i] {
                self.button(b, false);
            }
        }
    }

    pub fn set_layout(&mut self, iso: bool, german: bool) {
        self.keys.iso = iso;
        self.keys.german = german;
    }
}
