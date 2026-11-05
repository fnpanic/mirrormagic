import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Side = "Left" | "Right" | "Top" | "Bottom";
export interface Config {
  clipboard: boolean;
  pairing_key: string;
  side: Side;
  edge_enabled: boolean;
  monitor_hotkey: string;
  mac_input: number;
  pc_input: number;
  camera_width: number;
  camera_height: number;
  camera_fps: number;
  host: string;
  speed: number;
  invert_scroll: boolean;
  german_fixups: boolean | null;
  obs_password: string;
  camera_auto: boolean;
  ctrl_is_cmd: boolean;
  mic: boolean;
}
export interface DisplayInfo { name: string; builtin: boolean; x: number; y: number; w: number; h: number }
export interface Status {
  connected: boolean;
  peer: string;
  peer_addr: string;
  rtt_ms: number | null;
  control: "local" | "edge" | "monitor";
  monitor_on_mac: boolean;
  ddc: { monitor: string; current: number | null; inputs: number[]; last: string };
  camera_on: boolean;
  camera_detail: string;
  camera_wanted: boolean;
  mic_detail: string;
  can_inject: boolean;
  displays: DisplayInfo[];
  obs: { installed: boolean; running: boolean; virtualcam: boolean; detail: string };
  last_error: string;
  clipboard: string;
}
export interface Snapshot {
  role: "pc" | "mac";
  version: string;
  config: Config;
  pairingValid: boolean;
  hotkeyValid: boolean;
  autostart: boolean;
  status: Status;
  inputs: { value: number; name: string }[];
}

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  save: (patch: Partial<Config>) => invoke<void>("save_config", { patch }),
  setAutostart: (on: boolean) => invoke<void>("set_autostart", { on }),
  newPairingKey: () => invoke<string>("new_pairing_key"),
  setInput: (value: number) => invoke<string>("set_input", { value }),
  toggleMonitor: () => invoke<void>("toggle_monitor"),
  takeBack: () => invoke<void>("take_back"),
  discover: () => invoke<[string, string][]>("discover"),
  setupObs: () => invoke<string>("setup_obs"),
  cameraNow: (on: boolean) => invoke<void>("camera_now", { on }),
  openPrivacy: () => invoke<void>("open_privacy"),
  onStatus: (f: (s: Status) => void) => listen<Status>("status", (e) => f(e.payload)),
};
