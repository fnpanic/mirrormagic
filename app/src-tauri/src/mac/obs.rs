//! OBS automation: OBS's signed virtual camera is the only way to add a camera on macOS without an
//! Apple Developer ID. We enable obs-websocket, start OBS hidden, add a "PC camera" source that
//! reads our localhost MJPEG stream, and start the virtual camera.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const APP: &str = "/Applications/OBS.app";
const PORT: u16 = 4455;
pub const SCENE: &str = "MirrorMagic";
pub const INPUT: &str = "PC camera";

pub fn installed() -> bool {
    std::path::Path::new(APP).exists()
}

pub fn running() -> bool {
    std::process::Command::new("pgrep").args(["-x", "OBS"]).output().map(|o| o.status.success()).unwrap_or(false)
        || std::process::Command::new("pgrep").args(["-x", "obs"]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn ws_config_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json")
}

/// Enables obs-websocket with our password. Only safe while OBS is not running (it rewrites the file on exit).
fn write_ws_config(password: &str) -> std::io::Result<()> {
    let p = ws_config_path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    let mut v: Value = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    v["server_enabled"] = json!(true);
    v["server_port"] = json!(PORT);
    v["auth_required"] = json!(true);
    v["server_password"] = json!(password);
    v["alerts_enabled"] = json!(false);
    v["first_load"] = json!(false);
    std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap())
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Client {
    ws: Ws,
    next: u64,
}

impl Client {
    async fn connect(password: &str) -> Result<Client, String> {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{PORT}")).await.map_err(|e| e.to_string())?;
        let hello = Self::read(&mut ws).await?;
        let mut d = json!({ "rpcVersion": 1, "eventSubscriptions": 0 });
        if let Some(a) = hello["d"]["authentication"].as_object() {
            let (salt, challenge) = (a["salt"].as_str().unwrap_or(""), a["challenge"].as_str().unwrap_or(""));
            let secret = B64.encode(Sha256::digest(format!("{password}{salt}")));
            d["authentication"] = json!(B64.encode(Sha256::digest(format!("{secret}{challenge}"))));
        }
        ws.send(Message::Text(json!({ "op": 1, "d": d }).to_string().into())).await.map_err(|e| e.to_string())?;
        let ident = Self::read(&mut ws).await?;
        if ident["op"] != 2 {
            return Err("OBS refused the password".into());
        }
        Ok(Client { ws, next: 1 })
    }

    async fn read(ws: &mut Ws) -> Result<Value, String> {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => return serde_json::from_str(&t).map_err(|e| e.to_string()),
                Some(Ok(Message::Close(f))) => return Err(format!("OBS closed the connection: {f:?}")),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("OBS closed the connection".into()),
            }
        }
    }

    async fn call(&mut self, ty: &str, data: Value) -> Result<Value, String> {
        let id = self.next.to_string();
        self.next += 1;
        let req = json!({ "op": 6, "d": { "requestType": ty, "requestId": id, "requestData": data } });
        self.ws.send(Message::Text(req.to_string().into())).await.map_err(|e| e.to_string())?;
        loop {
            let v = Self::read(&mut self.ws).await?;
            if v["op"] == 7 && v["d"]["requestId"] == id.as_str() {
                let st = &v["d"]["requestStatus"];
                if st["result"] == true {
                    return Ok(v["d"]["responseData"].clone());
                }
                return Err(format!("{ty}: {}", st["comment"].as_str().unwrap_or("failed")));
            }
        }
    }
}

/// Makes OBS feed the virtual camera from our stream. `start_obs`: launch it if not running.
pub async fn ensure(stream_url: &str, start_obs: bool) -> Result<String, String> {
    if !installed() {
        return Err("OBS is not installed (get it from obsproject.com)".into());
    }
    let mut password = crate::config::get().obs_password;
    if !running() {
        if !start_obs {
            return Err("OBS is not running".into());
        }
        if password.is_empty() {
            password = format!("{:032x}", rand::random::<u128>());
            crate::config::update(|c| c.obs_password = password.clone());
        }
        write_ws_config(&password).map_err(|e| format!("cannot configure OBS: {e}"))?;
        std::process::Command::new("open")
            .args(["-g", "-a", APP, "--args", "--minimize-to-tray", "--disable-shutdown-check", "--disable-updater"])
            .spawn()
            .map_err(|e| format!("cannot start OBS: {e}"))?;
    }
    let mut c = None;
    for _ in 0..40 {
        match Client::connect(&password).await {
            Ok(x) => {
                c = Some(x);
                break;
            }
            Err(e) if e.contains("password") => return Err("OBS uses a different websocket password: quit OBS and press Set up again".into()),
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    let mut c = c.ok_or("OBS remote control is off: quit OBS and press Set up again")?;

    let scenes = c.call("GetSceneList", json!({})).await?;
    let has_scene = scenes["scenes"].as_array().is_some_and(|a| a.iter().any(|s| s["sceneName"] == SCENE));
    if !has_scene {
        c.call("CreateScene", json!({ "sceneName": SCENE })).await?;
    }
    let settings = json!({
        "is_local_file": false, "input": stream_url, "input_format": "mpjpeg",
        "buffering_mb": 0, "reconnect_delay_sec": 1, "restart_on_activate": false,
        "close_when_inactive": false, "hw_decode": true, "clear_on_media_end": false,
    });
    let inputs = c.call("GetInputList", json!({})).await?;
    let has_input = inputs["inputs"].as_array().is_some_and(|a| a.iter().any(|i| i["inputName"] == INPUT));
    if has_input {
        c.call("SetInputSettings", json!({ "inputName": INPUT, "inputSettings": settings })).await?;
    } else {
        c.call("CreateInput", json!({ "sceneName": SCENE, "inputName": INPUT, "inputKind": "ffmpeg_source", "inputSettings": settings, "sceneItemEnabled": true })).await?;
    }
    // fill the canvas whatever the camera resolution
    let video = c.call("GetVideoSettings", json!({})).await?;
    if let Ok(item) = c.call("GetSceneItemId", json!({ "sceneName": SCENE, "sourceName": INPUT })).await {
        let _ = c
            .call("SetSceneItemTransform", json!({ "sceneName": SCENE, "sceneItemId": item["sceneItemId"], "sceneItemTransform": {
                "boundsType": "OBS_BOUNDS_SCALE_INNER", "boundsWidth": video["baseWidth"], "boundsHeight": video["baseHeight"],
                "boundsAlignment": 0, "alignment": 5, "positionX": 0, "positionY": 0 } }))
            .await;
    }
    c.call("SetCurrentProgramScene", json!({ "sceneName": SCENE })).await?;
    let vc = c.call("GetVirtualCamStatus", json!({})).await?;
    if vc["outputActive"] != true {
        c.call("StartVirtualCam", json!({})).await?;
    }
    Ok("OBS Virtual Camera is on and shows the PC camera".into())
}
