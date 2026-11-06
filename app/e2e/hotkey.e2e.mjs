// End-to-end check of the monitor-hotkey UI in a real (headless, invisible) browser against the
// built frontend, with the Tauri backend mocked: record a new combo, Reset back to Ctrl+Alt+M,
// and record Ctrl+Alt+M by keyboard again (also as AltGr+M, the German-layout case).
//   cd app && npm run build && node e2e/hotkey.e2e.mjs
import { createRequire } from "module";
import { spawn } from "node:child_process";
import assert from "node:assert/strict";

const require = createRequire("C:/install/remote-support/apps/desktop/package.json");
const { chromium } = require("playwright-core");
const PORT = 4179;
const server = spawn("npx", ["vite", "preview", "--port", String(PORT), "--strictPort", "--host", "127.0.0.1"], { shell: true, stdio: "ignore" });

const mock = () => {
  const status = { connected: true, peer: "Mac", peer_addr: "", rtt_ms: 3, control: "local", monitor_on_mac: false,
    ddc: { monitor: "VX3276", current: 15, inputs: [15, 17], last: "" }, camera_on: false, camera_detail: "", camera_wanted: false,
    mic_detail: "", can_inject: true, displays: [], obs: { installed: false, running: false, virtualcam: false, detail: "" }, last_error: "", clipboard: "" };
  const config = { clipboard: true, pairing_key: "ABCD", side: "Left", edge_enabled: true, monitor_hotkey: "Ctrl+Alt+M", mac_input: 17, pc_input: 15,
    camera_width: 1280, camera_height: 720, camera_fps: 30, host: "", speed: 1, invert_scroll: false, german_fixups: null, obs_password: "",
    camera_auto: true, ctrl_is_cmd: true, mic: true };
  window.__saved = [];
  let cb = 0;
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => ++cb,
    invoke: async (cmd, args) => {
      if (cmd === "snapshot") return { role: "pc", version: "test", config: { ...config }, pairingValid: true, hotkeyValid: true, autostart: true, status, inputs: [] };
      if (cmd === "save_config") { window.__saved.push(args.patch); Object.assign(config, args.patch); return null; }
      return null;
    },
  };
};

let browser;
try {
  for (let i = 0; i < 50; i++) { if (await fetch(`http://127.0.0.1:${PORT}`).then(() => true, () => false)) break; await new Promise((r) => setTimeout(r, 200)); }
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage();
  await page.addInitScript(mock);
  await page.goto(`http://127.0.0.1:${PORT}`);
  const field = page.locator("button.font-mono");
  const reset = page.getByRole("button", { name: "Reset" });
  const saved = () => page.evaluate(() => window.__saved.map((p) => p.monitor_hotkey));

  await field.waitFor();
  assert.equal(await field.innerText(), "Ctrl+Alt+M");
  assert.equal(await reset.count(), 0, "no Reset while the default is set");

  await field.click();
  await page.keyboard.press("Control+Alt+KeyQ");
  await page.getByRole("button", { name: "Ctrl+Alt+Q" }).waitFor();
  assert.equal(await reset.count(), 1, "Reset appears for a non-default combo");

  await reset.click();
  await page.getByRole("button", { name: "Ctrl+Alt+M" }).waitFor();
  assert.equal(await reset.count(), 0);

  // record Ctrl+Alt+Q, then Ctrl+Alt+M by keyboard (not Reset)
  await field.click();
  await page.keyboard.press("Control+Alt+KeyQ");
  await page.getByRole("button", { name: "Ctrl+Alt+Q" }).waitFor();
  await field.click();
  await page.keyboard.press("Control+Alt+KeyM");
  await page.getByRole("button", { name: "Ctrl+Alt+M" }).waitFor();

  // German layout: Windows reports Ctrl+Alt+M as AltGraph (ctrlKey/altKey false)
  await field.click();
  await page.keyboard.press("Control+Alt+KeyQ");
  await page.getByRole("button", { name: "Ctrl+Alt+Q" }).waitFor();
  await field.click();
  await field.evaluate((el) => {
    const e = new KeyboardEvent("keydown", { key: "µ", code: "KeyM", bubbles: true, cancelable: true });
    Object.defineProperty(e, "getModifierState", { value: (k) => k === "AltGraph" });
    el.dispatchEvent(e);
  });
  await page.getByRole("button", { name: "Ctrl+Alt+M" }).waitFor();

  assert.deepEqual(await saved(), ["Ctrl+Alt+Q", "Ctrl+Alt+M", "Ctrl+Alt+Q", "Ctrl+Alt+M", "Ctrl+Alt+Q", "Ctrl+Alt+M"]);
  console.log("hotkey e2e OK: record, Reset to default, re-record default (Ctrl+Alt and AltGr)");
} finally {
  await browser?.close();
  spawn("taskkill", ["/pid", String(server.pid), "/t", "/f"], { stdio: "ignore" });
}
