#![cfg_attr(not(windows), allow(dead_code))] // probe/current are used by the PC side only
//! Monitor input switching over DDC/CI (VCP 0x60). Same code on Windows and macOS via ddc-hi.
use ddc_hi::{Ddc, Display};
use std::sync::Mutex;

const VCP_INPUT: u8 = 0x60;
/// DDC is slow and monitors dislike concurrent requests.
static LOCK: Mutex<()> = Mutex::new(());

pub fn input_name(v: u16) -> String {
    match v {
        0x01 => "VGA 1".into(),
        0x03 => "DVI 1".into(),
        0x04 => "DVI 2".into(),
        0x0F => "DisplayPort 1".into(),
        0x10 => "DisplayPort 2".into(),
        0x11 => "HDMI 1".into(),
        0x12 => "HDMI 2".into(),
        0x1B => "USB-C".into(),
        v => format!("Input 0x{v:02X}"),
    }
}

/// Input values listed in a capabilities string, e.g. `vcp(... 60(01 03 0F 11 12) ...)`.
pub fn parse_inputs(caps: &str) -> Vec<u16> {
    let Some(i) = caps.find(" 60(").or_else(|| caps.find("(60(")) else { return vec![] };
    let rest = &caps[i + 4..];
    let end = rest.find(')').unwrap_or(rest.len());
    rest[..end].split_whitespace().filter_map(|h| u16::from_str_radix(h, 16).ok()).collect()
}

/// The first display that answers a 0x60 read. Returns (display, name, current value).
fn find() -> Result<(Display, String, u16), String> {
    let mut last = "no DDC/CI capable monitor found".to_string();
    for mut d in Display::enumerate() {
        match d.handle.get_vcp_feature(VCP_INPUT) {
            Ok(v) => {
                let name = d.info.model_name.clone().unwrap_or_else(|| d.info.id.clone());
                // monitors may report the value in the low byte only
                return Ok((d, name, v.value() & 0xFF));
            }
            Err(e) => last = format!("{}: {e}", d.info.id),
        }
    }
    Err(last)
}

#[derive(Debug, Default, Clone)]
pub struct Probe {
    pub monitor: String,
    pub current: Option<u16>,
    pub inputs: Vec<u16>,
    pub error: String,
}

pub fn probe() -> Probe {
    let _g = LOCK.lock().unwrap();
    match find() {
        Ok((mut d, monitor, current)) => {
            let inputs = d.handle.capabilities_string().map(|c| parse_inputs(&String::from_utf8_lossy(&c))).unwrap_or_default();
            Probe { monitor, current: Some(current), inputs, error: String::new() }
        }
        Err(error) => Probe { error, ..Default::default() },
    }
}

/// Current input value (None if no monitor answers, e.g. DDC only works on the active input).
pub fn current() -> Option<u16> {
    let _g = LOCK.lock().unwrap();
    find().ok().map(|(_, _, v)| v)
}

pub fn set_input(value: u16) -> Result<String, String> {
    let _g = LOCK.lock().unwrap();
    let (mut d, name, _) = find()?;
    d.handle.set_vcp_feature(VCP_INPUT, value).map_err(|e| format!("{name}: {e}"))?;
    Ok(format!("{name} → {}", input_name(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_viewsonic_caps_string() {
        let caps = "(prot(monitor)type(LCD)model(RTK)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 06 08 0B 0C 10 12 14(01 02 04 05 06 08 0B) 16 18 1A 52 60(01 03 04 0F 10 11 12) 87 AC)mccs_ver(2.2))";
        assert_eq!(parse_inputs(caps), vec![0x01, 0x03, 0x04, 0x0F, 0x10, 0x11, 0x12]);
        assert_eq!(parse_inputs("vcp(10 12)"), Vec::<u16>::new());
        assert_eq!(input_name(0x11), "HDMI 1");
    }
}
