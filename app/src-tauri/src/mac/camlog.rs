//! Follows the macOS CoreMediaIO log for "CMIOExtensionPropertyDeviceControlPID = <pid>" — the
//! process that controls a camera (the source Objective-See's OverSight uses on macOS 13.3+).
//! It tells us when an app releases the camera, so the PC camera can stop without probing
//! OBS during calls. If the log can't be read (e.g. no admin rights), nothing arrives and the
//! caller keeps its periodic probe.
use mm_core::camdemand::parse_control_pid;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

/// Sent when a control event's PID is hidden (`<private>`: macOS redacts it for non-admins).
pub const UNKNOWN_PID: i64 = i64::MIN;

/// Starts `log stream` and sends every control PID it reports.
pub fn follow(tx: Sender<i64>) {
    std::thread::spawn(move || {
        let child = Command::new("/usr/bin/log")
            .args([
                "stream",
                "--style",
                "ndjson",
                "--level",
                "debug",
                "--predicate",
                // any PID message: which one marks the end of a call is still being found out
                "subsystem == \"com.apple.cmio\" AND eventMessage CONTAINS[c] \"pid\"",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return crate::status::log(format!("camera log: cannot start `log stream`: {e}")),
        };
        crate::status::log("camera log: following CoreMediaIO control events");
        // `log stream` may refuse (e.g. "Must be admin"): keep what it says
        let err = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok).take(5) {
                crate::status::log(format!("camera log: `log stream` says: {l}"));
            }
        });
        let mut seen = std::collections::HashSet::new();
        for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            let msg = v["eventMessage"].as_str().unwrap_or("");
            let who = v["processImagePath"].as_str().unwrap_or("?").rsplit('/').next().unwrap_or("?");
            // each kind of message once (digits ignored): documents what macOS reports
            let kind: String = format!("{who} {msg}").chars().filter(|c| !c.is_ascii_digit()).take(200).collect();
            if seen.len() < 200 && seen.insert(kind) {
                crate::status::log(format!("camera log: [{who}] {}", msg.chars().take(300).collect::<String>().replace('\n', " ")));
            }
            let control = msg.to_ascii_lowercase().contains("controlpid");
            let Some(pid) = parse_control_pid(msg).or(control.then_some(UNKNOWN_PID)) else { continue };
            if tx.send(pid).is_err() {
                break;
            }
        }
        let _ = child.kill();
        crate::status::log(format!("camera log: `log stream` ended ({:?})", child.wait()));
    });
}

/// Process name for a PID (for the log).
pub fn process_name(pid: i64) -> String {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().rsplit('/').next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("pid {pid}"))
}

/// OBS's own PID: its control events are the feed, not a viewer.
pub fn obs_pid() -> Option<i64> {
    let o = Command::new("pgrep").args(["-x", "OBS"]).output().ok()?;
    String::from_utf8_lossy(&o.stdout).lines().next()?.trim().parse().ok()
}
