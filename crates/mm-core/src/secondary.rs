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
    /// Hand control back to the PC at `along` (0..1 along the shared edge).
    Leave { along: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Active {
    mode: Mode,
    side: Side,
    area: Rect,
    pos: (f64, f64),
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

    /// Returns the absolute point to warp the cursor to.
    pub fn enter(&mut self, mode: Mode, side: Side, along: f32) -> (f64, f64) {
        let area = match mode {
            Mode::Edge => self.displays.edge,
            Mode::Monitor => self.displays.monitor.unwrap_or(self.displays.edge),
        };
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
        self.active = Some(Active { mode, side, area, pos });
        pos
    }

    pub fn exit(&mut self) {
        self.active = None;
    }

    /// `os_pos`: the real cursor position if known (the Mac's own trackpad may have moved it).
    pub fn motion(&mut self, dx: i32, dy: i32, os_pos: Option<(f64, f64)>) -> Option<Effect> {
        let a = self.active.as_mut()?;
        let (dx, dy) = (dx as f64 * self.speed, dy as f64 * self.speed);
        let base = os_pos.filter(|p| a.area.contains(p.0, p.1)).unwrap_or(a.pos);
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
                self.active = None;
                return Some(Effect::Leave { along: along.clamp(0.0, 1.0) as f32 });
            }
        }
        let x = nx.clamp(r.x, r.x + r.w - 1.0);
        let y = ny.clamp(r.y, r.y + r.h - 1.0);
        a.pos = (x, y);
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
        assert_eq!(m.motion(5, 0, Some((1727.0, 1117.0 / 4.0))), Some(Effect::Leave { along: 0.25 }));
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
    fn speed_and_top_side() {
        let mut m = mac();
        m.speed = 0.5;
        m.enter(Mode::Edge, Side::Top, 1.0);
        assert_eq!(m.motion(10, 10, None), Some(Effect::MoveTo { x: 1727.0, y: 6.0, dx: 5.0, dy: 5.0 }));
        assert_eq!(m.motion(-1000, -20, None), Some(Effect::Leave { along: (1227.0 / 1728.0) as f32 }));
    }
}
