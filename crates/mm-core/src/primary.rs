//! PC-side KVM state machine. Pure logic: the Windows layer feeds hook events in and executes
//! the returned actions (send, park/hide cursor, switch monitor). Fully unit-tested.
use crate::keymap::{Hotkey, ModTracker, SCAN_SCROLL_LOCK};
use crate::proto::{Button, Mode, Msg, Side};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Clone, Debug)]
pub struct PrimaryConfig {
    /// The PC monitor next to the Mac.
    pub screen: Rect,
    /// All PC monitors (an edge only counts where no other monitor continues).
    pub monitors: Vec<Rect>,
    /// Side of `screen` where the Mac sits.
    pub side: Side,
    pub monitor_hotkey: Option<Hotkey>,
    /// Cross the edge only when the Mac is connected and this is true.
    pub edge_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Send(Msg2),
    /// Move the (hidden) PC cursor to x,y.
    Warp(f64, f64),
    HideCursor(bool),
    /// Switch the shared monitor's input (true = to the Mac).
    SwitchMonitor(bool),
    /// Something user-visible changed.
    Status,
}

/// `Msg` without heap data, so actions stay `Copy` (built into a `Msg` by `to_msg`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Msg2 {
    Enter { mode: Mode, side: Side, along: f32 },
    Exit,
    Motion { dx: i32, dy: i32 },
    Button { button: Button, down: bool },
    Wheel { dx: i32, dy: i32 },
    Key { scan: u16, down: bool },
}

impl Msg2 {
    pub fn to_msg(self) -> Msg {
        match self {
            Msg2::Enter { mode, side, along } => Msg::Enter { mode, side, along },
            Msg2::Exit => Msg::Exit,
            Msg2::Motion { dx, dy } => Msg::Motion { dx, dy },
            Msg2::Button { button, down } => Msg::Button { button, down },
            Msg2::Wheel { dx, dy } => Msg::Wheel { dx, dy },
            Msg2::Key { scan, down } => Msg::Key { scan, down },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Local,
    Remote { mode: Mode },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Pc,
    Mac,
    /// Hotkey keys: swallowed on both sides.
    Nobody,
}

/// Result of feeding one hook event: whether to swallow it, and what to do.
#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub block: bool,
    pub actions: Vec<Action>,
}

pub struct Primary {
    pub cfg: PrimaryConfig,
    pub state: State,
    pub connected: bool,
    pub monitor_on_mac: bool,
    keys: HashMap<u16, Owner>,
    buttons: HashMap<Button, Owner>,
    mods: ModTracker,
    park: (f64, f64),
}

impl Primary {
    pub fn new(cfg: PrimaryConfig) -> Self {
        let park = cfg.screen.center();
        Primary { cfg, state: State::Local, connected: false, monitor_on_mac: false, keys: HashMap::new(), buttons: HashMap::new(), mods: ModTracker::default(), park }
    }

    pub fn set_config(&mut self, cfg: PrimaryConfig) {
        self.park = cfg.screen.center();
        self.cfg = cfg;
    }

    fn local_buttons_held(&self) -> bool {
        self.buttons.values().any(|o| *o == Owner::Pc)
    }

    /// Point beyond the configured edge that no PC monitor covers → the cursor wants to leave.
    fn at_edge(&self, x: f64, y: f64) -> Option<f32> {
        let s = self.cfg.screen;
        let (hit, along, probe) = match self.cfg.side {
            Side::Left => (x <= s.x && y >= s.y && y < s.y + s.h, (y - s.y) / s.h, (s.x - 1.0, y)),
            Side::Right => (x >= s.x + s.w - 1.0 && y >= s.y && y < s.y + s.h, (y - s.y) / s.h, (s.x + s.w, y)),
            Side::Top => (y <= s.y && x >= s.x && x < s.x + s.w, (x - s.x) / s.w, (x, s.y - 1.0)),
            Side::Bottom => (y >= s.y + s.h - 1.0 && x >= s.x && x < s.x + s.w, (x - s.x) / s.w, (x, s.y + s.h)),
        };
        let covered = self.cfg.monitors.iter().any(|m| m.contains(probe.0, probe.1));
        (hit && !covered).then_some(along.clamp(0.0, 1.0) as f32)
    }

    fn enter(&mut self, mode: Mode, along: f32, out: &mut Out) {
        self.state = State::Remote { mode };
        out.actions.push(Action::Warp(self.park.0, self.park.1));
        out.actions.push(Action::HideCursor(true));
        out.actions.push(Action::Send(Msg2::Enter { mode, side: self.cfg.side.opposite(), along }));
        out.actions.push(Action::Status);
    }

    /// Back to the PC. `along` = where the cursor came back over the edge (None: centre).
    fn release(&mut self, along: Option<f32>, out: &mut Out) {
        if self.state == State::Local {
            return;
        }
        self.state = State::Local;
        if self.connected {
            out.actions.push(Action::Send(Msg2::Exit));
        }
        let s = self.cfg.screen;
        let (x, y) = match along.map(|a| a as f64) {
            // land 2 px inside the edge so the next move doesn't bounce straight back
            Some(a) => match self.cfg.side {
                Side::Left => (s.x + 2.0, s.y + a * s.h),
                Side::Right => (s.x + s.w - 3.0, s.y + a * s.h),
                Side::Top => (s.x + a * s.w, s.y + 2.0),
                Side::Bottom => (s.x + a * s.w, s.y + s.h - 3.0),
            },
            None => s.center(),
        };
        out.actions.push(Action::Warp(x, y));
        out.actions.push(Action::HideCursor(false));
        out.actions.push(Action::Status);
    }

    // ------------------------------------------------------------ hook events

    pub fn mouse_move(&mut self, x: f64, y: f64) -> Out {
        let mut out = Out::default();
        match self.state {
            State::Local => {
                if self.connected && self.cfg.edge_enabled && !self.monitor_on_mac && !self.local_buttons_held() {
                    if let Some(along) = self.at_edge(x, y) {
                        self.enter(Mode::Edge, along, &mut out);
                        out.block = true;
                    }
                }
            }
            State::Remote { .. } => {
                out.block = true;
                let (dx, dy) = ((x - self.park.0).round() as i32, (y - self.park.1).round() as i32);
                if dx != 0 || dy != 0 {
                    out.actions.push(Action::Send(Msg2::Motion { dx, dy }));
                }
            }
        }
        out
    }

    pub fn button(&mut self, button: Button, down: bool) -> Out {
        let mut out = Out::default();
        if down {
            let owner = if matches!(self.state, State::Remote { .. }) { Owner::Mac } else { Owner::Pc };
            self.buttons.insert(button, owner);
            if owner == Owner::Mac {
                out.block = true;
                out.actions.push(Action::Send(Msg2::Button { button, down }));
            }
        } else {
            let remote = matches!(self.state, State::Remote { .. });
            match self.buttons.remove(&button) {
                Some(Owner::Pc) => {} // Windows saw the press
                Some(Owner::Mac) => {
                    out.block = true;
                    if remote {
                        out.actions.push(Action::Send(Msg2::Button { button, down }));
                    } // else: the Mac already released everything on Exit
                }
                _ => out.block = remote,
            }
        }
        out
    }

    pub fn wheel(&mut self, dx: i32, dy: i32) -> Out {
        let mut out = Out::default();
        if matches!(self.state, State::Remote { .. }) {
            out.block = true;
            out.actions.push(Action::Send(Msg2::Wheel { dx, dy }));
        }
        out
    }

    pub fn key(&mut self, scan: u16, down: bool) -> Out {
        let mut out = Out::default();
        let is_mod = self.mods.update(scan, down);
        let remote = matches!(self.state, State::Remote { .. });

        if down && !is_mod {
            // Emergency release, works even if the Mac is gone.
            if scan == SCAN_SCROLL_LOCK && remote {
                self.keys.insert(scan, Owner::Nobody);
                self.release(None, &mut out);
                out.block = true;
                return out;
            }
            if self.cfg.monitor_hotkey.is_some_and(|h| h.matches(self.mods.mods(), scan)) && !self.keys.contains_key(&scan) {
                self.keys.insert(scan, Owner::Nobody);
                self.toggle_monitor_inner(&mut out);
                out.block = true;
                return out;
            }
        }

        if down {
            match self.keys.get(&scan).copied() {
                Some(Owner::Nobody) => out.block = true,                       // autorepeat of a hotkey
                Some(Owner::Pc) => out.block = remote,                         // autorepeat of a local key: don't leak it to the Mac
                Some(Owner::Mac) if !remote => out.block = true,               // autorepeat of a key that went to the Mac
                _ if remote => {
                    self.keys.insert(scan, Owner::Mac);
                    out.block = true;
                    out.actions.push(Action::Send(Msg2::Key { scan, down }));
                }
                _ => {
                    self.keys.insert(scan, Owner::Pc);
                }
            }
        } else {
            match self.keys.remove(&scan) {
                Some(Owner::Pc) => {}                                          // Windows saw the press, give it the release
                Some(Owner::Nobody) => out.block = true,
                Some(Owner::Mac) => {
                    out.block = true;
                    if remote {
                        out.actions.push(Action::Send(Msg2::Key { scan, down }));
                    }
                }
                None => {
                    if remote {
                        out.block = true;
                    }
                }
            }
        }
        out
    }

    // ------------------------------------------------------------ commands & network

    /// Monitor-mode toggle (also from the UI/tray).
    pub fn toggle_monitor(&mut self) -> Out {
        let mut out = Out::default();
        self.toggle_monitor_inner(&mut out);
        out
    }

    fn toggle_monitor_inner(&mut self, out: &mut Out) {
        if self.monitor_on_mac {
            self.monitor_on_mac = false;
            out.actions.push(Action::SwitchMonitor(false));
            self.release(None, out);
        } else {
            self.monitor_on_mac = true;
            out.actions.push(Action::SwitchMonitor(true));
            if self.connected {
                // from Local or from Edge mode: re-enter in Monitor mode
                self.state = State::Local;
                self.enter(Mode::Monitor, 0.5, out);
            }
        }
        out.actions.push(Action::Status);
    }

    pub fn connected(&mut self) -> Out {
        self.connected = true;
        let mut out = Out::default();
        match self.state {
            // Mac reconnected while the monitor shows it
            State::Local if self.monitor_on_mac => self.enter(Mode::Monitor, 0.5, &mut out),
            // a new Mac session replaced one that had control: its cursor logic starts inactive,
            // so tell it again where control is instead of injecting blind
            State::Remote { mode } => self.enter(mode, 0.5, &mut out),
            State::Local => {}
        }
        out.actions.push(Action::Status);
        out
    }

    /// The OS put the parked cursor somewhere else than asked (clamped to a monitor that just
    /// disappeared, layout change): deltas must be measured from where it really is.
    pub fn set_park(&mut self, x: f64, y: f64) {
        self.park = (x, y);
    }

    /// Forgets keys/buttons whose release never reached the hook (Ctrl+Alt+Del, UAC, lock
    /// screen, a dropped hook). Call only while Local; `is_down` asks the OS for the real state.
    pub fn prune(&mut self, is_down_key: impl Fn(u16) -> bool, is_down_button: impl Fn(Button) -> bool) {
        if self.state != State::Local {
            return;
        }
        self.keys.retain(|&scan, _| is_down_key(scan));
        self.buttons.retain(|&b, _| is_down_button(b));
        self.mods.retain(&is_down_key);
    }

    pub fn disconnected(&mut self) -> Out {
        self.connected = false;
        let mut out = Out::default();
        self.release(None, &mut out);
        out.actions.push(Action::Status);
        out
    }

    pub fn on_leave(&mut self, along: f32) -> Out {
        let mut out = Out::default();
        if self.state == (State::Remote { mode: Mode::Edge }) {
            self.release(Some(along), &mut out);
        }
        out
    }

    /// Emergency / UI "take control back".
    pub fn release_now(&mut self) -> Out {
        let mut out = Out::default();
        self.release(None, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc() -> Primary {
        let screen = Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 };
        let mut p = Primary::new(PrimaryConfig {
            screen,
            monitors: vec![screen],
            side: Side::Left,
            monitor_hotkey: Hotkey::parse("Ctrl+Alt+M"),
            edge_enabled: true,
        });
        p.connected();
        p
    }

    fn sent(out: &Out) -> Vec<Msg2> {
        out.actions.iter().filter_map(|a| if let Action::Send(m) = a { Some(*m) } else { None }).collect()
    }

    #[test]
    fn edge_enter_motion_and_return() {
        let mut p = pc();
        assert!(!p.mouse_move(100.0, 720.0).block);
        let o = p.mouse_move(0.0, 360.0);
        assert!(o.block);
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Edge, side: Side::Right, along: 0.25 }]);
        assert!(o.actions.contains(&Action::Warp(1280.0, 720.0)));
        assert!(o.actions.contains(&Action::HideCursor(true)));
        // relative motion against the parked cursor
        let o = p.mouse_move(1275.0, 723.0);
        assert!(o.block);
        assert_eq!(sent(&o), vec![Msg2::Motion { dx: -5, dy: 3 }]);
        assert_eq!(sent(&p.mouse_move(1280.0, 720.0)), vec![]);
        // Mac pushes back at 50 %
        let o = p.on_leave(0.5);
        assert_eq!(p.state, State::Local);
        assert_eq!(sent(&o), vec![Msg2::Exit]);
        assert!(o.actions.contains(&Action::Warp(2.0, 720.0)));
        assert!(o.actions.contains(&Action::HideCursor(false)));
    }

    #[test]
    fn no_crossing_while_dragging_disconnected_or_into_another_monitor() {
        let mut p = pc();
        p.button(Button::Left, true);
        assert!(!p.mouse_move(0.0, 100.0).block);
        p.button(Button::Left, false);
        // a second monitor to the left continues the desktop: no edge there
        p.cfg.monitors.push(Rect { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 });
        assert!(!p.mouse_move(0.0, 100.0).block);
        assert!(p.mouse_move(0.0, 1200.0).block); // below the other monitor the edge is free
        let mut p = pc();
        p.disconnected();
        assert!(!p.mouse_move(0.0, 100.0).block);
    }

    #[test]
    fn keys_never_get_stuck_across_switches() {
        let mut p = pc();
        // Shift held on the PC, then the cursor crosses
        assert!(!p.key(0x2A, true).block);
        p.mouse_move(0.0, 10.0);
        // autorepeat of the PC-held key must not leak to the Mac
        assert!(p.key(0x2A, true).block);
        assert_eq!(sent(&p.key(0x2A, true)), vec![]);
        // its release goes to Windows (which saw the press)
        let o = p.key(0x2A, false);
        assert!(!o.block);
        // a key pressed on the Mac
        let o = p.key(0x1E, true);
        assert!(o.block);
        assert_eq!(sent(&o), vec![Msg2::Key { scan: 0x1E, down: true }]);
        // control returns while A is held; A's release must not reach Windows
        p.on_leave(0.1);
        let o = p.key(0x1E, false);
        assert!(o.block);
        assert_eq!(sent(&o), vec![]);
    }

    #[test]
    fn monitor_hotkey_round_trip_and_swallowing() {
        let mut p = pc();
        assert!(!p.key(0x1D, true).block); // Ctrl (local)
        assert!(!p.key(0x38, true).block); // Alt (local)
        let o = p.key(0x32, true); // M
        assert!(o.block);
        assert!(o.actions.contains(&Action::SwitchMonitor(true)));
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Monitor, side: Side::Right, along: 0.5 }]);
        assert!(p.key(0x32, true).block); // autorepeat swallowed
        assert!(p.key(0x32, false).block);
        // Ctrl/Alt were pressed locally: Windows gets their releases
        assert!(!p.key(0x38, false).block);
        assert!(!p.key(0x1D, false).block);
        assert_eq!(p.state, State::Remote { mode: Mode::Monitor });
        // edge pushes from the Mac are ignored in monitor mode
        assert!(sent(&p.on_leave(0.3)).is_empty());
        // typing goes to the Mac
        assert_eq!(sent(&p.key(0x1D, true)), vec![Msg2::Key { scan: 0x1D, down: true }]);
        assert_eq!(sent(&p.key(0x38, true)), vec![Msg2::Key { scan: 0x38, down: true }]);
        let o = p.key(0x32, true);
        assert!(o.actions.contains(&Action::SwitchMonitor(false)));
        assert_eq!(sent(&o), vec![Msg2::Exit]);
        assert_eq!(p.state, State::Local);
        // Ctrl/Alt went to the Mac: their releases are swallowed locally
        assert!(p.key(0x38, false).block);
        assert!(p.key(0x1D, false).block);
        assert!(p.key(0x32, false).block);
        assert!(!p.key(0x32, true).block); // M works normally again
    }

    #[test]
    fn hotkey_without_mac_still_switches_monitor_and_disconnect_releases() {
        let mut p = pc();
        p.disconnected();
        p.key(0x1D, true);
        p.key(0x38, true);
        let o = p.key(0x32, true);
        assert!(o.actions.contains(&Action::SwitchMonitor(true)));
        assert_eq!(p.state, State::Local);
        // Mac comes online while the monitor shows it -> straight into monitor mode
        let o = p.connected();
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Monitor, side: Side::Right, along: 0.5 }]);
        let o = p.disconnected();
        assert_eq!(p.state, State::Local);
        assert!(o.actions.contains(&Action::HideCursor(false)));
        assert!(sent(&o).is_empty()); // nothing to send to a gone peer
    }

    #[test]
    fn scroll_lock_is_the_emergency_exit() {
        let mut p = pc();
        p.mouse_move(0.0, 10.0);
        let o = p.key(SCAN_SCROLL_LOCK, true);
        assert!(o.block);
        assert_eq!(p.state, State::Local);
        assert!(p.key(SCAN_SCROLL_LOCK, false).block);
        assert!(!p.key(SCAN_SCROLL_LOCK, true).block); // local Scroll Lock works otherwise
    }

    #[test]
    fn buttons_follow_their_owner() {
        let mut p = pc();
        p.mouse_move(0.0, 10.0);
        assert_eq!(sent(&p.button(Button::Right, true)), vec![Msg2::Button { button: Button::Right, down: true }]);
        assert_eq!(sent(&p.button(Button::Right, false)), vec![Msg2::Button { button: Button::Right, down: false }]);
        assert_eq!(sent(&p.wheel(0, -120)), vec![Msg2::Wheel { dx: 0, dy: -120 }]);
        p.on_leave(0.5);
        assert!(!p.button(Button::Left, true).block);
        assert!(!p.wheel(0, 120).block);
    }

    #[test]
    fn ctrl_alt_del_does_not_kill_keys_forever() {
        let mut p = pc();
        p.mouse_move(0.0, 10.0);
        p.key(0x1D, true); // Ctrl down on the Mac side…
        p.key(0x38, true); // …Alt…  then Del: the secure desktop eats both releases
        p.release_now();
        assert!(p.key(0x1D, true).block); // stale owner: Ctrl would stay dead on the PC
        p.prune(|_| false, |_| false); // OS says nothing is held
        assert!(!p.key(0x1D, true).block);
        p.key(0x1D, false);
        // stale modifiers no longer block the hotkey either
        p.key(0x1D, true);
        p.key(0x38, true);
        assert!(p.key(0x32, true).actions.contains(&Action::SwitchMonitor(true)));
    }

    #[test]
    fn park_follows_the_real_cursor_and_replacement_sessions_resync() {
        let mut p = pc();
        p.mouse_move(0.0, 10.0);
        p.set_park(100.0, 100.0); // the monitor vanished, Windows clamped the cursor here
        assert_eq!(sent(&p.mouse_move(103.0, 99.0)), vec![Msg2::Motion { dx: 3, dy: -1 }]);
        // a new Mac session arrives while the old one had control
        let o = p.connected();
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Edge, side: Side::Right, along: 0.5 }]);
    }

    #[test]
    fn right_and_bottom_edges() {
        let mut p = pc();
        p.cfg.side = Side::Right;
        let o = p.mouse_move(2559.0, 1080.0);
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Edge, side: Side::Left, along: 0.75 }]);
        p.on_leave(0.0);
        p.cfg.side = Side::Bottom;
        let o = p.mouse_move(640.0, 1439.0);
        assert_eq!(sent(&o), vec![Msg2::Enter { mode: Mode::Edge, side: Side::Top, along: 0.25 }]);
        assert!(p.on_leave(1.0).actions.contains(&Action::Warp(2560.0, 1437.0)));
    }
}
