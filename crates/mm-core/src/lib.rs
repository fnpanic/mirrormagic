//! MirrorMagic core: wire protocol, encrypted channel and the platform-independent KVM logic.
pub mod camdemand;
pub mod files;
pub mod keymap;
pub mod primary;
pub mod proto;
pub mod secondary;
pub mod secure;

/// LAN discovery over UDP broadcast on `proto::DEFAULT_PORT`: the Mac asks, the PC answers with
/// its name. Carries no secrets; the pairing key still gates the actual connection.
pub const DISCOVER_ASK: &[u8] = b"MIRRORMAGIC?1";
pub const DISCOVER_REPLY_PREFIX: &[u8] = b"MIRRORMAGIC!1 ";
