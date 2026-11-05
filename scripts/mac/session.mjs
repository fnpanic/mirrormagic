// Drive a Tether session to the Mac for QA/builds without touching the user's desktop: the
// viewer runs on a hidden Windows desktop and is controlled over WebView2 CDP.
//   node session.mjs start [--file-transfer]     start viewer (PORT env selects CDP port)
//   node session.mjs stop                        kill viewers started by this script
//   node session.mjs shot <png>                  save the remote screen
//   node session.mjs keys <A+B> [<C> ...]        press key combos (DOM codes, e.g. MetaLeft+Space)
//   node session.mjs paste <text|@file>          paste text via clipboard sync (layout-safe), Windows clipboard restored
//   node session.mjs put <local> <remote>        upload a file   (needs a --file-transfer session)
//   node session.mjs get <remote> <local>        download a file (needs a --file-transfer session)
// Env: TETHER_EXE, MAC_ID (default 128974683), password from C:\vms\mac.pw (never printed).
import { createRequire } from "module";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire("C:/install/remote-support/apps/desktop/package.json");
const { chromium } = require("playwright-core");
const HERE = path.dirname(fileURLToPath(import.meta.url));
const PORT = +(process.env.PORT || 9281);
const EXE = process.env.TETHER_EXE || "C:\\Program Files\\Tether\\tether.exe";
const MAC_ID = process.env.MAC_ID || "128974683";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ps = (cmd) => execFileSync("powershell", ["-NoProfile", "-Command", cmd]).toString();

async function page() {
  for (let i = 0; i < 200; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const p = b.contexts().flatMap((c) => c.pages()).find((x) => x.url().includes("/session/"));
      if (p) return [b, p, p.url().match(/session\/([0-9a-f]{32})/)?.[1]];
      await b.close();
    } catch {}
    await sleep(300);
  }
  throw new Error("no session page on CDP port " + PORT);
}
const invoke = (p, c, a) => p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [c, a]);
async function combo(p, key, spec) {
  const ks = spec.split("+");
  const k = (code, down) => invoke(p, "session_key", { key, code, down, lockModes: 0, character: "" });
  for (const c of ks) { await k(c, true); await sleep(50); }
  for (const c of [...ks].reverse()) { await k(c, false); await sleep(50); }
  await sleep(300);
}
async function transfer(p, key, from, to, isRemote) {
  return p.evaluate(async ([k, f, t, rem]) => {
    const T = window.__TAURI_INTERNALS__; const id = 20000 + Math.floor(Math.random() * 9000); let res = null;
    const h = await T.invoke("plugin:event|listen", { event: "session", target: { kind: "Any" }, handler: T.transformCallback((e) => { const n = e.payload?.name; if ((n === "job_done" || n === "job_error") && e.payload.payload?.id === id) res = n + " " + JSON.stringify(e.payload.payload).slice(0, 200); }) });
    const inv = await T.invoke("fs_transfer", { key: k, id, path: f, to: t, fileNum: 0, hidden: false, isRemote: rem }).then(() => "ok", (e) => "ERR " + e);
    for (let i = 0; i < 480 && !res && inv === "ok"; i++) await new Promise((s) => setTimeout(s, 500));
    await T.invoke("plugin:event|unlisten", { event: "session", eventId: h }).catch(() => {});
    return res || inv;
  }, [key, from, to, isRemote]);
}

const [cmd, ...args] = process.argv.slice(2);
if (cmd === "start") {
  const pw = fs.readFileSync("C:/vms/mac.pw", "utf8").split(/\r?\n/)[0].trim();
  const env = { ...process.env, TETHER_PROFILE: "mm-" + PORT, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`, MMPW: pw };
  const pid = execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(HERE, "hidden-run.ps1"), "-Exe", EXE, "-Desktop", "mm-mac-" + PORT, "-Args", `--connect ${MAC_ID} --password-env MMPW ${args.join(" ")}`], { env }).toString().trim();
  const [b, p] = await page();
  for (let i = 0; i < 150 && !args.includes("--file-transfer"); i++) {
    if (await p.evaluate(() => { const c = document.querySelector("canvas"); return !!c && !(c.width === 300 && c.height === 150); })) break;
    await sleep(200);
  }
  console.log("viewer pid", pid, "|", (await p.evaluate(() => document.body.innerText)).slice(0, 200).replace(/\n+/g, " | "));
  await b.close();
} else if (cmd === "stop") {
  ps(`Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match '--password-env MMPW' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`);
} else if (cmd === "shot") {
  const [b, p] = await page();
  const r = await p.evaluate(() => {
    const c = document.querySelector("canvas"); const o = document.createElement("canvas");
    o.width = Math.min(c.width, 1600); o.height = Math.round(o.width * c.height / c.width);
    o.getContext("2d").drawImage(c, 0, 0, o.width, o.height);
    return { png: o.toDataURL("image/png"), cw: c.width, ch: c.height };
  });
  fs.writeFileSync(args[0], Buffer.from(r.png.split(",")[1], "base64"));
  console.log("canvas", r.cw, r.ch);
  await b.close();
} else if (cmd === "keys") {
  const [b, p, key] = await page();
  for (const c of args) await combo(p, key, c);
  await b.close();
} else if (cmd === "paste") {
  const text = args[0].startsWith("@") ? fs.readFileSync(args[0].slice(1), "utf8") : args.join(" ");
  const tmp = path.join(process.env.TEMP, "mm-paste.txt");
  fs.writeFileSync(tmp, text);
  const saved = ps("Get-Clipboard -Raw");
  const [b, p, key] = await page();
  try {
    ps(`Set-Clipboard -Value (Get-Content -Raw -LiteralPath '${tmp}')`);
    await sleep(1200); // clipboard sync to the Mac
    await combo(p, key, "MetaLeft+KeyV");
  } finally {
    await sleep(500);
    fs.writeFileSync(tmp, saved ?? "");
    ps(`Set-Clipboard -Value (Get-Content -Raw -LiteralPath '${tmp}')`);
    fs.rmSync(tmp, { force: true });
    await b.close();
  }
} else if (cmd === "put" || cmd === "get") {
  const [b, p, key] = await page();
  // fs_transfer: path = source, to = destination, isRemote = source is on the Mac
  console.log(await transfer(p, key, args[0], args[1], cmd === "get"));
  await b.close();
} else {
  console.log("usage: see header");
  process.exit(2);
}
