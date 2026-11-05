//! PC and Mac state machines talking over the real Noise channel on localhost:
//! edge crossing → motion → return edge → monitor hotkey → disconnect safety.
use mm_core::keymap::{Hotkey, KeyTranslator};
use mm_core::primary::{Action, Primary, PrimaryConfig, Rect, State};
use mm_core::proto::{Mode, Msg, Side};
use mm_core::secondary::{Displays, Effect, Secondary};
use mm_core::secure::{accept, connect, PairingKey};
use tokio::net::{TcpListener, TcpStream};

fn sends(actions: &[Action]) -> Vec<Msg> {
    actions.iter().filter_map(|a| if let Action::Send(m) = a { Some(m.to_msg()) } else { None }).collect()
}

#[tokio::test]
async fn full_session() {
    let key = PairingKey::generate();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let k2 = key.clone();
    let mac = tokio::spawn(async move { connect(TcpStream::connect(addr).await.unwrap(), &k2).await.unwrap() });
    let (s, _) = l.accept().await.unwrap();
    let (mut pc_tx, mut pc_rx) = accept(s, &key).await.unwrap();
    let (mut mac_tx, mut mac_rx) = mac.await.unwrap();

    let screen = Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 };
    let mut pc = Primary::new(PrimaryConfig { screen, monitors: vec![screen], side: Side::Left, monitor_hotkey: Hotkey::parse("Ctrl+Alt+M"), edge_enabled: true });
    pc.connected();
    let mut m = Secondary::new(Displays { edge: Rect { x: 0.0, y: 0.0, w: 1728.0, h: 1117.0 }, monitor: Some(Rect { x: 1728.0, y: 0.0, w: 2560.0, h: 1440.0 }) });
    let mut keys = KeyTranslator::new(true, true);

    // 1. cursor hits the left edge at 3/4 height → Mac cursor appears at its right edge
    let out = pc.mouse_move(0.0, 1080.0);
    assert!(out.block);
    for msg in sends(&out.actions) {
        pc_tx.send(&msg).await.unwrap();
    }
    let Msg::Enter { mode, side, along } = mac_rx.recv().await.unwrap() else { panic!() };
    let (x, y) = m.enter(mode, side, along);
    assert_eq!((mode, side, x), (Mode::Edge, Side::Right, 1726.0));
    assert!((y - 837.0).abs() < 1.0);

    // 2. motion left and German AltGr+Q (@) arrive as Option+L
    for msg in sends(&pc.mouse_move(1270.0, 720.0).actions) {
        pc_tx.send(&msg).await.unwrap();
    }
    for (scan, down) in [(0x21D, true), (0xE038, true), (0x10, true), (0x10, false), (0xE038, false), (0x21D, false)] {
        for msg in sends(&pc.key(scan, down).actions) {
            pc_tx.send(&msg).await.unwrap();
        }
    }
    let Msg::Motion { dx, dy } = mac_rx.recv().await.unwrap() else { panic!() };
    assert_eq!(m.motion(dx, dy, None), Some(Effect::MoveTo { x: 1716.0, y, dx: -10.0, dy: 0.0 }));
    let mut typed = vec![];
    for _ in 0..6 {
        let Msg::Key { scan, down } = mac_rx.recv().await.unwrap() else { panic!() };
        typed.extend(keys.translate(scan, down));
    }
    assert_eq!(typed, vec![(0x3D, true), (0x25, true), (0x25, false), (0x3D, false)]);

    // 3. pushing right past the Mac's edge hands control back
    let Some(Effect::Leave { along }) = m.motion(50, 0, None) else { panic!() };
    mac_tx.send(&Msg::Leave { along }).await.unwrap();
    let Msg::Leave { along } = pc_rx.recv().await.unwrap() else { panic!() };
    let out = pc.on_leave(along);
    assert_eq!(pc.state, State::Local);
    assert!(out.actions.contains(&Action::Warp(2.0, 1440.0 * along as f64)));

    // 4. monitor hotkey: cursor goes to the HDMI display centre; the hotkey again exits
    pc.key(0x1D, true);
    pc.key(0x38, true);
    let out = pc.key(0x32, true);
    assert!(out.actions.contains(&Action::SwitchMonitor(true)));
    for msg in sends(&out.actions) {
        pc_tx.send(&msg).await.unwrap();
    }
    let Msg::Enter { mode, side, along } = mac_rx.recv().await.unwrap() else { panic!() };
    assert_eq!(m.enter(mode, side, along), (3008.0, 720.0));

    // 5. the Mac vanishes mid-session: the PC gets its input back without sending anything
    drop(mac_tx);
    drop(mac_rx);
    assert!(pc_rx.recv().await.is_err());
    let out = pc.disconnected();
    assert_eq!(pc.state, State::Local);
    assert!(out.actions.contains(&Action::HideCursor(false)));
    assert!(sends(&out.actions).is_empty());
}
