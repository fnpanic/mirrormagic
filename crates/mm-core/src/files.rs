//! Clipboard files: a copied set of files/folders travels to the other machine on its own
//! encrypted connection (input never waits behind a big file) and lands in a folder there.
use crate::proto::Msg;
use crate::secure::{Error, Receiver, Sender};
use std::path::{Component, Path, PathBuf};

/// Refuse transfers above this size (bytes) and item count.
pub const MAX_BYTES: u64 = 4 << 30;
pub const MAX_ITEMS: usize = 50_000;
const CHUNK: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub src: PathBuf,
    /// Relative path with '/' separators, starting with the copied item's own name.
    pub rel: String,
    pub size: u64,
    pub dir: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<Item>,
    pub bytes: u64,
    /// Names of the copied (top-level) items.
    pub tops: Vec<String>,
}

/// Everything below the copied paths (folders recursively, symlinks skipped).
pub fn plan(paths: &[PathBuf]) -> Result<Plan, String> {
    fn walk(p: &Path, rel: String, out: &mut Plan) -> Result<(), String> {
        let md = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if md.file_type().is_symlink() {
            return Ok(());
        }
        if out.items.len() >= MAX_ITEMS {
            return Err(format!("more than {MAX_ITEMS} files"));
        }
        if md.is_dir() {
            out.items.push(Item { src: p.into(), rel: rel.clone(), size: 0, dir: true });
            let mut kids: Vec<_> = std::fs::read_dir(p).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
            kids.sort_by_key(|e| e.file_name());
            for k in kids {
                walk(&k.path(), format!("{rel}/{}", k.file_name().to_string_lossy()), out)?;
            }
        } else {
            out.bytes += md.len();
            if out.bytes > MAX_BYTES {
                return Err(format!("more than {} GB", MAX_BYTES >> 30));
            }
            out.items.push(Item { src: p.into(), rel, size: md.len(), dir: false });
        }
        Ok(())
    }
    let mut out = Plan::default();
    for p in paths {
        let name = p.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
        out.tops.push(name.clone());
        walk(p, name, &mut out)?;
    }
    Ok(out)
}

/// A relative path from the peer, made safe: no absolute paths, drive letters or `..`.
/// Characters Windows forbids in names become `_` (a Finder "/" arrives as ':'), so one awkward
/// name does not fail the whole transfer.
pub fn safe_rel(rel: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in rel.split(['/', '\\']) {
        let b = part.as_bytes();
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." || (b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic()) {
            return None;
        }
        let clean: String = part.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
        // Windows drops trailing dots and spaces ("..." would become nothing) and treats NUL, COM1 … as devices
        let mut clean = clean.trim_end_matches(['.', ' ']).to_string();
        let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit()) {
            clean.insert(0, '_');
        }
        match Path::new(&clean).components().next() {
            Some(Component::Normal(_)) => out.push(clean),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

pub async fn send(tx: &mut Sender, plan: &Plan) -> Result<(), Error> {
    use tokio::io::AsyncReadExt;
    let mut buf = vec![0u8; CHUNK];
    for it in &plan.items {
        tx.send(&Msg::FileEntry { path: it.rel.clone(), size: it.size, dir: it.dir }).await?;
        if it.dir {
            continue;
        }
        let mut f = tokio::fs::File::open(&it.src).await?;
        let mut left = it.size;
        while left > 0 {
            let n = f.read(&mut buf[..CHUNK.min(left as usize)]).await?;
            if n == 0 {
                return Err(format!("{} shrank while sending", it.rel).into());
            }
            tx.send(&Msg::FileData { data: buf[..n].to_vec() }).await?;
            left -= n as u64;
        }
    }
    tx.send(&Msg::FilesEnd).await
}

/// Receives into `dest` and returns the top-level paths (what goes on the clipboard).
pub async fn recv(rx: &mut Receiver, dest: &Path, mut progress: impl FnMut(u64)) -> Result<Vec<PathBuf>, Error> {
    use tokio::io::AsyncWriteExt;
    let (mut tops, mut total, mut count) = (Vec::<PathBuf>::new(), 0u64, 0usize);
    loop {
        match rx.recv().await? {
            Msg::FilesEnd => return Ok(tops),
            Msg::FileEntry { path, size, dir } => {
                count += 1;
                total = total.saturating_add(size); // a wrapped sum would pass the limit
                if count > MAX_ITEMS || total > MAX_BYTES {
                    return Err("transfer too large".into());
                }
                let rel = safe_rel(&path).ok_or_else(|| format!("refused unsafe path {path:?}"))?;
                let target = dest.join(&rel);
                let top = dest.join(rel.components().next().unwrap());
                if !tops.contains(&top) {
                    tops.push(top);
                }
                if dir {
                    tokio::fs::create_dir_all(&target).await?;
                    continue;
                }
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut f = tokio::fs::File::create(&target).await?;
                let mut left = size;
                while left > 0 {
                    let Msg::FileData { data } = rx.recv().await? else { return Err("file data expected".into()) };
                    if data.len() as u64 > left {
                        return Err("more data than announced".into());
                    }
                    f.write_all(&data).await?;
                    left -= data.len() as u64;
                    progress(data.len() as u64);
                }
                f.flush().await?;
            }
            other => return Err(format!("unexpected {other:?}").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_paths_are_refused() {
        assert_eq!(safe_rel("a/b.txt"), Some(PathBuf::from("a").join("b.txt")));
        assert_eq!(safe_rel("Folder\\x.png"), Some(PathBuf::from("Folder").join("x.png")));
        assert_eq!(safe_rel("../etc/passwd"), None);
        assert_eq!(safe_rel("a/../../x"), None);
        assert_eq!(safe_rel("/etc/passwd"), Some(PathBuf::from("etc").join("passwd"))); // leading '/' only splits
        assert_eq!(safe_rel("C:/Windows/x"), None);
        assert_eq!(safe_rel(""), None);
        assert_eq!(safe_rel("a/./b"), None);
        // names Windows refuses are renamed, not refused
        assert_eq!(safe_rel("Notes 10:11?.txt"), Some(PathBuf::from("Notes 10_11_.txt")));
        assert_eq!(safe_rel("a/.../x"), None);
        assert_eq!(safe_rel(".. /x"), None);
        assert_eq!(safe_rel("nul.txt"), Some(PathBuf::from("_nul.txt")));
        assert_eq!(safe_rel("report. "), Some(PathBuf::from("report")));
    }

    #[tokio::test]
    async fn announced_sizes_cannot_wrap_past_the_limit() {
        use crate::secure::{accept, connect, PairingKey};
        let key = PairingKey::generate();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let k = key.clone();
        let peer = tokio::spawn(async move {
            let (mut tx, _rx) = connect(tokio::net::TcpStream::connect(addr).await.unwrap(), &k).await.unwrap();
            tx.send(&Msg::FileEntry { path: "a".into(), size: 1, dir: false }).await.unwrap();
            tx.send(&Msg::FileData { data: vec![0] }).await.unwrap();
            let _ = tx.send(&Msg::FileEntry { path: "b".into(), size: u64::MAX, dir: false }).await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        });
        let (_tx, mut rx) = accept(l.accept().await.unwrap().0, &key).await.unwrap();
        let dest = std::env::temp_dir().join(format!("mm-wrap-test-{}", std::process::id()));
        let r = recv(&mut rx, &dest, |_| {}).await;
        let _ = std::fs::remove_dir_all(&dest);
        assert_eq!(r.unwrap_err().to_string(), "transfer too large");
        peer.await.unwrap();
    }
}
