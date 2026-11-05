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

pub async fn serve(frames: Frames) {
    let Ok(l) = TcpListener::bind(("127.0.0.1", PORT)).await else { return };
    loop {
        let Ok((mut s, _)) = l.accept().await else { continue };
        let mut rx = frames.clone();
        tokio::spawn(async move {
            let mut req = [0u8; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(2), s.read(&mut req)).await;
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
    fn part_framing() {
        let p = super::part(&[0xFF, 0xD8, 0xFF, 0xD9]);
        let s = String::from_utf8_lossy(&p);
        assert!(s.starts_with("--mirrormagicframe\r\nContent-Type: image/jpeg\r\nContent-Length: 4\r\n\r\n"));
        assert!(p.ends_with(&[0xFF, 0xD8, 0xFF, 0xD9, b'\r', b'\n']));
    }
}
