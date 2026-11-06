//! PC (primary) runtime: hook events → state machine → actions; TCP server for the Mac.
use crate::{clipboard, config, ddc, status, win};
use mm_core::keymap::Hotkey;
use mm_core::primary::{Action, Out, Primary, PrimaryConfig, Rect, State};
use mm_core::proto::{Channel, Mode, Msg, Side, DEFAULT_PORT, PROTOCOL_VERSION};
use mm_core::secure;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;

static PRIMARY: OnceLock<Mutex<Primary>> = OnceLock::new();
static CTRL_TX: Mutex<Option<mpsc::UnboundedSender<Msg>>> = Mutex::new(None);
static CAM_TX: Mutex<Option<mpsc::Sender<Vec<u8>>>> = Mutex::new(None);
static CAPTURE: Mutex<Option<win::camera::Capture>> = Mutex::new(None);
static SESSION: AtomicU64 = AtomicU64::new(0);
static T0: OnceLock<Instant> = OnceLock::new();

fn primary() -> std::sync::MutexGuard<'static, Primary> {
    PRIMARY.get().expect("pc::start ran").lock().unwrap_or_else(|e| e.into_inner())
}

/// Builds the state-machine config from settings + the current monitor layout.
fn primary_config() -> PrimaryConfig {
    let c = config::get();
    let monitors = win::monitors();
    // the outermost monitor on the configured side is the one next to the Mac
    let key = |r: &Rect| match c.side {
        Side::Left => r.x,
        Side::Right => -(r.x + r.w),
        Side::Top => r.y,
        Side::Bottom => -(r.y + r.h),
    };
    let screen = monitors.iter().copied().min_by(|a, b| key(a).total_cmp(&key(b))).unwrap_or(Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
    PrimaryConfig { screen, monitors, side: c.side, monitor_hotkey: Hotkey::parse(&c.monitor_hotkey), edge_enabled: c.edge_enabled }
}

pub fn reload_config() {
    let cfg = primary_config();
    status::log(format!("config reloaded: hotkey {:?}", cfg.monitor_hotkey.map(|h| h.format())));
    primary().set_config(cfg);
}

fn run(out: Out) -> bool {
    for a in out.actions {
        match a {
            Action::Send(m) => {
                if matches!(m, mm_core::primary::Msg2::Enter { .. }) {
                    clipboard::hand_over(send); // the clipboard travels with the pointer
                }
                send(m.to_msg())
            }
            Action::Warp(x, y) => {
                win::warp(x, y);
                // Windows may clamp the park point (e.g. the monitor dropped off after a DDC
                // switch): measure deltas from where the cursor really is
                if let Some((cx, cy)) = win::cursor_pos().filter(|p| *p != (x.round(), y.round())) {
                    let mut p = primary();
                    if p.state != State::Local {
                        p.set_park(cx, cy);
                    }
                }
            }
            Action::HideCursor(h) => win::set_cursor_hidden(h),
            Action::SwitchMonitor(to_mac) => switch_monitor(to_mac),
            Action::Status => publish(),
        }
    }
    out.block
}

fn publish() {
    let p = primary();
    let control = match p.state {
        State::Local => "local",
        State::Remote { mode: Mode::Edge } => "edge",
        State::Remote { mode: Mode::Monitor } => "monitor",
    };
    let on_mac = p.monitor_on_mac;
    drop(p);
    status::update(|s| {
        s.control = control.into();
        s.monitor_on_mac = on_mac;
    });
}

// ---------------------------------------------------------------- hook entry points (hook thread)

pub fn on_key(scan: u16, down: bool) -> bool {
    let (out, diag) = {
        let mut p = primary();
        let m = p.mods();
        // diagnostics for hotkey trouble: only Ctrl+Alt combinations are logged, never normal typing
        let diag = (down && m.ctrl && m.alt && !matches!(scan, 0x1D | 0xE01D | 0x38 | 0xE038 | 0x21D))
            .then(|| format!("hotkey check: Ctrl+Alt+scan 0x{scan:X}, configured {:?}", p.cfg.monitor_hotkey.map(|h| h.format())));
        (p.key(scan, down), diag)
    };
    if let Some(d) = diag {
        std::thread::spawn(move || status::log(d));
    }
    run(out)
}
pub fn on_move(x: f64, y: f64) -> bool {
    let out = primary().mouse_move(x, y);
    run(out)
}
pub fn on_button(b: mm_core::proto::Button, down: bool) -> bool {
    let out = primary().button(b, down);
    run(out)
}
pub fn on_wheel(dx: i32, dy: i32) -> bool {
    let out = primary().wheel(dx, dy);
    run(out)
}

// ---------------------------------------------------------------- commands (UI / tray)

pub fn toggle_monitor() {
    let out = primary().toggle_monitor();
    run(out);
}

pub fn release() {
    let out = primary().release_now();
    run(out);
}

pub fn send(m: Msg) {
    if let Some(tx) = CTRL_TX.lock().unwrap().as_ref() {
        let _ = tx.send(m);
    }
}

/// Newest monitor request; DDC threads that were overtaken by a newer toggle do nothing.
static DDC_SEQ: AtomicU64 = AtomicU64::new(0);
static DDC_RUN: Mutex<()> = Mutex::new(());

fn switch_monitor(to_mac: bool) {
    let seq = DDC_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let _g = DDC_RUN.lock().unwrap();
        if DDC_SEQ.load(Ordering::SeqCst) != seq {
            return;
        }
        let c = config::get();
        let result = if to_mac {
            // learn the PC's own input first, so "back" needs no configuration
            if let Some(cur) = ddc::current().filter(|&v| v != c.mac_input) {
                config::update(|c| c.pc_input = cur);
            }
            ddc::set_input(c.mac_input)
        } else {
            // Many monitors ignore DDC on an inactive input, so the Mac (whose input is active
            // now) is asked too; whichever works wins.
            send(Msg::Ddc { value: c.pc_input });
            ddc::set_input(c.pc_input)
        };
        status::update(|s| {
            s.ddc.last = match result {
                Ok(m) => m,
                Err(e) if !to_mac => format!("PC: {e} (asked the Mac to switch)"),
                Err(e) => e,
            }
        });
    });
}

// ---------------------------------------------------------------- start

pub fn start(rt: &tokio::runtime::Handle) {
    T0.get_or_init(Instant::now);
    win::restore_cursors(); // heal a hidden cursor left by a crash
    let _ = PRIMARY.set(Mutex::new(Primary::new(primary_config())));
    std::thread::Builder::new().name("input-hook".into()).spawn(win::hook::run).unwrap();
    rt.spawn(server());
    rt.spawn(discovery());
    std::thread::spawn(maintenance);
    std::thread::spawn(|| {
        let p = ddc::probe();
        status::update(|s| {
            s.ddc.monitor = p.monitor;
            s.ddc.current = p.current;
            s.ddc.inputs = p.inputs;
            if !p.error.is_empty() {
                s.ddc.last = p.error;
            }
        });
    });
    publish();
}

/// Every 2 s while the PC has control: pick up monitor layout changes (resolution, a monitor
/// unplugged by a DDC switch) and forget keys whose release the hook never saw.
fn maintenance() {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if primary().state != State::Local {
            continue;
        }
        let cfg = primary_config();
        let mut p = primary();
        if p.state == State::Local {
            if cfg.screen != p.cfg.screen || cfg.monitors != p.cfg.monitors {
                p.set_config(cfg);
            }
            p.prune(win::key_down, win::button_down);
        }
    }
}

async fn discovery() {
    let Ok(sock) = UdpSocket::bind(("0.0.0.0", DEFAULT_PORT)).await else { return };
    let mut buf = [0u8; 64];
    loop {
        if let Ok((n, from)) = sock.recv_from(&mut buf).await {
            if &buf[..n] == mm_core::DISCOVER_ASK {
                let mut reply = mm_core::DISCOVER_REPLY_PREFIX.to_vec();
                reply.extend_from_slice(win::hostname().as_bytes());
                let _ = sock.send_to(&reply, from).await;
            }
        }
    }
}

async fn server() {
    let l = match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)).await {
        Ok(l) => l,
        Err(e) => return status::update(|s| s.last_error = format!("port {DEFAULT_PORT} unavailable: {e}")),
    };
    loop {
        if let Ok((s, addr)) = l.accept().await {
            tokio::spawn(async move {
                if let Err(e) = connection(s).await {
                    log::info!("connection from {addr} ended: {e}");
                }
            });
        }
    }
}

async fn connection(s: TcpStream) -> Result<(), secure::Error> {
    let addr = s.peer_addr()?;
    let key = config::pairing_key().ok_or("no pairing key")?;
    let (mut tx, mut rx) = secure::accept(s, &key).await.inspect_err(|_| {
        status::update(|s| s.last_error = format!("refused {}: wrong pairing key", addr.ip()));
    })?;
    let hello = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.map_err(|_| "no hello")??;
    let Msg::Hello { version, name, channel } = hello else { return Err("expected hello".into()) };
    tx.send(&Msg::Hello { version: PROTOCOL_VERSION, name: win::hostname(), channel }).await?;
    if version != PROTOCOL_VERSION {
        status::update(|s| s.last_error = format!("{name} runs an incompatible MirrorMagic version"));
        return Err("version mismatch".into());
    }
    match channel {
        Channel::Control => control(tx, rx, name, addr.ip().to_string()).await,
        Channel::Camera => camera_channel(tx, rx).await,
        Channel::Files => files_channel(tx, rx).await,
    }
}

async fn control(mut tx: secure::Sender, mut rx: secure::Receiver, name: String, addr: String) -> Result<(), secure::Error> {
    let id = SESSION.fetch_add(1, Ordering::SeqCst) + 1;
    let (utx, mut urx) = mpsc::unbounded_channel::<Msg>();
    *CTRL_TX.lock().unwrap() = Some(utx.clone()); // a newer Mac session replaces an old one
    status::update(|s| {
        s.connected = true;
        s.peer = name;
        s.peer_addr = addr;
        s.last_error.clear();
    });
    // input is handed over only once the Mac reports it can inject (PeerInfo, Accessibility granted)

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
                let t = T0.get().unwrap().elapsed().as_micros() as u64;
                if utx.send(Msg::Ping { t }).is_err() {
                    break;
                }
            }
        })
    };
    let result: Result<(), secure::Error> = async {
        loop {
            // pings go out every 2 s, so silence for 7 s means the Mac is gone (sleep, Wi-Fi)
            let m = tokio::time::timeout(Duration::from_secs(7), rx.recv()).await.map_err(|_| "peer timed out")??;
            match m {
                Msg::Leave { along } => {
                    let out = primary().on_leave(along);
                    run(out);
                }
                Msg::PeerInfo { displays, can_inject } => {
                    status::update(|s| {
                        s.displays = displays;
                        s.can_inject = can_inject;
                    });
                    let ready = primary().connected;
                    if can_inject != ready {
                        let out = if can_inject { primary().connected() } else { primary().disconnected() };
                        run(out);
                    }
                }
                Msg::DdcResult { ok, detail } => status::update(|s| s.ddc.last = if ok { format!("Mac: {detail}") } else { format!("Mac could not switch: {detail}") }),
                Msg::Clipboard { text, html, png } => {
                    std::thread::spawn(move || clipboard::receive(text, html, png));
                }
                Msg::CameraWanted { on } => camera_wanted(on),
                Msg::FilesOffer { id, count, bytes, names } => {
                    status::log(format!("clipboard: Mac offers {count} file(s) {names:?}"));
                    status::update(|s| s.clipboard = format!("receiving {count} file(s)…"));
                    PENDING_FILES.lock().unwrap().insert(id, bytes);
                    let _ = utx.send(Msg::FilesPushRequest { id }); // only the Mac can dial: it pushes
                }
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
    }
    .await;
    writer.abort();
    pinger.abort();
    if SESSION.load(Ordering::SeqCst) == id {
        *CTRL_TX.lock().unwrap() = None;
        let out = primary().disconnected();
        run(out);
        camera_wanted(false);
        status::update(|s| {
            s.connected = false;
            s.rtt_ms = None;
            s.can_inject = false;
        });
    }
    result
}

// ---------------------------------------------------------------- camera

/// Generation of the current capture; callbacks from an older capture thread are ignored.
static CAM_GEN: AtomicU64 = AtomicU64::new(0);
/// Generation of the current camera channel, so an old channel can't clear a newer one's sender.
static CAM_CHAN: AtomicU64 = AtomicU64::new(0);
static AUD_TX: Mutex<Option<mpsc::Sender<Msg>>> = Mutex::new(None);
/// Stops the running microphone capture.
static MIC_STOP: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> = Mutex::new(None);

/// The PC microphone streams together with the camera (an app on the Mac opened it).
fn mic_wanted(on: bool) {
    let mut m = MIC_STOP.lock().unwrap();
    if let Some(stop) = m.take() {
        stop.store(true, Ordering::SeqCst);
    }
    if !on || !config::get().mic {
        return;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    *m = Some(stop.clone());
    std::thread::spawn(move || {
        let r = crate::audio::capture(stop, |rate, pcm| {
            if let Some(tx) = AUD_TX.lock().unwrap().as_ref() {
                let _ = tx.try_send(Msg::Audio { rate, pcm });
            }
        });
        status::log(format!("microphone: {r:?}"));
        status::update(|s| s.mic_detail = r.unwrap_or_else(|e| format!("microphone failed: {e}")));
    });
}

fn camera_wanted(on: bool) {
    status::update(|s| s.camera_wanted = on);
    status::log(format!("camera wanted: {on}"));
    mic_wanted(on);
    let mut cap = CAPTURE.lock().unwrap();
    if !on {
        *cap = None; // Drop stops the capture thread → camera light off
        CAM_GEN.fetch_add(1, Ordering::SeqCst);
        return;
    }
    if cap.is_some() {
        return;
    }
    let c = config::get();
    let gen = CAM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    *cap = Some(win::camera::start(
        c.camera_width,
        c.camera_height,
        c.camera_fps,
        |jpeg| {
            if let Some(tx) = CAM_TX.lock().unwrap().as_ref() {
                let _ = tx.try_send(jpeg); // drop frames rather than queue latency
            }
        },
        move |on, detail| {
            if CAM_GEN.load(Ordering::SeqCst) != gen {
                return; // a newer capture owns the status
            }
            if !on {
                // failed or stopped: free the slot so the next request starts afresh
                std::thread::spawn(move || {
                    let mut cap = CAPTURE.lock().unwrap();
                    if CAM_GEN.load(Ordering::SeqCst) == gen {
                        *cap = None;
                    }
                });
            }
            send(Msg::CameraState { on, detail: detail.clone() });
            status::update(|s| {
                s.camera_on = on;
                s.camera_detail = detail;
            });
        },
    ));
}

async fn camera_channel(mut tx: secure::Sender, mut rx: secure::Receiver) -> Result<(), secure::Error> {
    let (ftx, mut frx) = mpsc::channel::<Vec<u8>>(2);
    let (atx, mut arx) = mpsc::channel::<Msg>(64);
    let me = CAM_CHAN.fetch_add(1, Ordering::SeqCst) + 1;
    *CAM_TX.lock().unwrap() = Some(ftx);
    *AUD_TX.lock().unwrap() = Some(atx);
    let r = loop {
        tokio::select! {
            biased; // audio first: 10 ms chunks must not wait behind video frames
            a = arx.recv() => if let Some(m) = a { if let Err(e) = tx.send(&m).await { break Err(e) } },
            f = frx.recv() => match f {
                Some(jpeg) => if let Err(e) = tx.send(&Msg::Frame { jpeg }).await { break Err(e) },
                None => break Ok(()),
            },
            m = rx.recv() => if let Err(e) = m { break Err(e) },
        }
    };
    let mut tx = CAM_TX.lock().unwrap();
    if CAM_CHAN.load(Ordering::SeqCst) == me {
        *tx = None;
        *AUD_TX.lock().unwrap() = None;
    }
    r
}

// ---------------------------------------------------------------- clipboard files

/// Offers from the Mac we asked it to push (id → size).
static PENDING_FILES: Mutex<std::collections::BTreeMap<u64, u64>> = Mutex::new(std::collections::BTreeMap::new());

/// A files connection from the Mac: it pulls our offer, or pushes its own.
async fn files_channel(mut tx: secure::Sender, mut rx: secure::Receiver) -> Result<(), secure::Error> {
    match rx.recv().await? {
        Msg::FilesPull { id } => {
            let plan = clipboard::outgoing(id).ok_or("unknown file offer")?;
            status::log(format!("clipboard: sending {} file(s) to the Mac", plan.items.len()));
            mm_core::files::send(&mut tx, &plan).await?;
            let _ = rx.recv().await; // wait until the Mac closes: everything arrived
            Ok(())
        }
        Msg::FilesPush { id } => {
            let bytes = PENDING_FILES.lock().unwrap().remove(&id).ok_or("files we did not ask for")?;
            let r = clipboard::receive_files(&mut rx, id, bytes).await;
            if let Err(e) = &r {
                status::update(|s| s.clipboard = format!("file transfer failed: {e}"));
            }
            r
        }
        _ => Err("unexpected files message".into()),
    }
}
