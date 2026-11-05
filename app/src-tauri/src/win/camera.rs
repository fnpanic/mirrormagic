//! Webcam capture: the camera's own MJPEG stream via Media Foundation (no re-encode, low CPU).
//! Runs only while the Mac wants the camera, so the camera light means "the Mac is using it".
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
use nokhwa::Camera;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Capture {
    stop: Arc<AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Starts capturing on a thread; `frame` gets each JPEG, `state` gets (on, detail) changes.
pub fn start(
    width: u32,
    height: u32,
    fps: u32,
    frame: impl Fn(Vec<u8>) + Send + 'static,
    state: impl Fn(bool, String) + Send + 'static,
) -> Capture {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::spawn(move || {
        let fmt = CameraFormat::new(Resolution::new(width, height), FrameFormat::MJPEG, fps);
        let req = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(fmt));
        let mut cam = match Camera::new(CameraIndex::Index(0), req) {
            Ok(c) => c,
            Err(e) => return state(false, format!("no camera: {e}")),
        };
        if let Err(e) = cam.open_stream() {
            return state(false, format!("camera busy or unavailable: {e}"));
        }
        let f = cam.camera_format();
        if f.format() != FrameFormat::MJPEG {
            let _ = cam.stop_stream();
            return state(false, format!("camera offers no MJPEG mode (got {:?})", f.format()));
        }
        state(true, format!("{} · {}×{} @ {} fps", cam.info().human_name(), f.width(), f.height(), f.frame_rate()));
        let mut errors = 0;
        while !s.load(Ordering::SeqCst) {
            match cam.frame_raw() {
                Ok(buf) => {
                    errors = 0;
                    frame(buf.into_owned());
                }
                Err(e) => {
                    errors += 1;
                    if errors > 30 {
                        state(false, format!("camera stopped: {e}"));
                        break;
                    }
                }
            }
        }
        let _ = cam.stop_stream();
        if s.load(Ordering::SeqCst) {
            state(false, "idle".into());
        }
    });
    Capture { stop }
}
