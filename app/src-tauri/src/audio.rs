#![allow(dead_code)] // capture() is used on the PC, play() on the Mac
//! PC microphone → Mac. The PC captures the webcam's microphone (mono 16-bit, device rate) and
//! sends 10 ms chunks next to the camera frames; the Mac plays them into the "BlackHole 2ch"
//! virtual audio device, which apps (Teams, Zoom, browser) pick as their microphone.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Mixes interleaved samples down to mono 16-bit.
pub fn to_mono_i16(data: &[f32], channels: usize) -> Vec<i16> {
    let ch = channels.max(1);
    data.chunks(ch).map(|f| ((f.iter().sum::<f32>() / ch as f32).clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

/// Keeps the playback buffer between `low` and `high` samples: after a network hiccup the
/// backlog is dropped instead of adding permanent delay.
pub fn trim_backlog(buf: &mut VecDeque<i16>, low: usize, high: usize) {
    if buf.len() > high {
        let drop = buf.len() - low;
        buf.drain(..drop);
    }
}

fn name(d: &cpal::Device) -> String {
    d.description().map(|x| x.name().to_string()).unwrap_or_default()
}

/// The webcam's microphone if there is one (e.g. "Mikrofon (HD Pro Webcam C920)"), else the default input.
// shortcut: name heuristic, add a device picker if someone has two webcams
fn pick_input(host: &cpal::Host) -> Option<cpal::Device> {
    let cam = host
        .input_devices()
        .ok()
        .and_then(|mut it| it.find(|d| ["webcam", "camera", "c920", "c922", "brio"].iter().any(|k| name(d).to_lowercase().contains(k))));
    cam.or_else(|| host.default_input_device())
}

/// Captures until `stop` is set. `chunk(rate, pcm)` gets 10 ms of mono audio at a time.
/// Returns the device name, or an error if no microphone could be opened.
pub fn capture(stop: Arc<AtomicBool>, chunk: impl FnMut(u32, Vec<i16>) + Send + 'static) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let Some(dev) = pick_input(&host) else { return tx.send(Err("no microphone".to_string())).unwrap_or(()) };
        let dev_name = name(&dev);
        let Ok(cfg) = dev.default_input_config() else { return tx.send(Err(format!("{dev_name}: no input format"))).unwrap_or(()) };
        let rate = cfg.sample_rate();
        let channels = cfg.channels() as usize;
        let per_chunk = (rate / 100) as usize;
        let pending = Arc::new(Mutex::new(Vec::<i16>::with_capacity(per_chunk * 2)));
        let chunk = Arc::new(Mutex::new(chunk));
        let push = {
            let pending = pending.clone();
            move |mono: Vec<i16>| {
                let mut p = pending.lock().unwrap();
                p.extend(mono);
                while p.len() >= per_chunk {
                    let out: Vec<i16> = p.drain(..per_chunk).collect();
                    (chunk.lock().unwrap())(rate, out);
                }
            }
        };
        let config: StreamConfig = cfg.config();
        let err = |e| log::warn!("microphone: {e}");
        let stream = match cfg.sample_format() {
            SampleFormat::F32 => dev.build_input_stream::<f32, _, _>(config, move |d, _| push(to_mono_i16(d, channels)), err, None),
            SampleFormat::I16 => dev.build_input_stream::<i16, _, _>(
                config,
                move |d, _| push(d.chunks(channels).map(|f| (f.iter().map(|&s| s as i32).sum::<i32>() / channels as i32) as i16).collect()),
                err,
                None,
            ),
            f => return tx.send(Err(format!("{dev_name}: unsupported sample format {f:?}"))).unwrap_or(()),
        };
        let stream = match stream.and_then(|s| s.play().map(|_| s)) {
            Ok(s) => s,
            Err(e) => return tx.send(Err(format!("{dev_name}: {e}"))).unwrap_or(()),
        };
        let _ = tx.send(Ok(format!("{dev_name} · {rate} Hz")));
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
        drop(stream); // the Stream is not Send on every host, so it lives and dies on this thread
    });
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "microphone did not start".to_string())?
}

/// Plays mono 16-bit samples into the first output device whose name contains `device`
/// (e.g. "BlackHole") until `stop` is set. Returns the queue to push samples into.
pub fn play(device: &'static str, rate: u32, stop: Arc<AtomicBool>) -> Result<Arc<Mutex<VecDeque<i16>>>, String> {
    let buf = Arc::new(Mutex::new(VecDeque::<i16>::new()));
    let (tx, rx) = std::sync::mpsc::channel();
    let b = buf.clone();
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let Some(dev) = host.output_devices().ok().and_then(|mut it| it.find(|d| name(d).contains(device))) else {
            return tx.send(Err(format!("{device} is not installed"))).unwrap_or(());
        };
        let Some(cfg) = dev.supported_output_configs().ok().and_then(|mut it| {
            it.find(|c| c.sample_format() == SampleFormat::F32 && c.min_sample_rate() <= rate && rate <= c.max_sample_rate())
        }) else {
            return tx.send(Err(format!("{device} does not support {rate} Hz"))).unwrap_or(());
        };
        let cfg = cfg.with_sample_rate(rate);
        let channels = cfg.channels() as usize;
        let (low, high) = (rate as usize / 25, rate as usize / 5); // 40 ms target, 200 ms max backlog
        let stream = dev.build_output_stream::<f32, _, _>(
            cfg.config(),
            move |out, _| {
                let mut q = b.lock().unwrap();
                trim_backlog(&mut q, low, high);
                for frame in out.chunks_mut(channels) {
                    let s = q.pop_front().unwrap_or(0) as f32 / 32768.0;
                    frame.iter_mut().for_each(|x| *x = s);
                }
            },
            |e| log::warn!("speaker: {e}"),
            None,
        );
        let stream = match stream.and_then(|s| s.play().map(|_| s)) {
            Ok(s) => s,
            Err(e) => return tx.send(Err(format!("{device}: {e}"))).unwrap_or(()),
        };
        let _ = tx.send(Ok(()));
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
        drop(stream);
    });
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| format!("{device} did not start"))??;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_and_clamp() {
        assert_eq!(to_mono_i16(&[1.0, 0.0, -0.5, -0.5, 2.0, 2.0], 2), vec![16383, -16383, 32767]);
        assert_eq!(to_mono_i16(&[0.25], 1), vec![8191]);
    }

    #[test]
    fn backlog_is_trimmed_to_the_target() {
        let mut q: VecDeque<i16> = (0..1000).collect();
        trim_backlog(&mut q, 100, 500);
        assert_eq!(q.len(), 100);
        assert_eq!(q.front(), Some(&900)); // newest audio kept
        let mut q: VecDeque<i16> = (0..400).collect();
        trim_backlog(&mut q, 100, 500);
        assert_eq!(q.len(), 400); // under the limit: untouched
    }
}
