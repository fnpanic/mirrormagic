//! Mac (secondary) runtime: connects to the PC, injects its input, switches the monitor back,
//! and turns the PC camera into the OBS virtual camera when an app opens it.
pub mod inject;
pub mod obs;
pub mod sys;

use crate::{clipboard, config, ddc, mjpeg, status};
use inject::Injector;
use mm_core::proto::{Channel, Mode, Msg, DEFAULT_PORT, PROTOCOL_VERSION};
use mm_core::secondary::{Displays, Effect, Secondary};
use mm_core::secure;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};

static CTRL_TX: Mutex<Option<mpsc::UnboundedSender<Msg>>> = Mutex::new(None);
static INJ: OnceLock<Mutex<Injector>> = OnceLock::new();
static SEC: OnceLock<Mutex<Secondary>> = OnceLock::new();
static FRAMES: OnceLock<watch::Sender<Option<Arc<Vec<u8>>>>> = OnceLock::new();
static CAM_WANTED: AtomicBool = AtomicBool::new(false);
/// UI "stream now" switch, used when automatic detection is off.
pub static CAM_MANUAL: AtomicBool = AtomicBool::new(false);
static RT: OnceLock<tokio::runtime::Handle> = OnceLock::new();
static T0: OnceLock<Instant> = OnceLock::new();

fn inj() -> std::sync::MutexGuard<'static, Injector> {
    INJ.get().unwrap().lock().unwrap_or_else(|e| e.into_inner())
}
fn sec() -> std::sync::MutexGuard<'static, Secondary> {
    SEC.get().unwrap().lock().unwrap_or_else(|e| e.into_inner())
}

fn send(m: Msg) {
    if let Some(tx) = CTRL_TX.lock().unwrap().as_ref() {
        let _ = tx.send(m);
    }
}

/// Detected once on the main thread in `start` (TIS calls assert off the main thread on macOS 14+).
static LAYOUT_GERMAN: AtomicBool = AtomicBool::new(false);

fn german(c: &config::Config) -> bool {
    c.german_fixups.unwrap_or_else(|| LAYOUT_GERMAN.load(Ordering::Relaxed))
}

/// Re-reads displays, keyboard layout and permissions; returns PeerInfo for the PC.
fn refresh() -> Msg {
    let c = config::get();
    let ds = sys::displays();
    if let Some((edge, monitor)) = sys::layout(&ds) {
        let mut s = sec();
        s.displays = Displays { edge, monitor };
        s.speed = c.speed;
    }
    {
        let mut i = inj();
        i.set_layout(sys::keyboard_is_iso(), german(&c));
        i.invert_scroll = c.invert_scroll;
    }
    let can = sys::can_inject(false);
    status::update(|s| {
        s.displays = ds.clone();
        s.can_inject = can;
    });
    Msg::PeerInfo { displays: ds, can_inject: can }
}

pub fn start(rt: &tokio::runtime::Handle) {
    let _ = RT.set(rt.clone());
    T0.get_or_init(Instant::now);
    LAYOUT_GERMAN.store(sys::layout_id().contains("German"), Ordering::Relaxed);
    let c = config::get();
    let ds = sys::displays();
    let (edge, monitor) = sys::layout(&ds).unwrap_or((mm_core::primary::Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 }, None));
    let _ = SEC.set(Mutex::new(Secondary::new(Displays { edge, monitor })));
    let _ = INJ.set(Mutex::new(Injector::new(sys::keyboard_is_iso(), german(&c))));
    // ask once for Accessibility; macOS shows its own dialog
    if !sys::can_inject(true) {
        status::update(|s| s.last_error = "Allow MirrorMagic under Privacy & Security → Accessibility".into());
    }
    refresh();
    let (ftx, frx) = watch::channel(None);
    let _ = FRAMES.set(ftx);
    rt.spawn(mjpeg::serve(frx));
    rt.spawn(client_loop());
    clipboard::watch(|text| send(Msg::Clipboard { text }));
    std::thread::spawn(camera_watch);
    if !c.obs_password.is_empty() && c.camera_auto {
        rt.spawn(async { setup_obs(true).await });
    } else {
        obs_status(String::new());
    }
}

fn obs_status(detail: String) {
    let (installed, running) = (obs::installed(), obs::running());
    status::update(|s| {
        s.obs.installed = installed;
        s.obs.running = running;
        if !detail.is_empty() {
            s.obs.detail = detail;
        }
    });
}

pub async fn setup_obs(start: bool) -> Result<String, String> {
    let r = obs::ensure(mjpeg::URL, start).await;
    let ok = r.is_ok();
    status::update(|s| s.obs.virtualcam = ok);
    obs_status(match &r {
        Ok(m) | Err(m) => m.clone(),
    });
    r
}

// ---------------------------------------------------------------- PC connection

async fn discover() -> Option<String> {
    let s = UdpSocket::bind(("0.0.0.0", 0)).await.ok()?;
    s.set_broadcast(true).ok()?;
    s.send_to(mm_core::DISCOVER_ASK, ("255.255.255.255", DEFAULT_PORT)).await.ok()?;
    let mut buf = [0u8; 128];
    let (n, from) = tokio::time::timeout(Duration::from_millis(1500), s.recv_from(&mut buf)).await.ok()?.ok()?;
    buf[..n].starts_with(mm_core::DISCOVER_REPLY_PREFIX).then(|| from.ip().to_string())
}

/// All PCs answering on the LAN: (address, name). For the "Find PC" button.
pub async fn discover_all() -> Vec<(String, String)> {
    let Ok(s) = UdpSocket::bind(("0.0.0.0", 0)).await else { return vec![] };
    let _ = s.set_broadcast(true);
    let _ = s.send_to(mm_core::DISCOVER_ASK, ("255.255.255.255", DEFAULT_PORT)).await;
    let mut out = vec![];
    let mut buf = [0u8; 128];
    while let Ok(Ok((n, from))) = tokio::time::timeout(Duration::from_millis(1500), s.recv_from(&mut buf)).await {
        if let Some(name) = buf[..n].strip_prefix(mm_core::DISCOVER_REPLY_PREFIX) {
            out.push((from.ip().to_string(), String::from_utf8_lossy(name).into_owned()));
        }
    }
    out
}

async fn open(channel: Channel) -> Result<(secure::Sender, secure::Receiver, String, String), secure::Error> {
    let c = config::get();
    let key = config::pairing_key().ok_or("Enter the pairing key shown on the PC")?;
    let host = if c.host.trim().is_empty() { discover().await.ok_or("no PC found on the network: enter its address")? } else { c.host.trim().to_string() };
    let addr = if host.contains(':') && !host.starts_with('[') && host.matches(':').count() == 1 { host.clone() } else { format!("{host}:{DEFAULT_PORT}") };
    let s = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await.map_err(|_| format!("{addr}: no answer"))??;
    let (mut tx, mut rx) = secure::connect(s, &key).await.map_err(|_| "the PC refused the pairing key")?;
    tx.send(&Msg::Hello { version: PROTOCOL_VERSION, name: sys::hostname(), channel }).await?;
    let Msg::Hello { version, name, .. } = rx.recv().await? else { return Err("unexpected reply".into()) };
    if version != PROTOCOL_VERSION {
        return Err(format!("{name} runs an incompatible MirrorMagic version").into());
    }
    Ok((tx, rx, name, host))
}

async fn client_loop() {
    loop {
        match open(Channel::Control).await {
            Ok((tx, rx, name, host)) => {
                status::update(|s| {
                    s.connected = true;
                    s.peer = name;
                    s.peer_addr = host;
                    s.last_error.clear();
                });
                let r = session(tx, rx).await;
                if let Err(e) = r {
                    status::update(|s| s.last_error = format!("connection lost: {e}"));
                }
                status::update(|s| {
                    s.connected = false;
                    s.rtt_ms = None;
                    s.control = "local".into();
                });
            }
            Err(e) => status::update(|s| s.last_error = e.to_string()),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn session(mut tx: secure::Sender, mut rx: secure::Receiver) -> Result<(), secure::Error> {
    let (utx, mut urx) = mpsc::unbounded_channel::<Msg>();
    *CTRL_TX.lock().unwrap() = Some(utx.clone());
    tx.send(&refresh()).await?;
    if CAM_WANTED.load(Ordering::SeqCst) {
        tx.send(&Msg::CameraWanted { on: true }).await?;
    }
    let writer = tokio::spawn(async move {
        while let Some(m) = urx.recv().await {
            if tx.send(&m).await.is_err() {
                break;
            }
        }
    });
    let pinger = {
        let utx = utx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if utx.send(Msg::Ping { t: T0.get().unwrap().elapsed().as_micros() as u64 }).is_err() {
                    break;
                }
            }
        })
    };
    let r: Result<(), secure::Error> = async {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(7), rx.recv()).await.map_err(|_| "PC timed out")??;
            handle(m, &utx);
        }
    }
    .await;
    writer.abort();
    pinger.abort();
    *CTRL_TX.lock().unwrap() = None;
    sec().exit();
    inj().release_all();
    r
}

fn set_control(c: &str) {
    let c = c.to_string();
    status::update(|s| s.control = c);
}

fn handle(m: Msg, utx: &mpsc::UnboundedSender<Msg>) {
    match m {
        Msg::Enter { mode, side, along } => {
            let info = refresh(); // displays may have changed (HDMI plugged, lid closed)
            let _ = utx.send(info);
            let (x, y) = sec().enter(mode, side, along);
            inj().warp(x, y);
            set_control(if mode == Mode::Monitor { "monitor" } else { "edge" });
        }
        Msg::Motion { dx, dy } => {
            let os = inject::cursor();
            let effect = sec().motion(dx, dy, os);
            match effect {
                Some(Effect::MoveTo { x, y, dx, dy }) => inj().move_to(x, y, dx, dy),
                Some(Effect::Leave { along }) => {
                    inj().release_all();
                    let _ = utx.send(Msg::Leave { along });
                    set_control("local");
                }
                None => {}
            }
        }
        Msg::Button { button, down } => inj().button(button, down),
        Msg::Wheel { dx, dy } => inj().wheel(dx, dy),
        Msg::Key { scan, down } => inj().key(scan, down),
        Msg::Exit => {
            sec().exit();
            inj().release_all();
            set_control("local");
        }
        Msg::ReleaseAll => inj().release_all(),
        Msg::Ddc { value } => {
            let utx = utx.clone();
            std::thread::spawn(move || {
                let r = ddc::set_input(value);
                let _ = utx.send(Msg::DdcResult { ok: r.is_ok(), detail: r.unwrap_or_else(|e| e) });
            });
        }
        Msg::Clipboard { text } => clipboard::set_from_peer(text),
        Msg::CameraState { on, detail } => status::update(|s| {
            s.camera_on = on;
            s.camera_detail = detail;
        }),
        Msg::Ping { t } => {
            let _ = utx.send(Msg::Pong { t });
        }
        Msg::Pong { t } => {
            let rtt = (T0.get().unwrap().elapsed().as_micros() as u64).saturating_sub(t) as f64 / 1000.0;
            status::update(|s| s.rtt_ms = Some(rtt));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- camera

/// Watches whether any app uses the OBS virtual camera; streams the PC camera only then.
fn camera_watch() {
    let mut last_use = None::<Instant>;
    let mut task: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let auto = config::get().camera_auto;
        let in_use = auto && sys::cameras().iter().any(|(n, running)| *running && n.contains("OBS Virtual Camera"));
        if in_use {
            last_use = Some(Instant::now());
        }
        // keep streaming 10 s after the last user, so call apps that re-open the camera don't flicker
        let want = CAM_MANUAL.load(Ordering::SeqCst) || last_use.is_some_and(|t| t.elapsed() < Duration::from_secs(10));
        if want != CAM_WANTED.swap(want, Ordering::SeqCst) {
            status::update(|s| s.camera_wanted = want);
            send(Msg::CameraWanted { on: want });
            if want {
                task = Some(RT.get().unwrap().spawn(camera_stream()));
            } else if let Some(t) = task.take() {
                t.abort();
                let _ = FRAMES.get().unwrap().send(None);
            }
        }
    }
}

async fn camera_stream() {
    while CAM_WANTED.load(Ordering::SeqCst) {
        if let Ok((_tx, mut rx, _, _)) = open(Channel::Camera).await {
            while let Ok(Msg::Frame { jpeg }) = rx.recv().await {
                let _ = FRAMES.get().unwrap().send(Some(Arc::new(jpeg)));
            }
        }
        let _ = FRAMES.get().unwrap().send(None);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
