//! Mac (secondary) runtime: connects to the PC, injects its input, switches the monitor back,
//! and turns the PC camera into the OBS virtual camera when an app opens it.
pub mod camlog;
pub mod inject;
pub mod obs;
pub mod sys;
pub mod vaudio;

use crate::{clipboard, config, ddc, mjpeg, status};
use inject::Injector;
use mm_core::proto::{Channel, Mode, Msg, DEFAULT_PORT, PROTOCOL_VERSION};
use mm_core::secondary::{Displays, Effect, Secondary};
use mm_core::secure;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// Protocol version agreed with the PC.
static PEER_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(PROTOCOL_VERSION);

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
        s.set_displays(Displays { edge, monitor }, inject::cursor());
        s.speed = c.speed;
    }
    {
        let mut i = inj();
        i.set_layout(sys::keyboard_is_iso(), german(&c));
        i.invert_scroll = c.invert_scroll;
        i.set_windows_shortcuts(c.windows_shortcuts);
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
    mirror(false); // undo a mirror left by a crash during monitor mode
    hdmi_restore();
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
    status::log(format!("start: cameras {:?}", sys::cameras()));
    std::thread::spawn(camera_watch);
    std::thread::spawn(audio_watch);
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
    // the virtual camera runs only while an app watches (camera_watch switches it)
    let r = obs::ensure(mjpeg::URL, start, CAM_WANTED.load(Ordering::SeqCst)).await;
    status::log(format!("obs setup: {r:?}"));
    let ok = r.is_ok();
    let vc = ok && CAM_WANTED.load(Ordering::SeqCst);
    status::update(|s| s.obs.virtualcam = vc);
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
    if !(mm_core::proto::MIN_COMPATIBLE_VERSION..=PROTOCOL_VERSION).contains(&version) {
        return Err(format!("{name} runs an incompatible MirrorMagic version").into());
    }
    PEER_VERSION.store(version, Ordering::SeqCst);
    Ok((tx, rx, name, host))
}

async fn client_loop() {
    let mut lost = Instant::now();
    loop {
        if lost.elapsed().as_secs() >= HDMI_BACK_S {
            hdmi_on("no PC for 5 min");
        }
        match open(Channel::Control).await {
            Ok((tx, rx, name, host)) => {
                status::update(|s| {
                    s.connected = true;
                    s.peer = name;
                    s.peer_addr = host;
                    s.last_error.clear();
                });
                let r = session(tx, rx).await;
                lost = Instant::now();
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
            let mut could = sys::can_inject(false);
            let mut reported: Option<Instant> = None;
            let mut seen = String::new();
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                hdmi_off();
                // diagnostics: where is the Mac's pointer while the PC does not control it?
                if !sec().is_active() {
                    let (now, at) = pointer_state();
                    if now != seen {
                        status::log(format!("pointer (Mac in charge): {now}, at {at}"));
                        seen = now;
                    }
                }
                // someone works on the Mac: keep the PC screen on (else the monitor shows the Mac's second screen)
                let since = reported.map_or(f64::MAX, |r| r.elapsed().as_secs_f64());
                if PEER_VERSION.load(Ordering::SeqCst) >= 5 && mm_core::secondary::mac_active_due(sys::idle_seconds(), since) {
                    reported = Some(Instant::now());
                    let _ = utx.send(Msg::MacActive);
                }
                let can = sys::can_inject(false);
                if can != could {
                    could = can; // Accessibility granted/revoked: tell the PC right away
                    let _ = utx.send(refresh());
                }
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
    mirror(false);
    PC_DISPLAY_OFF.store(false, Ordering::SeqCst);
    r
}

/// ("visible on the built-in display", "x,y"): changes of the first are logged to find pointer trouble.
fn pointer_state() -> (String, String) {
    let Some((x, y)) = inject::cursor() else { return ("unknown".into(), String::new()) };
    let d = sec().displays.clone();
    let on = if d.edge.contains(x, y) {
        "built-in display"
    } else if d.monitor.is_some_and(|m| m.contains(x, y)) {
        "shared monitor (invisible unless monitor mode)"
    } else {
        "outside all displays"
    };
    let visible = if sys::cursor_visible() { "visible" } else { "HIDDEN" };
    (format!("{visible} on the {on}"), format!("{x:.0},{y:.0}"))
}

static PC_DISPLAY_OFF: AtomicBool = AtomicBool::new(false);
static SLEEP_WATCH: AtomicBool = AtomicBool::new(false);
/// Nobody has touched the Mac for this long (seconds) while the PC's screen is off: user away.
const AWAY_S: f64 = 60.0;

/// While the PC's screen is off, the monitor's automatic input detection shows the Mac's HDMI
/// signal (its second screen). Once the Mac is idle too, the user is away from both: put the
/// displays to sleep, so the monitor gets no signal and goes to standby. Not while a program
/// keeps the display awake (video, caffeinate -d, a test run).
fn sleep_displays_when_idle() {
    while PC_DISPLAY_OFF.load(Ordering::SeqCst) {
        let idle = sys::idle_seconds();
        if idle >= AWAY_S {
            if sys::display_sleep_prevented() {
                status::log("PC screen off, Mac idle: a program keeps the display awake, leaving it on");
            } else {
                status::log(format!("PC screen off, Mac idle for {idle:.0} s: displays to sleep"));
                sys::sleep_displays();
            }
            break;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    SLEEP_WATCH.store(false, Ordering::SeqCst);
}

/// The shared monitor's display while MirrorMagic has its output off (it is not online then).
static HDMI_OFF: Mutex<Option<u32>> = Mutex::new(None);
/// When monitor mode last ended (ms since T0): the switch back over DDC needs the HDMI link.
static MONITOR_LEFT_MS: AtomicU64 = AtomicU64::new(0);
/// Turn the HDMI output back on after this long without the PC (PC shut down: monitor for the Mac).
const HDMI_BACK_S: u64 = 300;

fn now_ms() -> u64 {
    T0.get().unwrap().elapsed().as_millis() as u64
}

/// Outside monitor mode the Mac's HDMI output is off while the PC is connected: otherwise the
/// monitor's automatic input detection shows the Mac whenever the PC's signal is gone (PC screen
/// asleep, user wakes the Mac first). Re-checked every 2 s, because waking the monitor or
/// re-plugging HDMI brings the display back.
fn hdmi_off() {
    static LAST_ERR: Mutex<String> = Mutex::new(String::new());
    if config::get().mirrored || now_ms().saturating_sub(MONITOR_LEFT_MS.load(Ordering::SeqCst)) < 5000 {
        return;
    }
    let Some(id) = sys::external_display() else { return };
    let r = sys::set_display_enabled(id, false);
    match &r {
        Ok(()) => {
            *HDMI_OFF.lock().unwrap() = Some(id);
            config::update(|c| c.hdmi_off = Some(id));
            status::log(format!("HDMI output off (display {id}): the monitor gets the Mac's signal only in monitor mode"));
        }
        Err(e) if *LAST_ERR.lock().unwrap() != *e => status::log(format!("HDMI output off failed: {e}")),
        Err(_) => {}
    }
    *LAST_ERR.lock().unwrap() = r.err().unwrap_or_default();
}

/// Start: a display switched off by an earlier run (quit, update, crash) is still off. Turn it on,
/// so this run knows it; while the PC is connected, the 2 s check switches it off again. Without
/// a record (versions before 0.5.13, lost config), every id that is not online is tried.
fn hdmi_restore() {
    let online = sys::online_ids();
    let known = config::get().hdmi_off;
    let ids: Vec<u32> = known.map_or_else(|| (1..=32).collect(), |id| vec![id]).into_iter().filter(|i| !online.contains(i)).collect();
    if ids.is_empty() || (known.is_none() && sys::external_display().is_some()) {
        config::update(|c| c.hdmi_off = None); // nothing was left off
        return;
    }
    // unknown ids fail at once (complete error 1001)
    let r: Vec<(u32, Result<(), String>)> = ids.into_iter().map(|id| (id, sys::set_display_enabled(id, true))).filter(|x| x.1.is_ok()).collect();
    for _ in 0..30 {
        if sys::external_display().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    status::log(format!("start: HDMI output left off by an earlier run? on: {:?} (record {known:?}, turned on {r:?})", sys::external_display()));
    config::update(|c| c.hdmi_off = None);
}

/// MirrorMagic quits: give the Mac its monitor back (macOS would keep it off).
pub fn shutdown() {
    drop(AUDIO_DEVS.lock().unwrap().take());
    hdmi_on("MirrorMagic quits");
}

/// Monitor mode (or no PC for a while): turn the HDMI output on and wait until it is online.
fn hdmi_on(why: &str) {
    let Some(id) = HDMI_OFF.lock().unwrap().take() else { return };
    let r = sys::set_display_enabled(id, true);
    config::update(|c| c.hdmi_off = None);
    for _ in 0..30 {
        if sys::external_display().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    status::log(format!("HDMI output on ({why}): {r:?}, online: {}", sys::external_display().is_some()));
}

/// Monitor mode shows the Mac's main screen: mirror it onto the shared monitor while active.
fn mirror(on: bool) {
    if config::get().mirrored == on {
        return;
    }
    status::log(format!("mirror {on}"));
    if !on {
        MONITOR_LEFT_MS.store(now_ms(), Ordering::SeqCst); // the DDC switch back still needs HDMI
    }
    match sys::set_mirror(on) {
        Ok(()) => {
            config::update(|c| c.mirrored = on);
            // macOS applies the new layout asynchronously; wait (max 2 s) so the cursor area is right
            for _ in 0..20 {
                if sys::displays().iter().any(|d| d.builtin) != on {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // Leaving monitor mode: the shared monitor shows the PC again, so a cursor left on its
            // area would be invisible. Put it in the middle of the laptop screen.
            if let (false, Some(inj), Some((edge, _))) = (on, INJ.get(), sys::layout(&sys::displays())) {
                let (x, y) = edge.center();
                inj.lock().unwrap_or_else(|e| e.into_inner()).warp(x, y);
            }
        }
        Err(e) => {
            status::log(format!("mirror {on} failed: {e}"));
            status::update(|s| s.last_error = format!("could not {} the screen: {e}", if on { "mirror" } else { "unmirror" }));
        }
    }
}

/// While the PC moves the cursor: has the display layout changed since it was read (macOS still
/// settling after mirroring, HDMI re-plugged, resolution changed)? Checked at most every 500 ms.
fn displays_changed() -> bool {
    static LAST_MS: AtomicU64 = AtomicU64::new(0);
    let now = T0.get().unwrap().elapsed().as_millis() as u64;
    if now.saturating_sub(LAST_MS.load(Ordering::Relaxed)) < 500 {
        return false;
    }
    LAST_MS.store(now, Ordering::Relaxed);
    let ds = sys::displays();
    let Some((edge, monitor)) = sys::layout(&ds) else { return false };
    let changed = sec().displays != Displays { edge, monitor };
    if changed {
        status::log(format!("displays changed while in control: {ds:?}"));
    }
    changed
}

fn set_control(c: &str) {
    let c = c.to_string();
    status::update(|s| s.control = c);
}

fn handle(m: Msg, utx: &mpsc::UnboundedSender<Msg>) {
    match m {
        Msg::Enter { mode, side, along } => {
            if mode == Mode::Monitor {
                hdmi_on("monitor mode");
            }
            mirror(mode == Mode::Monitor);
            let info = refresh(); // displays may have changed (HDMI plugged, lid closed)
            let _ = utx.send(info);
            let (x, y) = sec().enter(mode, side, along);
            inj().warp(x, y);
            status::log(format!("enter {mode:?} at {x:.0},{y:.0} in {:?}", sec().displays));
            set_control(if mode == Mode::Monitor { "monitor" } else { "edge" });
        }
        Msg::Motion { dx, dy } => {
            if displays_changed() {
                let _ = utx.send(refresh());
            }
            let os = inject::cursor();
            let effect = sec().motion(dx, dy, os);
            match effect {
                Some(Effect::MoveTo { x, y, dx, dy }) => inj().move_to(x, y, dx, dy),
                Some(Effect::Leave { along, park }) => {
                    inj().release_all();
                    inj().warp(park.0, park.1); // visible for the Mac's own trackpad
                    inj().nudge(park.0, park.1);
                    let tx = utx.clone();
                    clipboard::hand_over(move |m| drop(tx.send(m)));
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
            if sec().is_active() {
                let tx = utx.clone();
                clipboard::hand_over(move |m| drop(tx.send(m))); // hotkey back to the PC
            }
            sec().exit();
            inj().release_all();
            mirror(false);
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
        Msg::FilesOffer { id, count, bytes, names } => {
            // files copied on the PC: fetch them on a separate connection, then put them on the clipboard
            status::log(format!("clipboard: PC offers {count} file(s) {names:?}"));
            status::update(|s| s.clipboard = format!("receiving {count} file(s)…"));
            RT.get().unwrap().spawn(async move {
                let r: Result<(), secure::Error> = async {
                    let (mut tx, mut rx, _, _) = open(Channel::Files).await?;
                    tx.send(&Msg::FilesPull { id }).await?;
                    clipboard::receive_files(&mut rx, id, bytes).await
                }
                .await;
                if let Err(e) = r {
                    status::log(format!("clipboard: file transfer failed: {e}"));
                    status::update(|s| s.clipboard = format!("file transfer failed: {e}"));
                }
            });
        }
        Msg::AudioDevices { inputs, outputs } => {
            status::log(format!("audio: PC offers {inputs:?} / {outputs:?}"));
            *PC_AUDIO.lock().unwrap() = Some((inputs, outputs));
        }
        Msg::PcDisplay { on } => {
            status::log(format!("PC screen {}", if on { "on" } else { "off" }));
            PC_DISPLAY_OFF.store(!on, Ordering::SeqCst);
            if !on && !SLEEP_WATCH.swap(true, Ordering::SeqCst) {
                std::thread::spawn(sleep_displays_when_idle);
            }
        }
        Msg::FilesPushRequest { id } => {
            // the PC wants the files this Mac offered
            let Some(plan) = clipboard::outgoing(id) else { return };
            RT.get().unwrap().spawn(async move {
                let r: Result<(), secure::Error> = async {
                    let (mut tx, _rx, _, _) = open(Channel::Files).await?;
                    tx.send(&Msg::FilesPush { id }).await?;
                    mm_core::files::send(&mut tx, &plan).await?;
                    tokio::time::sleep(Duration::from_secs(1)).await; // let the last bytes drain
                    Ok(())
                }
                .await;
                status::log(format!("clipboard: pushed files to the PC: {r:?}"));
            });
        }
        Msg::Clipboard { text, html, png } => {
            std::thread::spawn(move || clipboard::receive(text, html, png));
        }
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
    use mm_core::camdemand::{Cmd, Demand};
    let rt = RT.get().unwrap();
    let in_use = || sys::cameras().iter().any(|(n, running)| *running && n.contains("OBS Virtual Camera"));
    let mut d = Demand::default();
    let mut task: Option<tokio::task::JoinHandle<()>> = None;
    // camera-control events from the macOS log: who controls a camera, and when it is released
    let (ltx, lrx) = std::sync::mpsc::channel::<i64>();
    camlog::follow(ltx);
    // start from a known state: OBS may still run its virtual camera from before (counts as "in use")
    if obs::running() {
        let _ = rt.block_on(obs::set_virtualcam(false));
        std::thread::sleep(Duration::from_millis(500));
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let (auto, manual) = (config::get().camera_auto, CAM_MANUAL.load(Ordering::SeqCst));
        for pid in lrx.try_iter() {
            // the log works: no need to interrupt calls with frequent probes
            d.probe_every_ms = mm_core::camdemand::PROBE_WITH_LOG_MS;
            if pid <= 0 {
                if d.on {
                    status::log("camera: an app released a camera, checking whether anyone still watches");
                }
                d.release_hint();
            } else if Some(pid) != camlog::obs_pid() {
                status::log(format!("camera: {} controls a camera", camlog::process_name(pid)));
            }
        }
        let cmd = match d.poll(1000, auto && in_use(), manual) {
            Cmd::Probe => {
                // OBS feeding its own virtual camera counts as "in use": pause it to see if an app watches
                let _ = rt.block_on(obs::set_virtualcam(false));
                std::thread::sleep(Duration::from_millis(500));
                let watching = in_use();
                status::log(format!("camera probe: app watching = {watching}"));
                d.probe_result(watching)
            }
            c => c,
        };
        match cmd {
            Cmd::Start => {
                status::log(format!("camera: an app opened it (devices: {:?})", sys::cameras()));
                set_camera_wanted(true, &mut task);
                rt.spawn(async { setup_obs(true).await });
            }
            Cmd::Resume => {
                if let Err(e) = rt.block_on(obs::set_virtualcam(true)) {
                    status::log(format!("camera: OBS resume failed: {e}"));
                    rt.spawn(async { setup_obs(true).await });
                }
            }
            Cmd::Stop => {
                status::log("camera: no app watches any more, PC camera off");
                set_camera_wanted(false, &mut task);
                status::update(|s| s.obs.virtualcam = false);
            }
            _ => {}
        }
    }
}

fn set_camera_wanted(want: bool, task: &mut Option<tokio::task::JoinHandle<()>>) {
    CAM_WANTED.store(want, Ordering::SeqCst);
    status::update(|s| s.camera_wanted = want);
    send(Msg::CameraWanted { on: want });
    if want {
        *task = Some(RT.get().unwrap().spawn(camera_stream()));
    } else if let Some(t) = task.take() {
        t.abort();
        let _ = FRAMES.get().unwrap().send(None);
    }
}

async fn camera_stream() {
    while CAM_WANTED.load(Ordering::SeqCst) {
        match open(Channel::Camera).await {
            Ok((_tx, mut rx, _, _)) => {
                status::log("camera stream: connected");
                let mut frames = 0u64;
                // a dead connection (PC asleep, Wi-Fi roam) must not freeze OBS on the last frame
                while let Ok(Ok(m)) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
                    if let Msg::Frame { jpeg } = m {
                        frames += 1;
                        if frames == 1 || frames % 300 == 0 {
                            status::log(format!("camera stream: {frames} frames"));
                        }
                        let _ = FRAMES.get().unwrap().send(Some(Arc::new(jpeg)));
                    }
                }
                status::log(format!("camera stream: ended after {frames} frames"));
            }
            Err(e) => status::log(format!("camera stream: {e}")),
        }
        let _ = FRAMES.get().unwrap().send(None);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// ---------------------------------------------------------------- PC microphones / speakers

/// The audio devices; removed on quit (coreaudiod would keep them, with a dead tap).
static AUDIO_DEVS: Mutex<Option<vaudio::Devices>> = Mutex::new(None);
/// The PC's audio devices (inputs, outputs) as last reported; the Mac offers each one.
static PC_AUDIO: Mutex<Option<(Vec<String>, Vec<String>)>> = Mutex::new(None);

type Want = (Option<String>, Option<String>);

/// Offers the PC's microphones and speakers as Mac devices (vaudio.rs) and connects one to the
/// PC while an app on the Mac uses it.
fn audio_watch() {
    // a process is a Core Audio object only once it has talked to the audio system
    let mut me = vaudio::own_process_object();
    if me == 0 {
        let stop = Arc::new(AtomicBool::new(false));
        let r = crate::audio::play(48000, None, stop.clone());
        std::thread::sleep(Duration::from_millis(300));
        stop.store(true, Ordering::SeqCst);
        me = vaudio::own_process_object();
        if me == 0 {
            return status::log(format!("audio devices: no Core Audio process object ({:?})", r.err()));
        }
    }
    let (wtx, _) = watch::channel::<Want>((None, None));
    let mut task: Option<tokio::task::JoinHandle<()>> = None;
    let mut offered = None;
    let (mut mics, mut speakers) = (vec![], vec![]);
    let mut last: Want = (None, None);
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let list = PC_AUDIO.lock().unwrap().clone();
        if list.is_some() && list != offered {
            let (inputs, outputs) = list.clone().unwrap();
            let mut devs = AUDIO_DEVS.lock().unwrap();
            drop(devs.take()); // the PC's list changed: offer the new one
            let (d, log) = vaudio::create(me, &inputs, &outputs);
            status::log(format!("audio devices: {log:?}"));
            (mics, speakers) = (d.mics.clone(), d.speakers.clone());
            *devs = Some(d);
            offered = list;
        }
        let used = |l: &[(u32, String)], input| l.iter().find(|(d, _)| vaudio::used_by_others(*d, input, me)).map(|x| x.1.clone());
        let want: Want = (used(&mics, true), used(&speakers, false));
        if want != last {
            status::log(format!("audio: Mac apps use PC microphone {:?}, PC speakers {:?}", want.0, want.1));
            status::update(|s| s.mic_detail = format!("{} · {}", want.0.as_deref().unwrap_or("no PC microphone in use"), want.1.as_deref().unwrap_or("no PC speakers in use")));
            last = want.clone();
        }
        let active = want != (None, None);
        let _ = wtx.send(want);
        if active && task.as_ref().is_none_or(|t| t.is_finished()) && status::get().connected && PEER_VERSION.load(Ordering::SeqCst) >= 7 {
            task = Some(RT.get().unwrap().spawn(audio_stream(wtx.subscribe())));
        }
    }
}

/// One audio connection to the PC while a PC device is used: plays the PC microphone through
/// the muted taps on this process, and sends what apps play to a PC output to the PC.
async fn audio_stream(mut want: watch::Receiver<Want>) {
    let Ok((mut tx, rx, _, _)) = open(Channel::Audio).await.map_err(|e| status::log(format!("audio: {e}"))) else { return };
    status::log("audio: connected");
    let (reader, mut rx) = crate::audio::reader(rx);
    let (ctx, mut crx) = mpsc::channel::<Msg>(64);
    let mut capture: Option<(String, Arc<AtomicBool>)> = None;
    let mut player = crate::audio::Player::default();
    let mut sent: Option<Want> = None;
    let r: Result<(), secure::Error> = loop {
        let w = want.borrow_and_update().clone();
        if sent.as_ref() != Some(&w) {
            if w == (None, None) {
                break Ok(());
            }
            if let Err(e) = tx.send(&Msg::AudioWant { mic: w.0.clone(), speaker: w.1.clone() }).await {
                break Err(e);
            }
            // read the tap of the PC output in use (switch when the app picks another one)
            if capture.as_ref().map(|c| &c.0) != w.1.as_ref() {
                if let Some((_, s)) = capture.take() {
                    s.store(true, Ordering::SeqCst);
                }
                if let Some(pc) = w.1.clone() {
                    let stop = Arc::new(AtomicBool::new(false));
                    let (s, ctx, dev) = (stop.clone(), ctx.clone(), vaudio::reader_name(&pc));
                    let r = tokio::task::spawn_blocking(move || crate::audio::capture(Some(dev), s, move |rate, pcm| drop(ctx.try_send(Msg::Audio { rate, pcm })))).await;
                    status::log(format!("audio: sound for {pc} from the tap: {r:?}"));
                    capture = Some((pc, stop));
                }
            }
            if w.0.is_none() {
                player.stop();
            }
            sent = Some(w);
        }
        tokio::select! {
            _ = want.changed() => {}
            m = crx.recv() => if let Some(m) = m { if let Err(e) = tx.send(&m).await { break Err(e) } },
            m = rx.recv() => match m.unwrap_or(Err("closed".into())) {
                Ok(Msg::Audio { rate, pcm }) => if let Some(r) = player.push(rate, pcm) {
                    status::log(format!("audio: PC microphone at {rate} Hz: {r:?}"));
                },
                Ok(_) => {}
                Err(e) => break Err(e),
            },
        }
    };
    reader.abort();
    if let Some((_, s)) = capture {
        s.store(true, Ordering::SeqCst);
    }
    status::log(format!("audio: ended {r:?}"));
}
