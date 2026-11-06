//! Wire messages. Encoded with postcard and carried inside the Noise channel (secure.rs).
use serde::{Deserialize, Serialize};

/// Bumped on incompatible changes; peers with a different version refuse the session.
pub const PROTOCOL_VERSION: u32 = 6; // 3: clipboard files, 4: PcDisplay, 5: MacActive, 6: audio channel
/// Oldest peer version still understood: newer versions only append messages, so the PC answers
/// with the lower of both versions and never sends a peer messages it does not know.
pub const MIN_COMPATIBLE_VERSION: u32 = 2;
pub const DEFAULT_PORT: u16 = 24850;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    /// Input, control, clipboard. Small messages, latency critical.
    Control,
    /// Camera frames. Kept on its own TCP connection so frames never delay input.
    Camera,
    /// Clipboard files, one connection per transfer (opened by the Mac, either direction).
    Files,
    /// PC microphone ⇄ Mac, Mac call sound → PC speakers (protocol 6). Opened by the Mac while an
    /// app uses "PC Microphone" or "PC Speakers".
    Audio,
}

/// Where input goes while the Mac has control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// The cursor crossed the screen edge onto the Mac's own (built-in) display.
    Edge,
    /// The shared monitor shows the Mac (HDMI); the cursor lives on that display.
    Monitor,
}

/// Side of a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub name: String,
    pub builtin: bool,
    /// Global coordinates in points (macOS) or pixels (Windows).
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    /// First message on every channel, both directions.
    Hello { version: u32, name: String, channel: Channel },
    /// Mac → PC: its displays and permission state, for the PC's UI.
    PeerInfo { displays: Vec<DisplayInfo>, can_inject: bool },
    /// PC → Mac: take control. `along` is the 0..1 position along the shared edge (Edge mode).
    /// `side` is the Mac's side that faces the PC (where the cursor appears).
    Enter { mode: Mode, side: Side, along: f32 },
    /// Mac → PC: the cursor was pushed back across the edge at `along`.
    Leave { along: f32 },
    /// PC → Mac: control returned to the PC (e.g. hotkey, disconnect, emergency key).
    Exit,
    Motion { dx: i32, dy: i32 },
    Button { button: Button, down: bool },
    /// Windows wheel units (120 per notch). Positive dy = away from the user.
    Wheel { dx: i32, dy: i32 },
    /// PC keyboard scan code (set 1, 0xE0xx for extended keys).
    Key { scan: u16, down: bool },
    /// Release every key and button the Mac thinks is held.
    ReleaseAll,
    /// PC → Mac: set the monitor's input source (VCP 0x60) over the Mac's cable.
    Ddc { value: u16 },
    /// Mac → PC: result of a Ddc request.
    DdcResult { ok: bool, detail: String },
    /// Clipboard hand-over on every switch (like a hardware KVM): plain text, HTML (formatted
    /// text) and/or an image as PNG. Sent by the side that is giving up control.
    Clipboard { text: Option<String>, html: Option<String>, png: Option<Vec<u8>> },
    /// Mac → PC: an app on the Mac wants the camera (true) or released it (false).
    CameraWanted { on: bool },
    /// PC → Mac: camera state.
    CameraState { on: bool, detail: String },
    /// Camera channel only: one JPEG frame.
    Frame { jpeg: Vec<u8> },
    /// Audio channel: 10 ms of mono 16-bit sound at `rate` Hz; PC → Mac: its microphone,
    /// Mac → PC: what Mac apps play to "PC Speakers".
    Audio { rate: u32, pcm: Vec<i16> },
    Ping { t: u64 },
    Pong { t: u64 },
    /// Control: the clipboard holds files; `names` are the top-level items (for display).
    FilesOffer { id: u64, count: u32, bytes: u64, names: Vec<String> },
    /// Control, PC → Mac: open a files channel and push offer `id` (only the Mac can dial).
    FilesPushRequest { id: u64 },
    /// Files channel, first message from the Mac: send me offer `id`.
    FilesPull { id: u64 },
    /// Files channel, first message from the Mac: here comes my offer `id`.
    FilesPush { id: u64 },
    /// Files channel: next item; `path` is relative with '/' separators; data follows for files.
    FileEntry { path: String, size: u64, dir: bool },
    FileData { data: Vec<u8> },
    FilesEnd,
    /// Control, PC → Mac: Windows turned its screen off (user away) or on again.
    PcDisplay { on: bool },
    /// Control, Mac → PC: someone uses the Mac right now (input within the last 30 s). The PC
    /// keeps its screen awake, so the monitor does not fall back to the Mac's HDMI signal.
    MacActive,
    /// Audio channel, Mac → PC: which of the PC's audio devices Mac apps use right now.
    AudioWant { mic: bool, speaker: bool },
}

pub fn encode(msg: &Msg) -> Vec<u8> {
    postcard::to_stdvec(msg).expect("postcard encoding of an owned enum cannot fail")
}

pub fn decode(bytes: &[u8]) -> Result<Msg, postcard::Error> {
    postcard::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_shapes() {
        let msgs = vec![
            Msg::Hello { version: PROTOCOL_VERSION, name: "GameMachine".into(), channel: Channel::Control },
            Msg::Enter { mode: Mode::Monitor, side: Side::Right, along: 0.25 },
            Msg::Motion { dx: -3, dy: 7 },
            Msg::Key { scan: 0xE01D, down: true },
            Msg::Wheel { dx: 0, dy: -120 },
            Msg::Frame { jpeg: vec![0xFF, 0xD8, 1, 2, 3] },
            Msg::Clipboard { text: Some("grüße ✓".into()), html: Some("<b>grüße</b>".into()), png: Some(vec![0x89, b'P']) },
        ];
        for m in msgs {
            assert_eq!(decode(&encode(&m)).unwrap(), m);
        }
    }

    #[test]
    fn motion_is_tiny() {
        // Input latency: a mouse move should stay a handful of bytes on the wire.
        assert!(encode(&Msg::Motion { dx: 5, dy: -5 }).len() <= 4);
    }
}
