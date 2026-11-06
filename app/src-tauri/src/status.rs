//! Live status shown in the window and tray. Any thread may update it; the UI gets an event.
use mm_core::proto::DisplayInfo;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub connected: bool,
    pub peer: String,
    pub peer_addr: String,
    pub rtt_ms: Option<f64>,
    /// "local" | "edge" | "monitor"
    pub control: String,
    pub monitor_on_mac: bool,
    /// A full-screen app is in front: input hooks removed, no edge crossing (PC only).
    pub game_mode: bool,
    pub ddc: DdcStatus,
    pub camera_on: bool,
    pub camera_detail: String,
    pub camera_wanted: bool,
    /// Microphone path, e.g. "Mikrofon (HD Pro Webcam C920) · 48000 Hz" or "BlackHole 2ch".
    pub mic_detail: String,
    pub can_inject: bool,
    pub displays: Vec<DisplayInfo>,
    pub obs: ObsStatus,
    pub last_error: String,
    /// Last clipboard hand-over, e.g. "sent: text, image".
    pub clipboard: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct DdcStatus {
    pub monitor: String,
    pub current: Option<u16>,
    pub inputs: Vec<u16>,
    pub last: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ObsStatus {
    pub installed: bool,
    pub running: bool,
    pub virtualcam: bool,
    pub detail: String,
}

static STATUS: OnceLock<Mutex<Status>> = OnceLock::new();
static TX: OnceLock<Mutex<std::sync::mpsc::Sender<()>>> = OnceLock::new();

/// UI/tray updates run on their own thread: `update` is called from the input hook, which must
/// return within milliseconds or Windows drops the hook.
pub fn init(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = TX.set(Mutex::new(tx));
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            while rx.try_recv().is_ok() {} // coalesce bursts
            let s = get();
            let _ = app.emit("status", &s);
            crate::tray::refresh(&app, &s);
        }
    });
}

fn cell() -> &'static Mutex<Status> {
    STATUS.get_or_init(|| Mutex::new(Status { control: "local".into(), ..Default::default() }))
}

pub fn get() -> Status {
    cell().lock().unwrap().clone()
}

/// Appends a timestamped line to MirrorMagic.log next to config.json (kept below 1 MB), so
/// problems on either machine can be diagnosed after the fact.
pub fn log(msg: impl AsRef<str>) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock().unwrap();
    let p = crate::config::dir().join("MirrorMagic.log");
    if std::fs::metadata(&p).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.old"));
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = format!("{t} {}\n", msg.as_ref());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn update(f: impl FnOnce(&mut Status)) {
    f(&mut cell().lock().unwrap());
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(());
    }
}
