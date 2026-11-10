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

/// The window records a new hotkey: the current one must not fire meanwhile (saving reloads it).
pub fn hotkey_recording(on: bool) {
    if on {
        primary().cfg.monitor_hotkey = None;
    } else {
        reload_config();
    }
}

/// Quit: give the monitor and the input back first, then the cursor scheme.
pub fn shutdown() {
    let monitor = primary().monitor_on_mac;
    release();
    if monitor {
        std::thread::sleep(Duration::from_millis(1500)); // the DDC switch runs on its own thread
    }
    win::restore_cursors();
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
    let mut p = primary();
    if p.state != State::Local {
        // Measure from where the hidden cursor really is (GetCursorPos inside the hook = before
        // this event). Moves that slip past the hook (the 2 s re-install, a hook Windows dropped)
        // shift it, and deltas from the stored park point would then carry that shift forever:
        // the Mac cursor got pulled left or right on its own.
        if let Some((cx, cy)) = win::cursor_pos() {
            let (px, py) = p.park();
            if (cx - px).abs() + (cy - py).abs() >= 2.0 {
                // evidence in the log: without this correction, the offset would pull the Mac cursor
                static LOGGED: AtomicU64 = AtomicU64::new(0);
                let now = T0.get().unwrap().elapsed().as_secs() + 60;
                if now - LOGGED.load(Ordering::Relaxed) >= 60 {
                    LOGGED.store(now, Ordering::Relaxed);
                    let d = (cx - px, cy - py);
                    std::thread::spawn(move || status::log(format!("hidden cursor slipped by {:.0},{:.0} px: corrected (would have pulled the Mac cursor)", d.0, d.1)));
                }
            }
            p.set_park(cx, cy);
        }
    }
    let out = p.mouse_move(x, y);
    drop(p);
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

/// Pause: monitor back to the PC first (the Mac still listens and switches it), then input.
pub fn release_for_pause() {
    release(); // in monitor mode this toggles the monitor back as well
}

/// The session ended while monitor mode was on. Usually the monitor still shows the Mac (Wi-Fi
/// drop: the reconnect re-enters monitor mode), but a Mac pause switches it to the PC itself:
/// then the flag must go, or the next connection jumps back into monitor mode.
fn recheck_monitor_after_drop() {
    std::thread::sleep(Duration::from_secs(3)); // DisplayPort link up, DDC answers on the active input
    let pc = config::get().pc_input;
    let on_mac = primary().monitor_on_mac; // not across the DDC read: the input hook locks it too
    if on_mac && ddc::current() == Some(pc) {
        primary().monitor_on_mac = false;
        status::log("session ended in monitor mode, but the monitor shows the PC: monitor mode off");
        status::update(|s| s.monitor_on_mac = false);
    }
}

/// Protocol version agreed with the connected Mac.
static PEER_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(PROTOCOL_VERSION);

pub fn send(m: Msg) {
    if matches!(m, Msg::PcDisplay { .. }) && PEER_VERSION.load(Ordering::SeqCst) < 4 {
        return;
    }
    if matches!(m, Msg::FilesOffer { .. }) && PEER_VERSION.load(Ordering::SeqCst) < 3 {
        status::update(|s| s.clipboard = "files need MirrorMagic 0.5 on the Mac".into());
        return;
    }
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
            let r = ddc::set_input(c.mac_input);
            if r.is_ok() {
                std::thread::spawn(move || recheck_mac_input(seq));
            }
            r
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

/// The Mac turns its HDMI output on only when monitor mode starts. A monitor with automatic input
/// detection may find no signal there yet and go back to the PC: then switch once more.
fn recheck_mac_input(seq: u64) {
    std::thread::sleep(Duration::from_secs(4));
    let _g = DDC_RUN.lock().unwrap();
    let c = config::get();
    if DDC_SEQ.load(Ordering::SeqCst) == seq && ddc::current() == Some(c.pc_input) {
        status::log("monitor went back to the PC before the Mac's HDMI signal was there: switching again");
        let _ = ddc::set_input(c.mac_input);
    }
}

// ---------------------------------------------------------------- game mode

/// Called by the hook thread every 500 ms. While a full-screen app (game, video) is in front and
/// the PC has control, the input hooks are removed: input reaches the app with zero added
/// latency, and the pointer cannot cross to the Mac. Returns whether the hooks should be installed.
pub fn hooks_wanted(fullscreen_in_front: Option<String>) -> bool {
    if config::paused() {
        return false; // no crossing, no hotkey: Windows input untouched
    }
    static GAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    // not while the monitor shows the Mac: the hotkey is the way back
    let game = fullscreen_in_front.is_some() && {
        let p = primary();
        p.state == State::Local && !p.monitor_on_mac
    };
    if GAME.swap(game, Ordering::SeqCst) != game {
        std::thread::spawn(move || {
            status::log(match fullscreen_in_front {
                Some(class) if game => format!("game mode on: full-screen app in front ({class}), input hooks removed"),
                _ => "game mode off: input hooks back".to_string(),
            });
            status::update(|s| s.game_mode = game);
        });
    }
    !game
}

// ---------------------------------------------------------------- display off/on (user away)

/// Windows turned the display on after it was off, i.e. the user came back.
fn display_came_back(prev: u32, now: u32) -> bool {
    prev == 0 && now == 1
}

/// While Windows has the display off, monitors with automatic input detection jump to the Mac's
/// HDMI signal and stay there. When the display comes back, the PC input is restored, unless
/// monitor mode deliberately shows the Mac.
fn on_display_state(state: u32) {
    static LAST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let prev = LAST.swap(state, Ordering::SeqCst);
    if state == 0 && prev != 0 {
        std::thread::spawn(|| status::log("display off (Windows power saving)"));
        send(Msg::PcDisplay { on: false }); // the Mac sleeps its displays once it is idle too
    }
    if display_came_back(prev, state) {
        send(Msg::PcDisplay { on: true });
        std::thread::spawn(restore_pc_input);
    }
}

fn restore_pc_input() {
    std::thread::sleep(Duration::from_secs(2)); // let the DisplayPort link come up first
    if primary().monitor_on_mac {
        return status::log("display on: monitor mode shows the Mac, input left alone");
    }
    let pc = config::get().pc_input;
    match ddc::current() {
        Some(v) if v == pc => status::log("display on: the monitor shows the PC"),
        cur => {
            // None: DDC from the PC often fails while another input is active, so ask anyway
            status::log(format!("display on: monitor input {cur:?}, switching back to {}", ddc::input_name(pc)));
            switch_monitor(false);
        }
    }
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
    win::power::watch_display(on_display_state);
    std::thread::spawn(probe_ddc);
    publish();
}

/// Reads the DDC monitors and the shared one's inputs (at start and when another monitor is picked).
pub fn probe_ddc() {
    let p = ddc::probe();
    status::update(|s| {
        s.ddc.monitor = p.monitor;
        s.ddc.monitors = p.monitors;
        s.ddc.inputs = p.inputs;
        if !p.error.is_empty() {
            s.ddc.last = p.error;
        }
    });
}

/// Bumped when the pairing key changes: every connection made with the old key ends.
static KEY_GEN: AtomicU64 = AtomicU64::new(0);

pub fn revoke_sessions() {
    KEY_GEN.fetch_add(1, Ordering::SeqCst);
}

/// Why a running connection must end now (`gen`: KEY_GEN when it was accepted).
fn must_end(gen: u64) -> Option<&'static str> {
    if config::paused() {
        Some("paused")
    } else if KEY_GEN.load(Ordering::SeqCst) != gen {
        Some("pairing key changed")
    } else {
        None
    }
}

/// Every 2 s while the PC has control: pick up monitor layout changes (resolution, a monitor
/// unplugged by a DDC switch) and forget keys whose release the hook never saw.
fn maintenance() {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if primary().state != State::Local {
            // Working on the Mac with this mouse: the hook may swallow the input that would keep
            // Windows awake, and a dark PC screen lets the monitor jump to the Mac's HDMI signal.
            win::keep_display_on();
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
                    // once per kind: a Mac with a wrong key retries every 2 s
                    static LAST: Mutex<String> = Mutex::new(String::new());
                    let line = format!("connection from {} ended: {e}", addr.ip());
                    if std::mem::replace(&mut *LAST.lock().unwrap(), line.clone()) != line {
                        status::log(line);
                    }
                }
            });
        }
    }
}

async fn connection(s: TcpStream) -> Result<(), secure::Error> {
    let addr = s.peer_addr()?;
    // strangers get at most this many handshakes at a time (each holds buffers for up to 5 s)
    static HANDSHAKES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);
    let permit = HANDSHAKES.try_acquire().map_err(|_| "too many connections at once")?;
    let gen = KEY_GEN.load(Ordering::SeqCst);
    let key = config::pairing_key().ok_or("no pairing key")?;
    let (mut tx, mut rx) = secure::accept(s, &key).await.inspect_err(|e| {
        // a port scan or a timeout lands here too: say what is known, and not over a working session
        let why = if e.to_string().contains("timed out") { "no handshake" } else { "handshake failed (wrong pairing key?)" };
        status::update(|s| {
            if !s.connected {
                s.last_error = format!("refused {}: {why}", addr.ip());
            }
        });
    })?;
    let hello = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.map_err(|_| "no hello")??;
    drop(permit);
    let Msg::Hello { version, name, channel } = hello else { return Err("expected hello".into()) };
    // answer with the lower version: an older Mac keeps working while it waits for its update
    let agreed = version.min(PROTOCOL_VERSION);
    tx.send(&Msg::Hello { version: agreed, name: win::hostname(), channel }).await?;
    if config::paused() {
        return Err("paused".into()); // after the hello, so the Mac reports a lost connection, not a wrong key
    }
    if agreed < mm_core::proto::MIN_COMPATIBLE_VERSION {
        status::update(|s| s.last_error = format!("{name} runs an incompatible MirrorMagic version"));
        return Err("version mismatch".into());
    }
    if channel == Channel::Control {
        PEER_VERSION.store(agreed, Ordering::SeqCst);
        status::log(format!("{name} connected with protocol {agreed}"));
    }
    match channel {
        Channel::Control => control(tx, rx, name, addr.ip().to_string(), gen).await,
        Channel::Camera => camera_channel(tx, rx, gen).await,
        Channel::Files => files_channel(tx, rx).await, // shortcut: a transfer in flight finishes after a key change
        Channel::Audio if agreed >= 7 => audio_channel(tx, rx, gen).await,
        Channel::Audio => Err("audio needs MirrorMagic 0.5.14 on both sides".into()),
    }
}

async fn control(mut tx: secure::Sender, mut rx: secure::Receiver, name: String, addr: String, gen: u64) -> Result<(), secure::Error> {
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
    if PEER_VERSION.load(Ordering::SeqCst) >= 7 {
        // the Mac offers each PC microphone and speaker to its apps under its own name
        let utx = utx.clone();
        tokio::task::spawn_blocking(move || {
            let (inputs, outputs) = crate::audio::devices();
            status::log(format!("audio: offering {inputs:?} / {outputs:?} to the Mac"));
            let _ = utx.send(Msg::AudioDevices { inputs, outputs });
        });
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
                let t = T0.get().unwrap().elapsed().as_micros() as u64;
                if utx.send(Msg::Ping { t }).is_err() {
                    break;
                }
            }
        })
    };
    let mut first = true;
    let result: Result<(), secure::Error> = async {
        loop {
            // pings go out every 2 s, so silence for 7 s means the Mac is gone (sleep, Wi-Fi)
            let m = tokio::time::timeout(Duration::from_secs(7), rx.recv()).await.map_err(|_| "peer timed out")??;
            if let Some(why) = must_end(gen) {
                return Err(why.into()); // the Mac pings every 2 s, so this ends within 2 s
            }
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
                    // a session that replaced a live one starts with an idle Mac cursor: enter again
                    if std::mem::take(&mut first) || can_inject != ready {
                        let out = if can_inject { primary().connected() } else { primary().disconnected() };
                        run(out);
                    }
                }
                Msg::MacActive => {
                    win::keep_display_on(); // the user works on the Mac
                    static LOGGED: AtomicU64 = AtomicU64::new(0);
                    let now = T0.get().unwrap().elapsed().as_secs();
                    if now.saturating_sub(LOGGED.load(Ordering::Relaxed)) >= 600 || LOGGED.load(Ordering::Relaxed) == 0 {
                        LOGGED.store(now.max(1), Ordering::Relaxed);
                        std::thread::spawn(|| status::log("Mac in use: keeping the PC screen on"));
                    }
                }
                Msg::DdcResult { ok, detail } => status::update(|s| s.ddc.last = if ok { format!("Mac: {detail}") } else { format!("Mac could not switch: {detail}") }),
                Msg::Clipboard { text, html, png } => {
                    std::thread::spawn(move || clipboard::receive(text, html, png));
                }
                Msg::CameraWanted { on } => camera_wanted(on),
                Msg::FilesOffer { id, count, bytes, names } if config::get().clipboard => {
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
        if primary().monitor_on_mac {
            std::thread::spawn(recheck_monitor_after_drop);
        }
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

/// What the Mac asked for last (microphone by name, camera): the switches in the window apply to it.
static MIC_WANT: Mutex<Option<String>> = Mutex::new(None);
static CAM_WANT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// "Microphone" or "Camera" was switched in the window: stop or start what the Mac wants now.
pub fn sharing_changed(mic: bool, camera: bool) {
    if mic {
        let want = MIC_WANT.lock().unwrap().clone();
        mic_wanted(want);
    }
    if camera {
        apply_camera();
    }
}

/// A PC microphone (by name) streams while an app on the Mac records from its Mac device.
fn mic_wanted(on: Option<String>) {
    *MIC_WANT.lock().unwrap() = on.clone();
    let mut m = MIC_STOP.lock().unwrap();
    if let Some(stop) = m.take() {
        stop.store(true, Ordering::SeqCst);
    }
    let Some(device) = on.filter(|_| config::get().mic) else { return };
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    *m = Some(stop.clone());
    std::thread::spawn(move || {
        let r = crate::audio::capture(Some(device), stop, |rate, pcm| {
            if let Some(tx) = AUD_TX.lock().unwrap().as_ref() {
                let _ = tx.try_send(Msg::Audio { rate, pcm });
            }
        });
        status::log(format!("microphone: {r:?}"));
        status::update(|s| s.mic_detail = r.unwrap_or_else(|e| format!("microphone failed: {e}")));
    });
}

fn camera_wanted(on: bool) {
    CAM_WANT.store(on, Ordering::SeqCst);
    status::update(|s| s.camera_wanted = on);
    status::log(format!("camera wanted: {on}"));
    apply_camera();
}

fn apply_camera() {
    let wanted = CAM_WANT.load(Ordering::SeqCst);
    let on = wanted && config::get().camera;
    let mut cap = CAPTURE.lock().unwrap();
    if !on {
        *cap = None; // Drop stops the capture thread → camera light off
        CAM_GEN.fetch_add(1, Ordering::SeqCst);
        // said here: the capture thread's own "off" belongs to an old generation and is dropped
        let detail = if wanted { "Camera sharing is switched off on the PC" } else { "" };
        send(Msg::CameraState { on: false, detail: detail.into() });
        status::update(|s| {
            s.camera_on = false;
            s.camera_detail = detail.into();
        });
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

async fn camera_channel(mut tx: secure::Sender, mut rx: secure::Receiver, gen: u64) -> Result<(), secure::Error> {
    let (ftx, mut frx) = mpsc::channel::<Vec<u8>>(2);
    let me = CAM_CHAN.fetch_add(1, Ordering::SeqCst) + 1;
    *CAM_TX.lock().unwrap() = Some(ftx);
    let r = loop {
        tokio::select! {
            f = frx.recv() => match f {
                Some(_) if must_end(gen).is_some() => break Err("ended".into()),
                Some(jpeg) => if let Err(e) = tx.send(&Msg::Frame { jpeg }).await { break Err(e) },
                None => break Ok(()),
            },
            m = rx.recv() => if let Err(e) = m { break Err(e) },
        }
    };
    let mut tx = CAM_TX.lock().unwrap();
    if CAM_CHAN.load(Ordering::SeqCst) == me {
        *tx = None;
    }
    r
}

/// Generation of the current audio channel, so an old channel can't clear a newer one's sender.
static AUD_CHAN: AtomicU64 = AtomicU64::new(0);

/// Opened by the Mac while an app there uses "PC Microphone" or "PC Speakers": the microphone
/// goes out, the Mac's call sound comes in and plays on the PC's default output.
async fn audio_channel(mut tx: secure::Sender, rx: secure::Receiver, gen: u64) -> Result<(), secure::Error> {
    let (reader, mut rx) = crate::audio::reader(rx);
    let (atx, mut arx) = mpsc::channel::<Msg>(64);
    let me = AUD_CHAN.fetch_add(1, Ordering::SeqCst) + 1;
    *AUD_TX.lock().unwrap() = Some(atx);
    let mut speaker = crate::audio::Player::default();
    let mut want: (Option<String>, Option<String>) = (None, None);
    let r = loop {
        // pause and a new key end the sound too (every chunk passes here, 100 a second)
        if let Some(why) = must_end(gen) {
            break Err(why.into());
        }
        tokio::select! {
            a = arx.recv() => match a {
                Some(m) => if let Err(e) = tx.send(&m).await { break Err(e) },
                None => break Ok(()), // a newer audio channel took over: without this the loop spins
            },
            m = rx.recv() => match m.unwrap_or(Err("closed".into())) {
                Ok(Msg::AudioWant { mic, speaker: spk }) => {
                    if want != (mic.clone(), spk.clone()) {
                        status::log(format!("audio: Mac uses microphone {mic:?}, speakers {spk:?}"));
                        if want.0 != mic {
                            mic_wanted(mic.clone());
                        }
                        speaker.set_device(spk.clone());
                        if spk.is_none() {
                            speaker.stop();
                        }
                        want = (mic, spk);
                    }
                }
                Ok(Msg::Audio { rate, pcm }) => {
                    if let Some(r) = speaker.push(rate, pcm) {
                        status::log(format!("audio: Mac sound to the PC speakers at {rate} Hz: {r:?}"));
                    }
                }
                Ok(_) => {}
                Err(e) => break Err(e),
            },
        }
    };
    reader.abort();
    if AUD_CHAN.load(Ordering::SeqCst) == me {
        *AUD_TX.lock().unwrap() = None;
        if want.0.is_some() {
            mic_wanted(None);
        }
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

#[cfg(test)]
mod tests {
    use super::display_came_back;

    #[test]
    fn only_off_to_on_means_the_user_is_back() {
        // Windows: on (1) → dimmed (2) → off (0) → on (1); the first notice repeats the current state
        assert!(!display_came_back(1, 1));
        assert!(!display_came_back(1, 2));
        assert!(!display_came_back(2, 1)); // dimmed screens keep their signal: no input change
        assert!(!display_came_back(2, 0));
        assert!(display_came_back(0, 1));
        assert!(!display_came_back(0, 2));
    }
}
