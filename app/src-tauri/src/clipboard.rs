//! Clipboard hand-over: when control moves to the other machine (edge crossing or monitor
//! hotkey) the side giving up control sends its clipboard (text, HTML, image), and the receiver
//! writes it as a normal clipboard change, so clipboard managers on both sides record it.
//! No polling: nothing is read or written between switches.
use mm_core::proto::Msg;
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

const MAX_TEXT: usize = 8 << 20;
const MAX_PNG: usize = 12 << 20;
/// Whole message stays under the channel limit (16 MiB) with room to spare.
const MAX_TOTAL: usize = 14 << 20;

/// Fingerprint of what the clipboard held when it last crossed (either way), so an unchanged
/// clipboard isn't sent back and forth (and clipboard managers don't get duplicates).
static LAST: Mutex<Option<u64>> = Mutex::new(None);
/// One hand-over at a time; a newer switch waits for the previous read to finish.
static BUSY: Mutex<()> = Mutex::new(());

fn fingerprint(text: Option<&str>, image: Option<(usize, usize, &[u8])>) -> u64 {
    // HTML is left out: Windows wraps it in a CF_HTML header, so it never round-trips byte-exact
    let mut h = DefaultHasher::new();
    text.unwrap_or("").trim_end().hash(&mut h);
    if let Some((w, hgt, px)) = image {
        (w, hgt).hash(&mut h);
        px.hash(&mut h);
    }
    h.finish()
}

fn encode_png(w: usize, h: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    Some((info.width as usize, info.height as usize, buf))
}

/// Reads the clipboard; None if it is empty, unchanged since the last hand-over, or sharing is off.
fn take() -> Option<Msg> {
    if !crate::config::get().clipboard {
        return None;
    }
    let mut cb = arboard::Clipboard::new().ok()?;
    // copied files (Explorer / Finder) go by an offer; the other side fetches them on their own connection
    if let Some(list) = cb.get().file_list().ok().filter(|l| !l.is_empty()) {
        return offer_files(list);
    }
    let text = cb.get_text().ok().filter(|t| !t.is_empty() && t.len() <= MAX_TEXT);
    let html = cb.get().html().ok().filter(|t| !t.is_empty() && t.len() <= MAX_TEXT);
    let img = cb.get_image().ok();
    let fp = fingerprint(text.as_deref(), img.as_ref().map(|i| (i.width, i.height, i.bytes.as_ref())));
    if text.is_none() && img.is_none() {
        return None;
    }
    {
        let mut last = LAST.lock().unwrap();
        if *last == Some(fp) {
            return None;
        }
        *last = Some(fp);
    }
    let mut png = img.and_then(|i| encode_png(i.width, i.height, &i.bytes)).filter(|p| p.len() <= MAX_PNG);
    let mut html = html;
    let size = |t: &Option<String>, h: &Option<String>, p: &Option<Vec<u8>>| t.as_ref().map_or(0, |s| s.len()) + h.as_ref().map_or(0, |s| s.len()) + p.as_ref().map_or(0, |v| v.len());
    // too big for one hand-over: drop the formatting first, then the picture
    if size(&text, &html, &png) > MAX_TOTAL {
        html = None;
    }
    if size(&text, &html, &png) > MAX_TOTAL {
        png = None;
    }
    Some(Msg::Clipboard { text, html, png })
}

/// "text (12 chars), formatting, image" for the status line.
fn describe(m: &Msg) -> String {
    if let Msg::FilesOffer { count, bytes, names, .. } = m {
        return format!("{count} file(s) ({} KB): {}", bytes / 1024, names.join(", "));
    }
    let Msg::Clipboard { text, html, png } = m else { return String::new() };
    let mut parts = vec![];
    if let Some(t) = text {
        parts.push(format!("text ({} chars)", t.chars().count()));
    }
    if html.is_some() {
        parts.push("formatting".into());
    }
    if let Some(p) = png {
        parts.push(format!("image ({} KB)", p.len() / 1024));
    }
    parts.join(", ")
}

// ---------------------------------------------------------------- files

/// The files this side offered last (id, what to send).
static OUTGOING: Mutex<Option<(u64, mm_core::files::Plan)>> = Mutex::new(None);

fn files_fingerprint(list: &[std::path::PathBuf]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in list {
        p.hash(&mut h);
        if let Ok(m) = std::fs::metadata(p) {
            m.len().hash(&mut h);
            m.modified().ok().hash(&mut h);
        }
    }
    h.finish()
}

fn offer_files(list: Vec<std::path::PathBuf>) -> Option<Msg> {
    let fp = files_fingerprint(&list);
    {
        let mut last = LAST.lock().unwrap();
        if *last == Some(fp) {
            return None;
        }
        *last = Some(fp);
    }
    let plan = match mm_core::files::plan(&list) {
        Ok(p) => p,
        Err(e) => {
            crate::status::update(|s| s.clipboard = format!("files not sent: {e}"));
            return None;
        }
    };
    let id = rand_id();
    let msg = Msg::FilesOffer { id, count: plan.items.iter().filter(|i| !i.dir).count() as u32, bytes: plan.bytes, names: plan.tops.clone() };
    *OUTGOING.lock().unwrap() = Some((id, plan));
    Some(msg)
}

fn rand_id() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (t as u64) ^ (std::process::id() as u64).rotate_left(32)
}

/// The plan for an offer this side made (the peer asks for it by id).
pub fn outgoing(id: u64) -> Option<mm_core::files::Plan> {
    OUTGOING.lock().unwrap().as_ref().filter(|(i, _)| *i == id).map(|(_, p)| p.clone())
}

/// Where received files land: <temp>/MirrorMagic/files/<id>. Older transfers are removed after a day.
pub fn files_dest(id: u64) -> std::path::PathBuf {
    let root = std::env::temp_dir().join("MirrorMagic").join("files");
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default().as_secs() > 86_400).unwrap_or(false);
            if old {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    root.join(id.to_string())
}

/// Puts received files on the clipboard (paste in Explorer / Finder copies them).
pub fn set_files(tops: Vec<std::path::PathBuf>) {
    let Ok(mut cb) = arboard::Clipboard::new() else { return };
    let r = cb.set().file_list(&tops);
    // remember them, so the next switch doesn't send them straight back
    let back = cb.get().file_list().unwrap_or(tops.clone());
    *LAST.lock().unwrap() = Some(files_fingerprint(&back));
    let names: Vec<String> = tops.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    crate::status::log(format!("clipboard: files ready {names:?} ({r:?})"));
    crate::status::update(|s| s.clipboard = format!("received files: {} — paste to copy", names.join(", ")));
}

/// Receives an offer's files on an open files channel and puts them on the clipboard.
pub async fn receive_files(rx: &mut mm_core::secure::Receiver, id: u64, bytes: u64) -> Result<(), mm_core::secure::Error> {
    let dest = files_dest(id);
    let mut got = 0u64;
    let mut last_shown = 0u64;
    let tops = mm_core::files::recv(rx, &dest, |n| {
        got += n;
        if bytes > 0 && got - last_shown > bytes / 20 {
            last_shown = got;
            let pct = got * 100 / bytes;
            crate::status::update(|s| s.clipboard = format!("receiving files… {pct}%"));
        }
    })
    .await?;
    set_files(tops);
    Ok(())
}

/// Called when control leaves this machine. Reads off the input thread and sends via `send`.
pub fn hand_over(send: impl FnOnce(Msg) + Send + 'static) {
    std::thread::spawn(move || {
        let _g = BUSY.lock().unwrap();
        if let Some(m) = take() {
            let what = describe(&m);
            crate::status::log(format!("clipboard: sending {what}"));
            send(m);
            crate::status::update(|s| s.clipboard = format!("sent {what}"));
        } else {
            crate::status::log("clipboard: nothing new to hand over (empty, unchanged, or sharing off)");
        }
    });
}

/// Writes what the other machine handed over.
pub fn receive(text: Option<String>, html: Option<String>, png: Option<Vec<u8>>) {
    if !crate::config::get().clipboard {
        return;
    }
    let what = describe(&Msg::Clipboard { text: text.clone(), html: None, png: None });
    let what = format!("{what}{}{}", if html.is_some() { " +formatting" } else { "" }, if png.is_some() { " +image" } else { "" });
    crate::status::update(|s| s.clipboard = format!("received {what}"));
    crate::status::log(format!("clipboard: received {what}"));
    let _g = BUSY.lock().unwrap();
    let Ok(mut cb) = arboard::Clipboard::new() else { return };
    let image = png.as_deref().and_then(decode_png);
    *LAST.lock().unwrap() = Some(fingerprint(text.as_deref(), image.as_ref().map(|(w, h, px)| (*w, *h, px.as_slice()))));
    let has_text = text.as_deref().is_some_and(|t| !t.trim().is_empty());
    // An image wins unless there is real text (a copied screenshot vs. a Word/browser selection).
    let r = match (image, html, text) {
        (Some((w, h, px)), _, _) if !has_text => cb.set_image(arboard::ImageData { width: w, height: h, bytes: Cow::Owned(px) }),
        (_, Some(html), text) => cb.set().html(html, text),
        (_, None, Some(text)) => cb.set_text(text),
        (Some((w, h, px)), None, None) => cb.set_image(arboard::ImageData { width: w, height: h, bytes: Cow::Owned(px) }),
        _ => Ok(()),
    };
    match &r {
        Ok(()) => crate::status::log("clipboard: written"),
        Err(e) => crate::status::log(format!("clipboard: write failed: {e}")),
    }
    if let Err(e) = r {
        log::warn!("clipboard write failed: {e}");
    }
    // Fingerprint what the OS now returns (images come back with converted pixels), so the
    // next hand-over recognises an unchanged clipboard and doesn't bounce it back.
    let text = cb.get_text().ok().filter(|t| !t.is_empty());
    let img = cb.get_image().ok();
    *LAST.lock().unwrap() = Some(fingerprint(text.as_deref(), img.as_ref().map(|i| (i.width, i.height, i.bytes.as_ref()))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip_keeps_pixels_and_fingerprint() {
        let px: Vec<u8> = (0..4 * 3 * 2).map(|i| (i * 37) as u8).collect();
        let png = encode_png(3, 2, &px).unwrap();
        let (w, h, back) = decode_png(&png).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(back, px);
        assert_eq!(fingerprint(Some("a"), Some((3, 2, &px))), fingerprint(Some("a\n"), Some((w, h, &back))));
        assert_ne!(fingerprint(Some("a"), None), fingerprint(Some("b"), None));
    }
}
