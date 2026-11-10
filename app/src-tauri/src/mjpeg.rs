#![cfg_attr(not(target_os = "macos"), allow(dead_code))]
//! Serves the PC camera as `multipart/x-mixed-replace` MJPEG on 127.0.0.1 only (OBS reads it).
//! Between streams a placeholder frame keeps the connection alive, so OBS never has to reconnect.
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

pub const PORT: u16 = 24851;
pub const URL: &str = "http://127.0.0.1:24851/camera.mjpg";
const BOUNDARY: &str = "mirrormagicframe";
static PLACEHOLDER: &[u8] = include_bytes!("../assets/camera-idle.jpg");

pub type Frames = watch::Receiver<Option<Arc<Vec<u8>>>>;

pub fn part(jpeg: &[u8]) -> Vec<u8> {
    let mut v = format!("--{BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", jpeg.len()).into_bytes();
    v.extend_from_slice(jpeg);
    v.extend_from_slice(b"\r\n");
    v
}

/// Only requests addressed to this loopback endpoint: a web page that re-points its own host
/// name at 127.0.0.1 (DNS rebinding) still sends that name as Host.
fn host_ok(req: &[u8]) -> bool {
    let text = String::from_utf8_lossy(req).to_ascii_lowercase();
    match text.lines().find_map(|l| l.strip_prefix("host:")) {
        Some(h) => matches!(h.trim(), "127.0.0.1:24851" | "localhost:24851"),
        // HTTP/1.0 clients send none, but then the whole header must be here: a request line
        // that fills the buffer would hide the Host line behind it
        None => text.contains("\r\n\r\n"),
    }
}

pub async fn serve(frames: Frames) {
    let l = match TcpListener::bind(("127.0.0.1", PORT)).await {
        Ok(l) => l,
        Err(e) => {
            crate::status::log(format!("camera: port {PORT} unavailable ({e}): OBS gets no picture"));
            return crate::status::update(|s| s.camera_detail = format!("port {PORT} is in use by another program: {e}"));
        }
    };
    loop {
        let Ok((mut s, _)) = l.accept().await else { continue };
        let mut rx = frames.clone();
        tokio::spawn(async move {
            let mut req = [0u8; 1024];
            let n = tokio::time::timeout(Duration::from_secs(2), s.read(&mut req)).await.ok().and_then(Result::ok).unwrap_or(0);
            if !host_ok(&req[..n]) {
                return;
            }
            let head = format!("HTTP/1.0 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary={BOUNDARY}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
            if s.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            let _ = s.set_nodelay(true);
            loop {
                // new frame, or the placeholder twice a second while the PC camera is off
                let frame = match tokio::time::timeout(Duration::from_millis(500), rx.changed()).await {
                    Ok(Ok(())) => rx.borrow_and_update().clone(),
                    Ok(Err(_)) => return,
                    Err(_) => None,
                };
                let bytes = match &frame {
                    Some(f) => part(f),
                    None if rx.borrow().is_none() => part(PLACEHOLDER),
                    None => continue, // camera on but between frames
                };
                if s.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_loopback_hosts_are_served() {
        assert!(super::host_ok(b"GET /camera.mjpg HTTP/1.1\r\nHost: 127.0.0.1:24851\r\n\r\n"));
        assert!(super::host_ok(b"GET / HTTP/1.1\r\nhost: LOCALHOST:24851\r\n\r\n"));
        assert!(super::host_ok(b"GET / HTTP/1.0\r\n\r\n"));
        assert!(!super::host_ok(b"GET / HTTP/1.1\r\nHost: evil.example:24851\r\n\r\n"));
        assert!(!super::host_ok(format!("GET /{} HTTP/1.1\r\n", "a".repeat(1000)).as_bytes())); // Host cut off
        assert!(!super::host_ok(b""));
    }

    #[test]
    fn part_framing() {
        let p = super::part(&[0xFF, 0xD8, 0xFF, 0xD9]);
        let s = String::from_utf8_lossy(&p);
        assert!(s.starts_with("--mirrormagicframe\r\nContent-Type: image/jpeg\r\nContent-Length: 4\r\n\r\n"));
        assert!(p.ends_with(&[0xFF, 0xD8, 0xFF, 0xD9, b'\r', b'\n']));
    }
}
