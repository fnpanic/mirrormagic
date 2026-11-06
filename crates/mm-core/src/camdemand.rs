//! When should the PC camera run? macOS reports the OBS Virtual Camera as "in use" as soon as
//! OBS feeds it, even if no app watches. So OBS's virtual camera stays off while idle; then "in
//! use" can only mean an app (Teams, FaceTime, a browser) opened it. While it runs, a short probe
//! (virtual camera off for ~0.5 s) checks whether an app still watches.

/// Probe interval while the camera runs and no camera-control log is available.
pub const PROBE_EVERY_MS: u64 = 45_000;
/// Safety probe interval when the macOS camera-control log tells us when apps release the camera.
pub const PROBE_WITH_LOG_MS: u64 = 600_000;

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

pub struct Demand {
    pub on: bool,
    since_probe_ms: u64,
    /// PROBE_EVERY_MS, or PROBE_WITH_LOG_MS once release hints arrive from the log.
    pub probe_every_ms: u64,
    release_hint: bool,
}

impl Default for Demand {
    fn default() -> Self {
        Demand { on: false, since_probe_ms: 0, probe_every_ms: PROBE_EVERY_MS, release_hint: false }
    }
}

impl Demand {
    /// The macOS log says an app gave up control of a camera: check at the next poll instead of
    /// waiting for the periodic probe (the only interruption then happens when nobody watches).
    pub fn release_hint(&mut self) {
        self.release_hint = true;
    }

    /// Called about once a second. `in_use`: macOS says the virtual camera is in use.
    /// `manual`: the user's "Stream now" switch.
    pub fn poll(&mut self, dt_ms: u64, in_use: bool, manual: bool) -> Cmd {
        if !self.on {
            self.release_hint = false;
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
        if std::mem::take(&mut self.release_hint) || self.since_probe_ms >= self.probe_every_ms {
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

/// Extracts the PID from macOS CoreMediaIO log lines like
/// "... CMIOExtensionPropertyDeviceControlPID = 4711; ..." (the process controlling a camera;
/// 0 or -1 when control is released). Objective-See's OverSight attributes camera use this way.
pub fn parse_control_pid(msg: &str) -> Option<i64> {
    const KEY: &str = "DeviceControlPID = ";
    let rest = &msg[msg.find(KEY)? + KEY.len()..];
    let end = rest.find(|c: char| c != '-' && !c.is_ascii_digit())?;
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_pid_from_log_lines() {
        assert_eq!(parse_control_pid("props { CMIOExtensionPropertyDeviceControlPID = 4711; CMIOExtensionPropertyDeviceModel = x; }"), Some(4711));
        assert_eq!(parse_control_pid("CMIOExtensionPropertyDeviceControlPID = -1;"), Some(-1));
        assert_eq!(parse_control_pid("CMIOExtensionPropertyDeviceControlPID = 0;\n"), Some(0));
        assert_eq!(parse_control_pid("unrelated message"), None);
        assert_eq!(parse_control_pid("CMIOExtensionPropertyDeviceControlPID = <null>;"), None);
    }

    #[test]
    fn with_the_camera_log_there_is_no_interruption_during_a_call() {
        let mut d = Demand { probe_every_ms: PROBE_WITH_LOG_MS, ..Default::default() };
        assert_eq!(d.poll(1000, true, false), Cmd::Start);
        // a 30-minute call: no probes (only the 10-minute safety check)
        let probes = (0..1800).filter(|_| d.poll(1000, true, false) == Cmd::Probe).count();
        assert_eq!(probes, 3);
        // the app releases the camera: checked at once, then off
        d.release_hint();
        assert_eq!(d.poll(1000, true, false), Cmd::Probe);
        assert_eq!(d.probe_result(false), Cmd::Stop);
        // a stale hint while idle is ignored
        d.release_hint();
        assert_eq!(d.poll(1000, false, false), Cmd::Nothing);
        assert_eq!(d.poll(1000, true, false), Cmd::Start);
        assert_eq!(d.poll(1000, true, false), Cmd::Nothing);
    }

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
