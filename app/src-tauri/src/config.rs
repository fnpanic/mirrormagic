//! Settings, persisted as JSON in the OS config dir (%APPDATA%\MirrorMagic, ~/Library/Application Support/MirrorMagic).
use mm_core::proto::Side;
use mm_core::secure::PairingKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Mac: Ctrl acts as Command (Ctrl+C/V copy and paste like on Windows), Win acts as Control.
    /// Off: Mac keyboard positions, Ctrl = Control, Win = Option, Alt = Command. (Replaces the
    /// former `ctrl_is_cmd`, which defaulted to on; an old saved value is ignored on purpose.)
    pub windows_shortcuts: bool,
    /// PC: a PC microphone may stream while a Mac app uses its "– PC" device.
    pub mic: bool,
    /// PC: the webcam may stream while a Mac app uses the camera.
    pub camera: bool,
    /// PC: model name of the shared monitor for DDC; empty = the first monitor that answers.
    pub ddc_monitor: String,
    /// Set while MirrorMagic mirrors the Mac screen for monitor mode (undone at the next start after a crash).
    pub mirrored: bool,
    /// Mac: the shared monitor's display while MirrorMagic has its HDMI output off. macOS keeps
    /// it off after MirrorMagic ends (also on update or crash): the next start turns it on again.
    pub hdmi_off: Option<u32>,
    /// Paused from the tray or window: no connection, no input sharing, until resumed (kept across restarts).
    pub paused: bool,
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
            windows_shortcuts: false,
            mic: true,
            camera: true,
            ddc_monitor: String::new(),
            mirrored: false,
            hdmi_off: None,
            paused: false,
        }
    }
}

pub fn dir() -> PathBuf {
    path().parent().unwrap().to_path_buf()
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
        let mut c = match std::fs::read(path()).ok().map(|b| serde_json::from_slice::<Config>(&b)) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                // keep the damaged file: defaults (and on the PC a new pairing key) replace it
                let _ = std::fs::rename(path(), path().with_extension("json.bad"));
                let msg = format!("settings file was damaged ({e}): kept as config.json.bad, defaults restored");
                crate::status::log(&msg);
                let _ = LOAD_ERROR.set(msg);
                Config::default()
            }
            None => Config::default(),
        };
        if cfg!(windows) && PairingKey::parse(&c.pairing_key).is_none() {
            // the PC owns the secret; the Mac gets it typed/pasted in once
            c.pairing_key = PairingKey::generate().to_display();
            save_to_disk(&c);
        }
        PAUSED.store(c.paused, Ordering::SeqCst);
        Mutex::new(c)
    })
}

/// `paused` without cloning the config: checked for every message.
static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn paused() -> bool {
    cell();
    PAUSED.load(Ordering::SeqCst)
}

/// Set when the settings file was damaged at start: shown in the window for this whole run.
static LOAD_ERROR: OnceLock<String> = OnceLock::new();

pub fn load_error() -> Option<String> {
    cell();
    LOAD_ERROR.get().cloned()
}

/// The last save problem, for the window (taken once).
static ERROR: Mutex<Option<String>> = Mutex::new(None);

pub fn take_error() -> Option<String> {
    cell();
    ERROR.lock().unwrap().take()
}

fn save_to_disk(c: &Config) {
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let tmp = p.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, serde_json::to_vec_pretty(c).unwrap()).and_then(|()| std::fs::rename(&tmp, &p)) {
        *ERROR.lock().unwrap() = Some(format!("settings could not be saved: {e}"));
    }
}

pub fn get() -> Config {
    cell().lock().unwrap().clone()
}

pub fn update(f: impl FnOnce(&mut Config)) -> Config {
    let mut c = cell().lock().unwrap();
    f(&mut c);
    PAUSED.store(c.paused, Ordering::SeqCst);
    save_to_disk(&c);
    c.clone()
}

pub fn pairing_key() -> Option<PairingKey> {
    PairingKey::parse(&get().pairing_key)
}
