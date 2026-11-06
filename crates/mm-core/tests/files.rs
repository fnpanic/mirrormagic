//! Clipboard files over the real Noise channel: a folder with nested files and a binary file
//! arrives byte-identical; an unsafe path from a malicious peer is refused.
use mm_core::files::{plan, recv, send};
use mm_core::proto::Msg;
use mm_core::secure::{accept, connect, PairingKey};
use std::path::PathBuf;
use tokio::net::{TcpListener, TcpStream};

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mm-files-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

async fn pair() -> ((mm_core::secure::Sender, mm_core::secure::Receiver), (mm_core::secure::Sender, mm_core::secure::Receiver)) {
    let key = PairingKey::generate();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let k2 = key.clone();
    let c = tokio::spawn(async move { connect(TcpStream::connect(addr).await.unwrap(), &k2).await.unwrap() });
    let (s, _) = l.accept().await.unwrap();
    (accept(s, &key).await.unwrap(), c.await.unwrap())
}

#[tokio::test]
async fn folder_and_big_binary_arrive_identical() {
    let src = tmp("src");
    std::fs::create_dir_all(src.join("Report/img")).unwrap();
    std::fs::write(src.join("Report/notes.txt"), "Grüße ✓").unwrap();
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(src.join("Report/img/photo.bin"), &big).unwrap();
    std::fs::write(src.join("single.pdf"), b"%PDF-1.7").unwrap();
    let p = plan(&[src.join("Report"), src.join("single.pdf")]).unwrap();
    assert_eq!(p.tops, vec!["Report", "single.pdf"]);
    assert_eq!(p.bytes, big.len() as u64 + "Grüße ✓".len() as u64 + 8);

    let ((mut tx, _), (_, mut rx)) = pair().await;
    let dest = tmp("dest");
    let d2 = dest.clone();
    let r = tokio::spawn(async move { recv(&mut rx, &d2, |_| {}).await.unwrap() });
    send(&mut tx, &p).await.unwrap();
    let tops = r.await.unwrap();
    assert_eq!(tops, vec![dest.join("Report"), dest.join("single.pdf")]);
    assert_eq!(std::fs::read(dest.join("Report/img/photo.bin")).unwrap(), big);
    assert_eq!(std::fs::read_to_string(dest.join("Report/notes.txt")).unwrap(), "Grüße ✓");
    assert_eq!(std::fs::read(dest.join("single.pdf")).unwrap(), b"%PDF-1.7");
}

#[tokio::test]
async fn path_traversal_from_peer_is_refused() {
    let ((mut tx, _), (_, mut rx)) = pair().await;
    let dest = tmp("evil");
    tx.send(&Msg::FileEntry { path: "../../escape.txt".into(), size: 1, dir: false }).await.unwrap();
    let err = recv(&mut rx, &dest, |_| {}).await.unwrap_err().to_string();
    assert!(err.contains("unsafe path"), "{err}");
    assert!(!dest.parent().unwrap().parent().unwrap().join("escape.txt").exists());
}
