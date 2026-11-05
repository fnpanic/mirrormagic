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
    pub ddc: DdcStatus,
    pub camera_on: bool,
    pub camera_detail: String,
    pub camera_wanted: bool,
    pub can_inject: bool,
    pub displays: Vec<DisplayInfo>,
    pub obs: ObsStatus,
    pub last_error: String,
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

pub fn update(f: impl FnOnce(&mut Status)) {
    f(&mut cell().lock().unwrap());
    if let Some(tx) = TX.get() {
        let _ = tx.lock().unwrap().send(());
    }
}
