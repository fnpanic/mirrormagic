//! Settings, persisted as JSON in the OS config dir (%APPDATA%\MirrorMagic, ~/Library/Application Support/MirrorMagic).
use mm_core::proto::Side;
use mm_core::secure::PairingKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    // shared
    pub clipboard: bool,
    pub pairing_key: String,
    // PC
    /// Side of the PC monitor where the Mac's screen sits.
    pub side: Side,
    pub edge_enabled: bool,
    pub monitor_hotkey: String,
    /// VCP 0x60 value of the Mac's input on the shared monitor (0x11 = HDMI 1).
    pub mac_input: u16,
    /// VCP 0x60 value of the PC's input; learned when switching away (0x0F = DisplayPort 1).
    pub pc_input: u16,
    pub camera_width: u32,
    pub camera_height: u32,
    pub camera_fps: u32,
    // Mac
    /// PC host name or IP; empty = discover on the LAN.
    pub host: String,
    pub speed: f64,
    pub invert_scroll: bool,
    /// None = auto-detect from the active keyboard layout.
    pub german_fixups: Option<bool>,
    pub obs_password: String,
    pub camera_auto: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            clipboard: true,
            pairing_key: String::new(),
            side: Side::Left,
            edge_enabled: true,
            monitor_hotkey: "Ctrl+Alt+M".into(),
            mac_input: 0x11,
            pc_input: 0x0F,
            camera_width: 1280,
            camera_height: 720,
            camera_fps: 30,
            host: String::new(),
            speed: 1.0,
            invert_scroll: false,
            german_fixups: None,
            obs_password: String::new(),
            camera_auto: true,
        }
    }
}

fn path() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    #[cfg(not(windows))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support")).unwrap_or_default();
    base.join("MirrorMagic").join("config.json")
}

static CONFIG: OnceLock<Mutex<Config>> = OnceLock::new();

fn cell() -> &'static Mutex<Config> {
    CONFIG.get_or_init(|| {
        let mut c: Config = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if cfg!(windows) && PairingKey::parse(&c.pairing_key).is_none() {
            // the PC owns the secret; the Mac gets it typed/pasted in once
            c.pairing_key = PairingKey::generate().to_display();
            save_to_disk(&c);
        }
        Mutex::new(c)
    })
}

fn save_to_disk(c: &Config) {
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(c).unwrap()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

pub fn get() -> Config {
    cell().lock().unwrap().clone()
}

pub fn update(f: impl FnOnce(&mut Config)) -> Config {
    let mut c = cell().lock().unwrap();
    f(&mut c);
    save_to_disk(&c);
    c.clone()
}

pub fn pairing_key() -> Option<PairingKey> {
    PairingKey::parse(&get().pairing_key)
}
