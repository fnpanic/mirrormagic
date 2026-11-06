//! When should the PC camera run? macOS reports the OBS Virtual Camera as "in use" as soon as
//! OBS feeds it, even if no app watches. So OBS's virtual camera stays off while idle; then "in
//! use" can only mean an app (Teams, FaceTime, a browser) opened it. While it runs, a short probe
//! (virtual camera off for ~0.5 s) checks whether an app still watches.

/// Seconds between probes while the camera runs.
pub const PROBE_EVERY_MS: u64 = 45_000;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Cmd {
    Nothing,
    /// An app wants the camera: PC camera + mic on, OBS virtual camera on.
    Start,
    /// Turn OBS's virtual camera off briefly, then call `probe_result` with "in use".
    Probe,
    /// Probe found an app: turn OBS's virtual camera back on.
    Resume,
    /// Nobody watches: PC camera + mic off, OBS virtual camera stays off.
    Stop,
}

#[derive(Default)]
pub struct Demand {
    pub on: bool,
    since_probe_ms: u64,
}

impl Demand {
    /// Called about once a second. `in_use`: macOS says the virtual camera is in use.
    /// `manual`: the user's "Stream now" switch.
    pub fn poll(&mut self, dt_ms: u64, in_use: bool, manual: bool) -> Cmd {
        if !self.on {
            // OBS's virtual camera is off now, so "in use" is a real app
            if in_use || manual {
                self.on = true;
                self.since_probe_ms = 0;
                return Cmd::Start;
            }
            return Cmd::Nothing;
        }
        if manual {
            self.since_probe_ms = 0;
            return Cmd::Nothing;
        }
        self.since_probe_ms += dt_ms;
        if self.since_probe_ms >= PROBE_EVERY_MS {
            self.since_probe_ms = 0;
            return Cmd::Probe;
        }
        Cmd::Nothing
    }

    pub fn probe_result(&mut self, in_use: bool) -> Cmd {
        if in_use {
            Cmd::Resume
        } else {
            self.on = false;
            Cmd::Stop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_runs_only_while_an_app_watches() {
        let mut d = Demand::default();
        assert_eq!(d.poll(1000, false, false), Cmd::Nothing); // idle: webcam off
        assert_eq!(d.poll(1000, true, false), Cmd::Start); // Teams opens the camera
        // while running, macOS always says "in use" (OBS feeds it): no decision from that
        for _ in 0..44 {
            assert_eq!(d.poll(1000, true, false), Cmd::Nothing);
        }
        assert_eq!(d.poll(1000, true, false), Cmd::Probe);
        assert_eq!(d.probe_result(true), Cmd::Resume); // still in the call
        for _ in 0..44 {
            d.poll(1000, true, false);
        }
        assert_eq!(d.poll(1000, true, false), Cmd::Probe);
        assert_eq!(d.probe_result(false), Cmd::Stop); // call ended
        assert!(!d.on);
        assert_eq!(d.poll(1000, false, false), Cmd::Nothing);
    }

    #[test]
    fn manual_switch_keeps_it_on_without_probes() {
        let mut d = Demand::default();
        assert_eq!(d.poll(1000, false, true), Cmd::Start);
        for _ in 0..200 {
            assert_eq!(d.poll(1000, true, true), Cmd::Nothing);
        }
        // switch off: the next probe decides
        for _ in 0..44 {
            d.poll(1000, true, false);
        }
        assert_eq!(d.poll(1000, true, false), Cmd::Probe);
    }
}
