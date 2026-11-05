//! Text clipboard sync in both directions (polling; both OSes lack a portable change event).
use std::sync::Mutex;
use std::time::Duration;

const MAX: usize = 1 << 20;
/// Last text seen or written by us, so a value received from the peer isn't echoed back.
static LAST: Mutex<Option<String>> = Mutex::new(None);

pub fn watch(send: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(mut cb) = arboard::Clipboard::new() else { return };
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if !crate::config::get().clipboard {
                continue;
            }
            let Ok(text) = cb.get_text() else { continue };
            let mut last = LAST.lock().unwrap();
            match last.as_ref() {
                None => *last = Some(text), // don't push whatever was there at start-up
                Some(l) if *l == text => {}
                Some(_) => {
                    *last = Some(text.clone());
                    drop(last);
                    if text.len() <= MAX {
                        send(text);
                    }
                }
            }
        }
    });
}

pub fn set_from_peer(text: String) {
    if !crate::config::get().clipboard || text.len() > MAX {
        return;
    }
    *LAST.lock().unwrap() = Some(text.clone());
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text);
    }
}
