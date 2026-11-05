//! Encrypted, mutually authenticated channel over TCP.
//!
//! Noise `NNpsk0_25519_ChaChaPoly_BLAKE2s`: fresh ephemeral keys per connection (forward secrecy),
//! and both sides must hold the same 256-bit PSK derived from the pairing key, otherwise the
//! handshake fails. Messages are length-prefixed postcard blobs; large messages (camera frames)
//! are split across Noise transport messages (max 65535 bytes each).
use crate::proto::{self, Msg};
use sha2::{Digest, Sha256};
use snow::{params::NoiseParams, Builder, StatelessTransportState};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

const PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"mirrormagic/1";
const MAX_NOISE: usize = 65535;
const TAG: usize = 16;
const CHUNK: usize = MAX_NOISE - TAG;
/// Upper bound for one decoded message (a 4K MJPEG frame is well below this).
const MAX_MSG: usize = 16 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

pub type Error = Box<dyn std::error::Error + Send + Sync>;

// ---------------------------------------------------------------- pairing key

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ"; // Crockford base32

/// 128-bit pairing secret, shown to the user as `XXXXX-XXXXX-XXXXX-XXXXX-XXXXXX`.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingKey(pub [u8; 16]);

impl std::fmt::Debug for PairingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairingKey(..)") // never log the secret
    }
}

impl PairingKey {
    pub fn generate() -> Self {
        let mut k = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut k);
        PairingKey(k)
    }

    pub fn to_display(&self) -> String {
        let mut bits: u32 = 0;
        let mut n = 0;
        let mut out = String::new();
        for &b in &self.0 {
            bits = (bits << 8) | b as u32;
            n += 8;
            while n >= 5 {
                n -= 5;
                out.push(ALPHABET[((bits >> n) & 31) as usize] as char);
            }
        }
        if n > 0 {
            out.push(ALPHABET[((bits << (5 - n)) & 31) as usize] as char);
        }
        // 26 chars -> 5-5-5-5-6 groups
        let groups = [&out[0..5], &out[5..10], &out[10..15], &out[15..20], &out[20..26]];
        groups.join("-")
    }

    /// Accepts any case, spaces and dashes; maps the usual look-alikes (O→0, I/L→1).
    pub fn parse(s: &str) -> Option<Self> {
        let mut bits: u32 = 0;
        let mut n = 0;
        let mut out = Vec::with_capacity(16);
        let mut count = 0;
        for c in s.chars() {
            let c = match c.to_ascii_uppercase() {
                '-' | ' ' => continue,
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
            count += 1;
            bits = (bits << 5) | v;
            n += 5;
            if n >= 8 {
                n -= 8;
                out.push((bits >> n) as u8);
            }
        }
        if count != 26 || out.len() != 16 {
            return None;
        }
        Some(PairingKey(out.try_into().ok()?))
    }

    fn psk(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"mirrormagic-psk-v1");
        h.update(self.0);
        h.finalize().into()
    }
}

// ---------------------------------------------------------------- channel

pub struct Sender {
    w: OwnedWriteHalf,
    t: Arc<StatelessTransportState>,
    nonce: u64,
    buf: Vec<u8>,
}

pub struct Receiver {
    r: OwnedReadHalf,
    t: Arc<StatelessTransportState>,
    nonce: u64,
    plain: Vec<u8>,
    ct: Vec<u8>,
}

fn params() -> NoiseParams {
    PATTERN.parse().expect("valid noise pattern")
}

async fn write_frame(s: &mut TcpStream, b: &[u8]) -> Result<(), Error> {
    s.write_all(&(b.len() as u16).to_be_bytes()).await?;
    s.write_all(b).await?;
    Ok(())
}

async fn read_frame(s: &mut TcpStream, buf: &mut Vec<u8>) -> Result<(), Error> {
    let mut len = [0u8; 2];
    s.read_exact(&mut len).await?;
    buf.resize(u16::from_be_bytes(len) as usize, 0);
    s.read_exact(buf).await?;
    Ok(())
}

/// Client side (the Mac). Fails if the PC holds a different pairing key.
pub async fn connect(mut s: TcpStream, key: &PairingKey) -> Result<(Sender, Receiver), Error> {
    s.set_nodelay(true)?;
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut hs = Builder::new(params()).prologue(PROLOGUE).psk(0, &key.psk()).build_initiator()?;
        let mut out = vec![0u8; MAX_NOISE];
        let mut inb = Vec::new();
        let n = hs.write_message(&[], &mut out)?;
        write_frame(&mut s, &out[..n]).await?;
        read_frame(&mut s, &mut inb).await?;
        hs.read_message(&inb, &mut out)?;
        Ok::<_, Error>(hs.into_stateless_transport_mode()?)
    })
    .await
    .map_err(|_| "handshake timed out")?
    .map(|t| split(s, t))
}

/// Server side (the PC).
pub async fn accept(mut s: TcpStream, key: &PairingKey) -> Result<(Sender, Receiver), Error> {
    s.set_nodelay(true)?;
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut hs = Builder::new(params()).prologue(PROLOGUE).psk(0, &key.psk()).build_responder()?;
        let mut out = vec![0u8; MAX_NOISE];
        let mut inb = Vec::new();
        read_frame(&mut s, &mut inb).await?;
        hs.read_message(&inb, &mut out)?;
        let n = hs.write_message(&[], &mut out)?;
        write_frame(&mut s, &out[..n]).await?;
        Ok::<_, Error>(hs.into_stateless_transport_mode()?)
    })
    .await
    .map_err(|_| "handshake timed out")?
    .map(|t| split(s, t))
}

fn split(s: TcpStream, t: StatelessTransportState) -> (Sender, Receiver) {
    let t = Arc::new(t);
    let (r, w) = s.into_split();
    (
        Sender { w, t: t.clone(), nonce: 0, buf: vec![0u8; MAX_NOISE] },
        Receiver { r, t, nonce: 0, plain: Vec::new(), ct: vec![0u8; MAX_NOISE] },
    )
}

impl Sender {
    pub async fn send(&mut self, msg: &Msg) -> Result<(), Error> {
        let body = proto::encode(msg);
        let mut plain = Vec::with_capacity(body.len() + 4);
        plain.extend_from_slice(&(body.len() as u32).to_be_bytes());
        plain.extend_from_slice(&body);
        let mut wire = Vec::with_capacity(plain.len() + (plain.len() / CHUNK + 1) * (TAG + 2));
        for chunk in plain.chunks(CHUNK) {
            let n = self.t.write_message(self.nonce, chunk, &mut self.buf)?;
            self.nonce += 1;
            wire.extend_from_slice(&(n as u16).to_be_bytes());
            wire.extend_from_slice(&self.buf[..n]);
        }
        self.w.write_all(&wire).await?;
        Ok(())
    }
}

impl Receiver {
    pub async fn recv(&mut self) -> Result<Msg, Error> {
        loop {
            if self.plain.len() >= 4 {
                let len = u32::from_be_bytes(self.plain[..4].try_into().unwrap()) as usize;
                if len > MAX_MSG {
                    return Err("message too large".into());
                }
                if self.plain.len() >= 4 + len {
                    let msg = proto::decode(&self.plain[4..4 + len]);
                    self.plain.drain(..4 + len);
                    return Ok(msg?);
                }
            }
            let mut len = [0u8; 2];
            self.r.read_exact(&mut len).await?;
            let n = u16::from_be_bytes(len) as usize;
            self.r.read_exact(&mut self.ct[..n]).await?;
            let mut out = vec![0u8; n];
            let m = self.t.read_message(self.nonce, &self.ct[..n], &mut out).map_err(|_| "decrypt failed")?;
            self.nonce += 1;
            self.plain.extend_from_slice(&out[..m]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Channel;
    use tokio::net::TcpListener;

    #[test]
    fn pairing_key_display_roundtrip() {
        for _ in 0..200 {
            let k = PairingKey::generate();
            let s = k.to_display();
            assert_eq!(s.len(), 30);
            assert_eq!(PairingKey::parse(&s), Some(k.clone()));
            assert_eq!(PairingKey::parse(&s.to_lowercase().replace('-', " ")), Some(k));
        }
        assert_eq!(PairingKey::parse("too-short"), None);
        assert_eq!(PairingKey::parse("UUUUU-UUUUU-UUUUU-UUUUU-UUUUUU"), None); // U is not in the alphabet
    }

    async fn pair(a: PairingKey, b: PairingKey) -> (Result<(Sender, Receiver), Error>, Result<(Sender, Receiver), Error>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            accept(s, &a).await
        });
        let cli = connect(TcpStream::connect(addr).await.unwrap(), &b).await;
        (srv.await.unwrap(), cli)
    }

    #[tokio::test]
    async fn exchange_small_and_large_messages() {
        let k = PairingKey::generate();
        let (srv, cli) = pair(k.clone(), k).await;
        let (mut stx, mut srx) = srv.unwrap();
        let (mut ctx, mut crx) = cli.unwrap();
        let hello = Msg::Hello { version: 1, name: "mac".into(), channel: Channel::Camera };
        ctx.send(&hello).await.unwrap();
        assert_eq!(srx.recv().await.unwrap(), hello);
        // Bigger than one Noise message: must be chunked and reassembled.
        let frame = Msg::Frame { jpeg: (0..300_000u32).map(|i| i as u8).collect() };
        stx.send(&frame).await.unwrap();
        stx.send(&Msg::Motion { dx: 1, dy: 2 }).await.unwrap();
        assert_eq!(crx.recv().await.unwrap(), frame);
        assert_eq!(crx.recv().await.unwrap(), Msg::Motion { dx: 1, dy: 2 });
    }

    #[tokio::test]
    async fn wrong_key_is_rejected() {
        let (srv, cli) = pair(PairingKey::generate(), PairingKey::generate()).await;
        // The responder cannot decrypt the initiator's PSK-bound first message.
        assert!(srv.is_err());
        // The initiator sees the connection close instead of a valid reply.
        assert!(cli.is_err());
    }
}
