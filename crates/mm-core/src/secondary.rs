//! Mac-side cursor logic: where the cursor appears, how PC deltas move it, and when it is pushed
//! back to the PC. Pure; the macOS layer reads the real cursor position and posts the events.
use crate::primary::Rect;
use crate::proto::{Mode, Side};

#[derive(Clone, Debug, PartialEq)]
pub struct Displays {
    /// The display next to the PC monitor (the MacBook's built-in screen).
    pub edge: Rect,
    /// The display on the shared monitor (HDMI), if connected.
    pub monitor: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Move the cursor to x,y (absolute, global points) carrying the raw deltas.
    MoveTo { x: f64, y: f64, dx: f64, dy: f64 },
    /// Hand control back to the PC at `along` (0..1 along the shared edge), and leave the Mac's
    /// cursor at `park`: inside the screen, where it is visible for the Mac's own trackpad (at
    /// the very edge, macOS draws the arrow off-screen).
    Leave { along: f32, park: (f64, f64) },
}

/// Mac → PC "in use" reports: only while the Mac had input within this many seconds, and at most
/// this often. Windows' shortest display timeout is a minute, so 30 s keeps the PC screen on.
pub const ACTIVE_EVERY_S: f64 = 30.0;

/// Should the Mac tell the PC it is in use? `idle_s`: seconds since the Mac's last input;
/// `since_report_s`: seconds since the last report.
pub fn mac_active_due(idle_s: f64, since_report_s: f64) -> bool {
    idle_s < ACTIVE_EVERY_S && since_report_s >= ACTIVE_EVERY_S
}

/// A real cursor position this far (points) from the tracked one means the Mac's own trackpad moved it.
const OS_MOVED: f64 = 3.0;
/// Positions posted most recently: macOS may report any of them while it catches up.
const RECENT: usize = 32;
/// How far inside the shared edge the Mac's cursor waits after handing control back.
const PARK_INSET: f64 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Active {
    mode: Mode,
    side: Side,
    area: Rect,
    pos: (f64, f64),
    /// Ring of the last positions posted (`pos` included), newest at `next - 1`.
    recent: [(f64, f64); RECENT],
    next: usize,
}

impl Active {
    fn new(mode: Mode, side: Side, area: Rect, pos: (f64, f64)) -> Self {
        Active { mode, side, area, pos, recent: [pos; RECENT], next: 0 }
    }

    fn set_pos(&mut self, pos: (f64, f64)) {
        self.pos = pos;
        self.recent[self.next] = pos;
        self.next = (self.next + 1) % RECENT;
    }

    /// The real position is none we posted lately: something else (the Mac's trackpad) moved
    /// the cursor. A read-back that is rounded or several events late must not move it (drift,
    /// or jumping back and forth during fast moves).
    fn moved_locally(&self, p: (f64, f64)) -> bool {
        let near = |q: &(f64, f64)| (p.0 - q.0).abs() <= OS_MOVED && (p.1 - q.1).abs() <= OS_MOVED;
        !near(&self.pos) && !self.recent.iter().any(near)
    }
}

pub struct Secondary {
    pub displays: Displays,
    /// Multiplier for PC pixels → Mac points.
    pub speed: f64,
    active: Option<Active>,
}

impl Secondary {
    pub fn new(displays: Displays) -> Self {
        Secondary { displays, speed: 1.0, active: None }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn mode(&self) -> Option<Mode> {
        self.active.map(|a| a.mode)
    }

    fn area_for(&self, mode: Mode) -> Rect {
        match mode {
            Mode::Edge => self.displays.edge,
            Mode::Monitor => self.displays.monitor.unwrap_or(self.displays.edge),
        }
    }

    /// A new display layout. macOS applies changes asynchronously (e.g. just after mirroring
    /// starts), so an area read too early is stale and would pin the cursor to one of its edges:
    /// an active session moves to the new area, at the real cursor position if that lies inside.
    pub fn set_displays(&mut self, displays: Displays, os_pos: Option<(f64, f64)>) {
        self.displays = displays;
        let Some(mode) = self.mode() else { return };
        let area = self.area_for(mode);
        let a = self.active.as_mut().unwrap();
        if a.area == area {
            return;
        }
        a.area = area;
        let pos = os_pos.filter(|p| area.contains(p.0, p.1)).unwrap_or((a.pos.0.clamp(area.x, area.x + area.w - 1.0), a.pos.1.clamp(area.y, area.y + area.h - 1.0)));
        *a = Active::new(a.mode, a.side, area, pos);
    }

    /// Returns the absolute point to warp the cursor to.
    pub fn enter(&mut self, mode: Mode, side: Side, along: f32) -> (f64, f64) {
        let area = self.area_for(mode);
        let a = (along as f64).clamp(0.0, 1.0);
        let pos = match mode {
            Mode::Monitor => area.center(),
            Mode::Edge => match side {
                Side::Right => (area.x + area.w - 2.0, area.y + a * (area.h - 1.0)),
                Side::Left => (area.x + 1.0, area.y + a * (area.h - 1.0)),
                Side::Top => (area.x + a * (area.w - 1.0), area.y + 1.0),
                Side::Bottom => (area.x + a * (area.w - 1.0), area.y + area.h - 2.0),
            },
        };
        self.active = Some(Active::new(mode, side, area, pos));
        pos
    }

    pub fn exit(&mut self) {
        self.active = None;
    }

    /// `os_pos`: the real cursor position if known (the Mac's own trackpad may have moved it).
    pub fn motion(&mut self, dx: i32, dy: i32, os_pos: Option<(f64, f64)>) -> Option<Effect> {
        let a = self.active.as_mut()?;
        let (dx, dy) = (dx as f64 * self.speed, dy as f64 * self.speed);
        let base = os_pos.filter(|p| a.area.contains(p.0, p.1) && a.moved_locally(*p)).unwrap_or(a.pos);
        let (nx, ny) = (base.0 + dx, base.1 + dy);
        let r = a.area;
        if a.mode == Mode::Edge {
            let out = match a.side {
                Side::Right => (nx >= r.x + r.w).then(|| (ny - r.y) / r.h),
                Side::Left => (nx < r.x).then(|| (ny - r.y) / r.h),
                Side::Top => (ny < r.y).then(|| (nx - r.x) / r.w),
                Side::Bottom => (ny >= r.y + r.h).then(|| (nx - r.x) / r.w),
            };
            if let Some(along) = out {
                let (x, y) = (nx.clamp(r.x, r.x + r.w - 1.0), ny.clamp(r.y, r.y + r.h - 1.0));
                let (ix, iy) = (PARK_INSET.min(r.w / 4.0), PARK_INSET.min(r.h / 4.0));
                let park = match a.side {
                    Side::Right => (r.x + r.w - ix, y),
                    Side::Left => (r.x + ix, y),
                    Side::Top => (x, r.y + iy),
                    Side::Bottom => (x, r.y + r.h - iy),
                };
                self.active = None;
                return Some(Effect::Leave { along: along.clamp(0.0, 1.0) as f32, park });
            }
        }
        let x = nx.clamp(r.x, r.x + r.w - 1.0);
        let y = ny.clamp(r.y, r.y + r.h - 1.0);
        a.set_pos((x, y));
        Some(Effect::MoveTo { x, y, dx, dy })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac() -> Secondary {
        Secondary::new(Displays {
            edge: Rect { x: 0.0, y: 0.0, w: 1728.0, h: 1117.0 },
            monitor: Some(Rect { x: 1728.0, y: -200.0, w: 2560.0, h: 1440.0 }),
        })
    }

    #[test]
    fn edge_enter_move_clamp_and_leave() {
        let mut m = mac();
        assert_eq!(m.motion(1, 1, None), None); // inactive
        let p = m.enter(Mode::Edge, Side::Right, 0.5);
        assert_eq!(p, (1726.0, 558.0));
        assert_eq!(m.motion(-10, 0, None), Some(Effect::MoveTo { x: 1716.0, y: 558.0, dx: -10.0, dy: 0.0 }));
        // other edges clamp
        assert_eq!(m.motion(0, -5000, None), Some(Effect::MoveTo { x: 1716.0, y: 0.0, dx: 0.0, dy: -5000.0 }));
        assert_eq!(m.motion(-5000, 0, None), Some(Effect::MoveTo { x: 0.0, y: 0.0, dx: -5000.0, dy: 0.0 }));
        // the real cursor moved (trackpad) to the far right: next push leaves at its y
        assert_eq!(m.motion(5, 0, Some((1727.0, 1117.0 / 4.0))), Some(Effect::Leave { along: 0.25, park: (1680.0, 1117.0 / 4.0) }));
        assert!(!m.is_active());
    }

    #[test]
    fn monitor_mode_uses_the_hdmi_display_and_never_leaves() {
        let mut m = mac();
        assert_eq!(m.enter(Mode::Monitor, Side::Right, 0.1), (3008.0, 520.0));
        for _ in 0..100 {
            assert!(matches!(m.motion(500, 0, None), Some(Effect::MoveTo { .. })));
        }
        assert_eq!(m.motion(0, 0, None), Some(Effect::MoveTo { x: 4287.0, y: 520.0, dx: 0.0, dy: 0.0 }));
        // a stale OS position outside the area is ignored
        assert!(matches!(m.motion(1, 0, Some((10.0, 10.0))), Some(Effect::MoveTo { x, .. }) if x == 4287.0));
        m.displays.monitor = None;
        assert_eq!(m.enter(Mode::Monitor, Side::Right, 0.0), (864.0, 558.5)); // falls back to built-in
    }

    #[test]
    fn stale_monitor_area_heals_when_the_layout_arrives() {
        // extended layout: the monitor sits above the laptop; mirroring then moves it to 0,0
        let mut m = Secondary::new(Displays {
            edge: Rect { x: 0.0, y: 0.0, w: 1728.0, h: 1117.0 },
            monitor: Some(Rect { x: 0.0, y: -1440.0, w: 2560.0, h: 1440.0 }),
        });
        m.enter(Mode::Monitor, Side::Right, 0.5);
        // the bug: with the old area every move down is clamped to its bottom row, y = -1
        assert!(matches!(m.motion(0, 1000, None), Some(Effect::MoveTo { y, .. }) if y == -1.0));
        let mirrored = Displays { edge: Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 }, monitor: Some(Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 }) };
        m.set_displays(mirrored.clone(), Some((1280.0, 0.0))); // macOS clamped the cursor to the top
        assert_eq!(m.motion(0, 300, Some((1280.0, 0.0))), Some(Effect::MoveTo { x: 1280.0, y: 300.0, dx: 0.0, dy: 300.0 }));
        // same layout again: nothing moves
        m.set_displays(mirrored, Some((5.0, 5.0)));
        assert_eq!(m.motion(0, 0, None), Some(Effect::MoveTo { x: 1280.0, y: 300.0, dx: 0.0, dy: 0.0 }));
        // inactive: just stored
        m.exit();
        m.set_displays(Displays { edge: Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, monitor: None }, None);
        assert_eq!(m.enter(Mode::Monitor, Side::Right, 0.0), (50.0, 50.0));
    }

    #[test]
    fn fast_moves_with_a_read_back_several_events_late_do_not_jiggle() {
        // monitor mode, 2026-11-08 23:43–00:00: the cursor jiggled and was barely usable. macOS
        // (busy mirroring) reports the position several events late; during fast moves that is
        // far more than OS_MOVED away and was taken for a trackpad move: the cursor jumped back.
        let mut m = mac();
        let start = m.enter(Mode::Monitor, Side::Right, 0.5);
        let mut posted = vec![start];
        for _ in 0..50 {
            let late = posted[posted.len().saturating_sub(4)]; // 3 events behind
            let Some(Effect::MoveTo { x, y, .. }) = m.motion(20, -5, Some(late)) else { panic!() };
            posted.push((x, y));
        }
        assert_eq!(*posted.last().unwrap(), (start.0 + 1000.0, start.1 - 250.0));
        // a real trackpad move is still followed
        assert_eq!(m.motion(0, 1, Some((2000.0, 600.0))), Some(Effect::MoveTo { x: 2000.0, y: 601.0, dx: 0.0, dy: 1.0 }));
    }

    #[test]
    fn a_lagging_or_rounded_os_position_causes_no_drift() {
        let mut m = mac();
        m.enter(Mode::Monitor, Side::Right, 0.5); // (3008, 520)
        // macOS reports the cursor one event behind and rounded: 1000 tiny moves right must add up
        let (mut last, mut before) = ((3008.0, 520.0), (3008.0, 520.0));
        for _ in 0..1000 {
            let Some(Effect::MoveTo { x, y, .. }) = m.motion(1, 0, Some((before.0 - 0.4, before.1))) else { panic!() };
            (before, last) = (last, (x, y));
        }
        assert_eq!(last, (4008.0, 520.0));
        // a real trackpad move (far from the tracked position) is still followed
        assert_eq!(m.motion(0, 1, Some((2000.0, 600.0))), Some(Effect::MoveTo { x: 2000.0, y: 601.0, dx: 0.0, dy: 1.0 }));
    }

    #[test]
    fn the_mac_reports_use_every_30_s_only_while_someone_works_on_it() {
        assert!(mac_active_due(2.0, 31.0));
        assert!(!mac_active_due(2.0, 10.0)); // reported recently
        assert!(!mac_active_due(45.0, 120.0)); // nobody at the Mac: let the PC screen sleep
    }

    #[test]
    fn speed_and_top_side() {
        let mut m = mac();
        m.speed = 0.5;
        m.enter(Mode::Edge, Side::Top, 1.0);
        assert_eq!(m.motion(10, 10, None), Some(Effect::MoveTo { x: 1727.0, y: 6.0, dx: 5.0, dy: 5.0 }));
        assert_eq!(m.motion(-1000, -20, None), Some(Effect::Leave { along: (1227.0 / 1728.0) as f32, park: (1227.0, 48.0) }));
    }
}
