//! Follows the macOS CoreMediaIO log for "CMIOExtensionPropertyDeviceControlPID = <pid>" — the
//! process that controls a camera (the source Objective-See's OverSight uses on macOS 13.3+).
//! It tells us when an app releases the camera, so the PC camera can stop without probing
//! OBS during calls. If the log can't be read (e.g. no admin rights), nothing arrives and the
//! caller keeps its periodic probe.
use mm_core::camdemand::parse_control_pid;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

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
                "subsystem == \"com.apple.cmio\" AND eventMessage CONTAINS \"DeviceControlPID\"",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return crate::status::log(format!("camera log: cannot start `log stream`: {e}")),
        };
        crate::status::log("camera log: following CoreMediaIO control events");
        let mut evidence = 0;
        for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            let msg = v["eventMessage"].as_str().unwrap_or("");
            let Some(pid) = parse_control_pid(msg) else { continue };
            if evidence < 40 {
                // keep the raw lines of the first events: they document what macOS reports
                evidence += 1;
                let who = v["processImagePath"].as_str().unwrap_or("?").rsplit('/').next().unwrap_or("?");
                crate::status::log(format!("camera log: [{who}] {}", msg.chars().take(300).collect::<String>().replace('\n', " ")));
            }
            if tx.send(pid).is_err() {
                break;
            }
        }
        let _ = child.kill();
        crate::status::log("camera log: `log stream` ended");
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
