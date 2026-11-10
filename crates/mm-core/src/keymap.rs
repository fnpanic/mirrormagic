//! PC scan codes → macOS virtual key codes, hotkey parsing, and the German AltGr fix-up.
//!
//! Mapping is positional (like a hardware KVM): the Mac's own layout decides the character.
//! Table derived from the USB HID / Windows set-1 / macOS kVK_* tables (cf. rustdesk-org/rdev, MIT).
use std::collections::HashMap;

/// (DOM `KeyboardEvent.code`, Windows set-1 scan code (0xE0xx = extended), macOS kVK code)
const TABLE: &[(&str, u16, Option<u16>)] = &[
    ("Escape", 0x01, Some(0x35)),
    ("Digit1", 0x02, Some(0x12)), ("Digit2", 0x03, Some(0x13)), ("Digit3", 0x04, Some(0x14)),
    ("Digit4", 0x05, Some(0x15)), ("Digit5", 0x06, Some(0x17)), ("Digit6", 0x07, Some(0x16)),
    ("Digit7", 0x08, Some(0x1A)), ("Digit8", 0x09, Some(0x1C)), ("Digit9", 0x0A, Some(0x19)),
    ("Digit0", 0x0B, Some(0x1D)), ("Minus", 0x0C, Some(0x1B)), ("Equal", 0x0D, Some(0x18)),
    ("Backspace", 0x0E, Some(0x33)), ("Tab", 0x0F, Some(0x30)),
    ("KeyQ", 0x10, Some(0x0C)), ("KeyW", 0x11, Some(0x0D)), ("KeyE", 0x12, Some(0x0E)),
    ("KeyR", 0x13, Some(0x0F)), ("KeyT", 0x14, Some(0x11)), ("KeyY", 0x15, Some(0x10)),
    ("KeyU", 0x16, Some(0x20)), ("KeyI", 0x17, Some(0x22)), ("KeyO", 0x18, Some(0x1F)),
    ("KeyP", 0x19, Some(0x23)), ("BracketLeft", 0x1A, Some(0x21)), ("BracketRight", 0x1B, Some(0x1E)),
    ("Enter", 0x1C, Some(0x24)), ("ControlLeft", 0x1D, Some(0x3B)),
    ("KeyA", 0x1E, Some(0x00)), ("KeyS", 0x1F, Some(0x01)), ("KeyD", 0x20, Some(0x02)),
    ("KeyF", 0x21, Some(0x03)), ("KeyG", 0x22, Some(0x05)), ("KeyH", 0x23, Some(0x04)),
    ("KeyJ", 0x24, Some(0x26)), ("KeyK", 0x25, Some(0x28)), ("KeyL", 0x26, Some(0x25)),
    ("Semicolon", 0x27, Some(0x29)), ("Quote", 0x28, Some(0x27)), ("Backquote", 0x29, Some(0x32)),
    ("ShiftLeft", 0x2A, Some(0x38)), ("Backslash", 0x2B, Some(0x2A)),
    ("KeyZ", 0x2C, Some(0x06)), ("KeyX", 0x2D, Some(0x07)), ("KeyC", 0x2E, Some(0x08)),
    ("KeyV", 0x2F, Some(0x09)), ("KeyB", 0x30, Some(0x0B)), ("KeyN", 0x31, Some(0x2D)),
    ("KeyM", 0x32, Some(0x2E)), ("Comma", 0x33, Some(0x2B)), ("Period", 0x34, Some(0x2F)),
    ("Slash", 0x35, Some(0x2C)), ("ShiftRight", 0x36, Some(0x3C)), ("NumpadMultiply", 0x37, Some(0x43)),
    ("AltLeft", 0x38, Some(0x3A)), ("Space", 0x39, Some(0x31)), ("CapsLock", 0x3A, Some(0x39)),
    ("F1", 0x3B, Some(0x7A)), ("F2", 0x3C, Some(0x78)), ("F3", 0x3D, Some(0x63)), ("F4", 0x3E, Some(0x76)),
    ("F5", 0x3F, Some(0x60)), ("F6", 0x40, Some(0x61)), ("F7", 0x41, Some(0x62)), ("F8", 0x42, Some(0x64)),
    ("F9", 0x43, Some(0x65)), ("F10", 0x44, Some(0x6D)),
    ("NumLock", 0x45, Some(0x47)), ("ScrollLock", 0x46, None),
    ("Numpad7", 0x47, Some(0x59)), ("Numpad8", 0x48, Some(0x5B)), ("Numpad9", 0x49, Some(0x5C)),
    ("NumpadSubtract", 0x4A, Some(0x4E)), ("Numpad4", 0x4B, Some(0x56)), ("Numpad5", 0x4C, Some(0x57)),
    ("Numpad6", 0x4D, Some(0x58)), ("NumpadAdd", 0x4E, Some(0x45)), ("Numpad1", 0x4F, Some(0x53)),
    ("Numpad2", 0x50, Some(0x54)), ("Numpad3", 0x51, Some(0x55)), ("Numpad0", 0x52, Some(0x52)),
    ("NumpadDecimal", 0x53, Some(0x41)), ("IntlBackslash", 0x56, Some(0x0A)),
    ("F11", 0x57, Some(0x67)), ("F12", 0x58, Some(0x6F)),
    ("F13", 0x64, Some(0x69)), ("F14", 0x65, Some(0x6B)), ("F15", 0x66, Some(0x71)), ("F16", 0x67, Some(0x6A)),
    ("F17", 0x68, Some(0x40)), ("F18", 0x69, Some(0x4F)), ("F19", 0x6A, Some(0x50)), ("F20", 0x6B, Some(0x5A)),
    ("NumpadEnter", 0xE01C, Some(0x4C)), ("ControlRight", 0xE01D, Some(0x3E)),
    ("NumpadDivide", 0xE035, Some(0x4B)), ("PrintScreen", 0xE037, None), ("AltRight", 0xE038, Some(0x3D)),
    ("Pause", 0xE045, None),
    ("Home", 0xE047, Some(0x73)), ("ArrowUp", 0xE048, Some(0x7E)), ("PageUp", 0xE049, Some(0x74)),
    ("ArrowLeft", 0xE04B, Some(0x7B)), ("ArrowRight", 0xE04D, Some(0x7C)), ("End", 0xE04F, Some(0x77)),
    ("ArrowDown", 0xE050, Some(0x7D)), ("PageDown", 0xE051, Some(0x79)), ("Insert", 0xE052, Some(0x72)),
    ("Delete", 0xE053, Some(0x75)), ("MetaLeft", 0xE05B, Some(0x37)), ("MetaRight", 0xE05C, Some(0x36)),
    ("ContextMenu", 0xE05D, Some(0x6E)), ("AudioVolumeMute", 0xE020, Some(0x4A)),
    ("AudioVolumeDown", 0xE02E, Some(0x49)), ("AudioVolumeUp", 0xE030, Some(0x48)),
];

pub const SCAN_SCROLL_LOCK: u16 = 0x46;
pub const SCAN_ALTGR: u16 = 0xE038;
/// Windows synthesizes this fake left Ctrl in front of AltGr on European layouts.
pub const SCAN_FAKE_CTRL: u16 = 0x21D;

// macOS virtual key codes used below
const MAC_SHIFT: u16 = 0x38;
const MAC_ISO_SECTION: u16 = 0x0A;
const MAC_ANSI_GRAVE: u16 = 0x32;

pub fn mac_from_scan(scan: u16, iso: bool) -> Option<u16> {
    let code = TABLE.iter().find(|e| e.1 == scan)?.2?;
    // On ISO keyboards macOS swaps these two codes (the ^ key and the <> key on German keyboards).
    Some(match (iso, code) {
        (true, MAC_ANSI_GRAVE) => MAC_ISO_SECTION,
        (true, MAC_ISO_SECTION) => MAC_ANSI_GRAVE,
        _ => code,
    })
}

pub fn scan_from_dom(code: &str) -> Option<u16> {
    TABLE.iter().find(|e| e.0 == code).map(|e| e.1)
}

pub fn dom_from_scan(scan: u16) -> Option<&'static str> {
    TABLE.iter().find(|e| e.1 == scan).map(|e| e.0)
}

// ---------------------------------------------------------------- modifiers & hotkeys

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// Tracks which modifiers are physically held on the PC (from every key event, local or remote).
#[derive(Default)]
pub struct ModTracker {
    held: [bool; 8],
}

impl ModTracker {
    /// Returns true if `scan` is a modifier (and updates the state).
    pub fn update(&mut self, scan: u16, down: bool) -> bool {
        let i = match scan {
            0x1D => 0,
            0xE01D => 1,
            0x38 => 2,
            0xE038 => 3,
            0x2A => 4,
            0x36 => 5,
            0xE05B => 6,
            0xE05C => 7,
            _ => return false,
        };
        self.held[i] = down;
        true
    }
    /// Drops modifiers that the OS says are no longer held.
    pub fn retain(&mut self, is_down: impl Fn(u16) -> bool) {
        const SCANS: [u16; 8] = [0x1D, 0xE01D, 0x38, 0xE038, 0x2A, 0x36, 0xE05B, 0xE05C];
        for (i, s) in SCANS.iter().enumerate() {
            self.held[i] &= is_down(*s);
        }
    }

    pub fn mods(&self) -> Mods {
        let h = &self.held;
        Mods { ctrl: h[0] || h[1], alt: h[2] || h[3], shift: h[4] || h[5], win: h[6] || h[7] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: Mods,
    pub scan: u16,
}

impl Hotkey {
    /// "Ctrl+Alt+M", "ctrl+shift+F12", "Ctrl+Alt+KeyM" (DOM codes accepted).
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut mods = Mods::default();
        let mut scan = None;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "win" | "meta" | "super" | "cmd" => mods.win = true,
                _ if scan.is_some() => return None,
                _ => {
                    let dom = if part.len() == 1 {
                        let c = part.chars().next()?.to_ascii_uppercase();
                        if c.is_ascii_alphabetic() { format!("Key{c}") } else if c.is_ascii_digit() { format!("Digit{c}") } else { return None }
                    } else {
                        part.to_string()
                    };
                    scan = Some(scan_from_dom(&dom).or_else(|| {
                        // case-insensitive DOM names ("f12", "scrolllock")
                        TABLE.iter().find(|e| e.0.eq_ignore_ascii_case(&dom)).map(|e| e.1)
                    })?);
                }
            }
        }
        Some(Hotkey { mods, scan: scan? })
    }

    pub fn format(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods.ctrl { parts.push("Ctrl".into()) }
        if self.mods.alt { parts.push("Alt".into()) }
        if self.mods.shift { parts.push("Shift".into()) }
        if self.mods.win { parts.push("Win".into()) }
        let k = dom_from_scan(self.scan).unwrap_or("?");
        let k = k.strip_prefix("Key").or_else(|| k.strip_prefix("Digit")).unwrap_or(k);
        parts.push(k.to_string());
        parts.join("+")
    }

    /// Needs Ctrl, Alt or Win, or is a function key: Shift+letter would fire while typing.
    pub fn is_usable(&self) -> bool {
        self.mods.ctrl || self.mods.alt || self.mods.win || matches!(self.scan, 0x3B..=0x44 | 0x57 | 0x58 | 0x64..=0x6B)
    }

    pub fn matches(&self, mods: Mods, scan: u16) -> bool {
        scan == self.scan && mods == self.mods
    }
}

// ---------------------------------------------------------------- Mac-side key translation

/// Converts PC key events into macOS key events, including the German AltGr fix-up:
/// on a German PC keyboard `AltGr+Q` is `@`, on a German Mac it is `Option+L`, etc.
pub struct KeyTranslator {
    pub iso: bool,
    pub german: bool,
    /// Windows habits on the Mac: Ctrl acts as Command (Ctrl+C copies) and the Win key as Control.
    /// Off (default): keys keep the Mac keyboard's positions, Ctrl = Control, Win = Option,
    /// Alt = Command (AltGr stays Option, for the German @ € { } fix-ups).
    pub windows_shortcuts: bool,
    altgr: bool,
    /// Win held: F-keys send real F1–F12 (like fn on a Mac keyboard) instead of the top-row functions.
    win: bool,
    /// What each held PC key was sent as, so the release matches the press.
    sent: HashMap<u16, Vec<u16>>,
}

/// German Windows AltGr combos → German macOS Option combos (key code, add Shift).
fn german_altgr(scan: u16) -> Option<(u16, bool)> {
    Some(match scan {
        0x10 => (0x25, false), // Q  @  -> Option+L
        0x08 => (0x1C, false), // 7  {  -> Option+8
        0x09 => (0x17, false), // 8  [  -> Option+5
        0x0A => (0x16, false), // 9  ]  -> Option+6
        0x0B => (0x19, false), // 0  }  -> Option+9
        0x0C => (0x1A, true),  // ß  \  -> Option+Shift+7
        0x56 => (0x1A, false), // <> |  -> Option+7
        0x1B => (0x2D, false), // +  ~  -> Option+N
        _ => return None,      // € (E) and µ (M) sit on the same keys on both systems
    })
}

/// (mac key code, down). Codes with [`AUX`] set are media/brightness keys (an NSSystemDefined
/// event with that NX key type); codes with [`PLAIN`] set are posted without modifier flags.
pub type MacKey = (u16, bool);

/// Marks an NX_KEYTYPE media key: volume, brightness, play.
pub const AUX: u16 = 0x1000;
/// Marks a key that must reach the Mac without modifiers (Win+F-key = plain F-key).
pub const PLAIN: u16 = 0x2000;
const MAC_MISSION_CONTROL: u16 = 0xA0;
const MAC_COMMAND: u16 = 0x37;
const MAC_SPACE: u16 = 0x31;

/// The MacBook top row for F1–F12 (without fn), plus the PC keyboard's own media keys.
fn top_row(scan: u16) -> Option<Vec<u16>> {
    Some(match scan {
        0x3B => vec![AUX | 3],                      // F1  brightness down
        0x3C => vec![AUX | 2],                      // F2  brightness up
        0x3D => vec![MAC_MISSION_CONTROL],          // F3  Mission Control
        0x3E => vec![MAC_COMMAND, MAC_SPACE],       // F4  Spotlight
        0x41 | 0xE010 => vec![AUX | 20],            // F7  previous / rewind
        0x42 | 0xE022 => vec![AUX | 16],            // F8  play / pause
        0x43 | 0xE019 => vec![AUX | 19],            // F9  next / fast-forward
        0x44 | 0xE020 => vec![AUX | 7],             // F10 mute
        0x57 | 0xE02E => vec![AUX | 1],             // F11 volume down
        0x58 | 0xE030 => vec![AUX],                  // F12 volume up (NX type 0)
        _ => return None,                           // F5, F6: real keys (no documented Mac code)
    })
}

fn is_f_key(scan: u16) -> bool {
    matches!(scan, 0x3B..=0x44 | 0x57 | 0x58)
}

impl KeyTranslator {
    pub fn new(iso: bool, german: bool) -> Self {
        KeyTranslator { iso, german, windows_shortcuts: false, altgr: false, win: false, sent: HashMap::new() }
    }

    pub fn translate(&mut self, scan: u16, down: bool) -> Vec<MacKey> {
        if scan == SCAN_FAKE_CTRL {
            return vec![];
        }
        if scan == SCAN_ALTGR {
            self.altgr = down;
        }
        if matches!(scan, 0xE05B | 0xE05C) {
            self.win = down;
        }
        if !down {
            return match self.sent.remove(&scan) {
                Some(codes) => codes.into_iter().rev().map(|c| (c, false)).collect(),
                None => vec![],
            };
        }
        if let Some(prev) = self.sent.get(&scan) {
            // autorepeat: repeat the main key only
            return vec![(*prev.last().unwrap(), true)];
        }
        let media = if self.win && is_f_key(scan) { None } else { top_row(scan) };
        let codes = match (self.german && self.altgr).then(|| german_altgr(scan)).flatten() {
            Some((code, true)) => vec![MAC_SHIFT, code],
            Some((code, false)) => vec![code],
            None if media.is_some() => media.unwrap(),
            None => match mac_from_scan(scan, self.iso) {
                Some(c) if is_f_key(scan) && self.win => vec![PLAIN | c],
                Some(c) if self.windows_shortcuts => vec![match c {
                    0x3B => 0x37, // left Ctrl -> left Command
                    0x3E => 0x36, // right Ctrl -> right Command
                    0x37 => 0x3B, // left Win (Command) -> left Control
                    0x36 => 0x3E, // right Win -> right Control
                    c => c,
                }],
                Some(c) => vec![match scan {
                    0x38 => 0x37,   // left Alt -> left Command (Mac keyboard position)
                    0xE05B => 0x3A, // left Win -> left Option
                    0xE05C => 0x3D, // right Win -> right Option
                    _ => c,
                }],
                None => return vec![],
            },
        };
        let out = codes.iter().map(|&c| (c, true)).collect();
        self.sent.insert(scan, codes);
        out
    }

    /// Key-ups for everything still held (on Exit / disconnect).
    pub fn release_all(&mut self) -> Vec<MacKey> {
        self.altgr = false;
        self.win = false;
        self.sent.drain().flat_map(|(_, codes)| codes.into_iter().rev().map(|c| (c, false))).collect()
    }
}

/// CGEventFlags for the modifiers currently held on the Mac side (device-independent + device bits).
#[derive(Default)]
pub struct MacFlags {
    held: HashMap<u16, ()>,
}

impl MacFlags {
    pub fn update(&mut self, code: u16, down: bool) {
        if down { self.held.insert(code, ()); } else { self.held.remove(&code); }
    }
    pub fn is_modifier(code: u16) -> bool {
        matches!(code, 0x36..=0x38 | 0x3A..=0x3F) // 0x39 Caps Lock is a normal key event
    }
    pub fn bits(&self) -> u64 {
        let mut f = 0u64;
        for &c in self.held.keys() {
            f |= match c {
                0x38 => 0x20000 | 0x2,     // shift L
                0x3C => 0x20000 | 0x4,     // shift R
                0x3B => 0x40000 | 0x1,     // control L
                0x3E => 0x40000 | 0x2000,  // control R
                0x3A => 0x80000 | 0x20,    // option L
                0x3D => 0x80000 | 0x40,    // option R
                0x37 => 0x100000 | 0x8,    // command L
                0x36 => 0x100000 | 0x10,   // command R
                0x3F => 0x800000,          // fn
                _ => 0,
            };
        }
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_letters_and_german_iso_keys() {
        assert_eq!(mac_from_scan(0x1E, true), Some(0x00)); // A
        assert_eq!(mac_from_scan(0x15, true), Some(0x10)); // Y position (Z on German) stays positional
        assert_eq!(mac_from_scan(0x2C, true), Some(0x06)); // Z position (Y on German)
        assert_eq!(mac_from_scan(0x0C, true), Some(0x1B)); // ß position
        assert_eq!(mac_from_scan(0x27, true), Some(0x29)); // ö position
        assert_eq!(mac_from_scan(0x29, true), Some(MAC_ISO_SECTION)); // ^ key on ISO
        assert_eq!(mac_from_scan(0x56, true), Some(MAC_ANSI_GRAVE)); // <> key on ISO
        assert_eq!(mac_from_scan(0x29, false), Some(MAC_ANSI_GRAVE));
        assert_eq!(mac_from_scan(0xE05B, true), Some(0x37)); // Win -> Command
        assert_eq!(mac_from_scan(SCAN_SCROLL_LOCK, true), None);
        // every table entry is unique on both axes
        for (i, a) in TABLE.iter().enumerate() {
            for b in &TABLE[i + 1..] {
                assert_ne!(a.0, b.0);
                assert_ne!(a.1, b.1);
            }
        }
    }

    #[test]
    fn hotkey_parse_format_match() {
        let h = Hotkey::parse("Ctrl+Alt+M").unwrap();
        assert_eq!(h.scan, 0x32);
        assert_eq!(h.format(), "Ctrl+Alt+M");
        assert_eq!(Hotkey::parse("ctrl + alt + KeyM"), Some(h));
        assert_eq!(Hotkey::parse("Shift+F12").unwrap().format(), "Shift+F12");
        assert_eq!(Hotkey::parse("Ctrl+Alt"), None);
        assert_eq!(Hotkey::parse("Ctrl+M+N"), None);
        assert!(h.is_usable() && Hotkey::parse("Shift+F12").unwrap().is_usable() && Hotkey::parse("F13").unwrap().is_usable());
        assert!(!Hotkey::parse("Shift+A").unwrap().is_usable() && !Hotkey::parse("M").unwrap().is_usable());
        let mut t = ModTracker::default();
        t.update(0x1D, true);
        t.update(0xE038, true); // AltGr counts as Alt
        assert!(h.matches(t.mods(), 0x32));
        t.update(0x2A, true);
        assert!(!h.matches(t.mods(), 0x32)); // extra Shift -> no match
    }

    #[test]
    fn modifiers_keep_the_mac_keyboard_positions() {
        let mut k = KeyTranslator::new(true, true);
        assert_eq!(k.translate(0x1D, true), vec![(0x3B, true)]); // Ctrl -> Control
        assert_eq!(k.translate(0xE05B, true), vec![(0x3A, true)]); // Win -> Option
        assert_eq!(k.translate(0x38, true), vec![(0x37, true)]); // Alt -> Command
        assert_eq!(k.translate(0xE01D, true), vec![(0x3E, true)]); // right Ctrl -> right Control
        assert_eq!(k.translate(0xE05C, true), vec![(0x3D, true)]); // right Win -> right Option
        assert_eq!(k.translate(0x38, false), vec![(0x37, false)]); // the release matches
        // AltGr keeps the German characters: AltGr+Q = @ = Option+L
        let mut k = KeyTranslator::new(true, true);
        k.translate(SCAN_FAKE_CTRL, true);
        assert_eq!(k.translate(SCAN_ALTGR, true), vec![(0x3D, true)]);
        assert_eq!(k.translate(0x10, true), vec![(0x25, true)]);
    }

    #[test]
    fn f_keys_are_the_macbook_top_row_and_win_makes_them_real() {
        let mut k = KeyTranslator::new(true, true);
        assert_eq!(k.translate(0x3B, true), vec![(AUX | 3, true)]); // F1 brightness down
        assert_eq!(k.translate(0x3B, true), vec![(AUX | 3, true)]); // held: repeats
        assert_eq!(k.translate(0x3B, false), vec![(AUX | 3, false)]);
        assert_eq!(k.translate(0x3D, true), vec![(0xA0, true)]); // F3 Mission Control
        assert_eq!(k.translate(0x3E, true), vec![(0x37, true), (0x31, true)]); // F4 Spotlight = Cmd+Space
        assert_eq!(k.translate(0x3E, false), vec![(0x31, false), (0x37, false)]);
        assert_eq!(k.translate(0x58, true), vec![(AUX, true)]); // F12 volume up (NX type 0)
        assert_eq!(k.translate(0x3F, true), vec![(0x60, true)]); // F5 stays F5
        assert_eq!(k.translate(0xE030, true), vec![(AUX, true)]); // the PC's own volume-up key too
        // Win held: real F-keys, posted without the Option flag
        k.translate(0xE05B, true);
        assert_eq!(k.translate(0x3B, true), vec![(PLAIN | 0x7A, true)]); // F1
        assert_eq!(k.translate(0x3B, false), vec![(PLAIN | 0x7A, false)]);
        k.translate(0xE05B, false);
        assert_eq!(k.translate(0x3B, true), vec![(AUX | 3, true)]);
    }

    #[test]
    fn german_altgr_becomes_option_combo() {
        let mut k = KeyTranslator::new(true, true);
        assert_eq!(k.translate(SCAN_FAKE_CTRL, true), vec![]);
        assert_eq!(k.translate(SCAN_ALTGR, true), vec![(0x3D, true)]);
        assert_eq!(k.translate(0x10, true), vec![(0x25, true)]); // @ -> Option+L
        assert_eq!(k.translate(0x10, false), vec![(0x25, false)]);
        assert_eq!(k.translate(0x0C, true), vec![(0x38, true), (0x1A, true)]); // \ -> Option+Shift+7
        assert_eq!(k.translate(0x0C, false), vec![(0x1A, false), (0x38, false)]);
        assert_eq!(k.translate(SCAN_ALTGR, false), vec![(0x3D, false)]);
        assert_eq!(k.translate(0x10, true), vec![(0x0C, true)]); // plain Q again
        assert_eq!(k.release_all(), vec![(0x0C, false)]);
        // non-German: no rewrite
        let mut k = KeyTranslator::new(true, false);
        k.translate(SCAN_ALTGR, true);
        assert_eq!(k.translate(0x10, true), vec![(0x0C, true)]);
    }

    #[test]
    fn flags_from_held_modifiers() {
        let mut f = MacFlags::default();
        f.update(0x37, true);
        f.update(0x38, true);
        assert_eq!(f.bits(), 0x100000 | 0x8 | 0x20000 | 0x2);
        f.update(0x37, false);
        assert_eq!(f.bits(), 0x20000 | 0x2);
        assert!(MacFlags::is_modifier(0x3D));
        assert!(!MacFlags::is_modifier(0x00));
    }
}

#[cfg(test)]
mod ctrl_cmd_tests {
    use super::*;

    #[test]
    fn ctrl_c_becomes_command_c_when_swapped() {
        let mut k = KeyTranslator::new(true, true);
        k.windows_shortcuts = true;
        assert_eq!(k.translate(0x1D, true), vec![(0x37, true)]); // Ctrl -> Command
        assert_eq!(k.translate(0x2E, true), vec![(0x08, true)]); // C
        assert_eq!(k.translate(0x2E, false), vec![(0x08, false)]);
        assert_eq!(k.translate(0x1D, false), vec![(0x37, false)]);
        assert_eq!(k.translate(0xE05B, true), vec![(0x3B, true)]); // Win -> Control
        // AltGr still works: its fake Ctrl is dropped, not turned into Command
        assert_eq!(k.translate(SCAN_FAKE_CTRL, true), vec![]);
    }
}
