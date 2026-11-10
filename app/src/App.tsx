import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Camera, Check, Copy, Eye, EyeOff, Keyboard, Laptop, Monitor, MousePointer2, RefreshCw, Search, ShieldAlert } from "lucide-react";
import { api, type Config, type Side, type Snapshot, type Status } from "./api";
import { DEFAULT_HOTKEY, hotkeyFromEvent, isUsableHotkey } from "./hotkey";

export default function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [error, setError] = useState("");

  const reload = useCallback(() => api.snapshot().then(setSnap).catch((e) => setError(String(e))), []);
  useEffect(() => {
    reload();
    const un = api.onStatus((status) => setSnap((s) => (s ? { ...s, status } : s)));
    // settings the backend changes by itself (the learned PC input) show when the window comes back
    window.addEventListener("focus", reload);
    return () => {
      window.removeEventListener("focus", reload);
      void un.then((f) => f());
    };
  }, [reload]);

  // for handlers that call the backend directly: a failure shows in the banner
  const attempt = (f: () => Promise<unknown>) => void f().catch((e) => setError(String(e)));

  const save = async (patch: Partial<Config>) => {
    setError("");
    try {
      await api.save(patch);
      await reload();
    } catch (e) {
      setError(String(e));
    }
  };

  if (!snap) return <div className="p-8 text-muted">{error ? <span role="alert" className="text-danger">MirrorMagic could not load its settings: {error}</span> : "Loading…"}</div>;
  const props = { snap, save, reload, setError, attempt };
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-[720px] px-6 py-6 flex flex-col gap-4">
        <Header s={snap.status} role={snap.role} version={snap.version} />
        {snap.notice && <p role="alert" className="rounded-md bg-tile px-4 py-3 text-caption text-warn">{snap.notice}{snap.role === "pc" ? " A new pairing key was made: enter it on the Mac." : ""}</p>}
        {error && <p role="alert" className="rounded-md bg-tile px-4 py-3 text-caption text-danger">{error}</p>}
        {snap.role === "pc" ? <PcView {...props} /> : <MacView {...props} />}
      </div>
    </div>
  );
}

type ViewProps = { snap: Snapshot; save: (p: Partial<Config>) => Promise<void>; reload: () => void; setError: (e: string) => void; attempt: (f: () => Promise<unknown>) => void };

// ---------------------------------------------------------------- shared pieces

function Header({ s, role, version }: { s: Status; role: string; version: string }) {
  const peer = s.peer || (role === "pc" ? "the Mac" : "the PC");
  const line = s.paused ? "Paused: no connection, keyboard and mouse stay on this machine"
    : !s.connected
    ? role === "pc" ? "Waiting for the Mac" : "Looking for the PC"
    : role === "pc" && !s.can_inject ? `${peer} is connected: allow Accessibility on the Mac to control it`
    : s.control === "edge" ? `${role === "pc" ? "Controlling" : "Controlled from"} ${peer}`
    : s.control === "monitor" ? "Monitor mode: the shared monitor shows the Mac"
    : `Connected to ${peer}`;
  const dot = s.paused ? "bg-track" : !s.connected ? "bg-warn" : s.control === "local" ? "bg-ok" : "bg-accent";
  return (
    <header className="flex items-end justify-between pb-2">
      <div>
        <h1 className="text-title font-semibold">MirrorMagic</h1>
        <p className="mt-1 flex items-center gap-2 text-callout text-ink-80">
          <span className={`inline-block size-2 rounded-full ${dot}`} />
          {line}
          {s.connected && s.rtt_ms != null && <span className="text-muted">· {s.rtt_ms.toFixed(1)} ms</span>}
        </p>
        {s.last_error && <p role="status" className="mt-1 text-footnote text-warn">{s.last_error}</p>}
      </div>
      <span className="text-footnote text-faint">{role === "pc" ? "PC" : "Mac"} · v{version}</span>
    </header>
  );
}

function Card({ icon, title, children, aside }: { icon: ReactNode; title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="rounded-2xl bg-tile p-5">
      <div className="mb-4 flex items-center justify-between">
        <h2 className="flex items-center gap-2 text-headline font-semibold">
          <span className="text-muted">{icon}</span>
          {title}
        </h2>
        {aside}
      </div>
      <div className="flex flex-col gap-4">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6">
      <div className="min-w-0">
        <div className="text-callout">{label}</div>
        {hint && <div className="text-footnote text-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)}
      className={`relative h-[26px] w-[44px] rounded-full ${on ? "bg-accent" : "bg-track"}`}>
      <span className={`absolute top-[3px] size-5 rounded-full bg-white transition-[left] duration-150 ${on ? "left-[21px]" : "left-[3px]"}`} />
    </button>
  );
}

function Btn({ children, onClick, primary, disabled, label }: { children: ReactNode; onClick: () => void; primary?: boolean; disabled?: boolean; label?: string }) {
  return (
    <button onClick={onClick} disabled={disabled} aria-label={label}
      className={`inline-flex h-9 items-center gap-2 rounded-full px-4 text-caption font-semibold disabled:opacity-40 ${primary ? "bg-accent text-white" : "bg-parchment text-ink"}`}>
      {children}
    </button>
  );
}

const field = "h-9 rounded-md border border-hairline bg-canvas px-3 text-caption text-ink";

function Shared({ snap, save, attempt }: ViewProps) {
  return (
    <Card icon={<RefreshCw size={18} />} title="General">
      <Row label="Share the clipboard" hint={snap.status.clipboard ? `Last switch: ${snap.status.clipboard}` : "Text, formatted text, images and files travel with the pointer when you switch"}>
        <Toggle label="Share the clipboard" on={snap.config.clipboard} onChange={(v) => save({ clipboard: v })} />
      </Row>
      <Row label={snap.role === "pc" ? "Start with Windows" : "Start at login"} hint="Runs quietly in the tray">
        <Toggle label="Start automatically" on={snap.autostart} onChange={(v) => attempt(async () => { await api.setAutostart(v); await save({}); })} />
      </Row>
      <Row label="Pause MirrorMagic" hint="Disconnects and stops sharing until you turn it off again (also in the tray menu)">
        <Toggle label="Pause MirrorMagic" on={snap.status.paused} onChange={(v) => attempt(async () => { await api.setPaused(v); await save({}); })} />
      </Row>
      <Row label="Quit MirrorMagic" hint="Stops it until the next login or until you start it again">
        <Btn onClick={() => void api.quit()}>Quit</Btn>
      </Row>
    </Card>
  );
}

// ---------------------------------------------------------------- PC

const SIDES: Side[] = ["Left", "Top", "Right", "Bottom"];

function LayoutPicker({ side, onPick }: { side: Side; onPick: (s: Side) => void }) {
  const mac: Record<Side, string> = {
    Left: "left-[4%] top-1/2 -translate-y-1/2",
    Right: "right-[4%] top-1/2 -translate-y-1/2",
    Top: "top-[4%] left-1/2 -translate-x-1/2",
    Bottom: "bottom-[4%] left-1/2 -translate-x-1/2",
  };
  const zone: Record<Side, string> = {
    Left: "left-0 top-0 h-full w-[24%]",
    Right: "right-0 top-0 h-full w-[24%]",
    Top: "top-0 left-[24%] h-[24%] w-[52%]",
    Bottom: "bottom-0 left-[24%] h-[24%] w-[52%]",
  };
  return (
    <div className="relative h-[220px] rounded-xl bg-parchment">
      {SIDES.filter((s) => s !== side).map((s) => (
        <button key={s} aria-label={`Mac is ${s.toLowerCase()} of the monitor`} onClick={() => onPick(s)}
          className={`absolute ${zone[s]} rounded-xl opacity-30 hover:opacity-100 focus-visible:opacity-100`}>
          <span className="absolute inset-3 rounded-lg border-2 border-dashed border-accent" />
        </button>
      ))}
      <div className="pointer-events-none absolute left-1/2 top-1/2 flex h-[110px] w-[190px] -translate-x-1/2 -translate-y-1/2 flex-col items-center justify-center rounded-lg border-2 border-ink bg-tile">
        <Monitor size={22} />
        <span className="mt-1 text-footnote font-semibold">This PC</span>
      </div>
      <div className={`pointer-events-none absolute ${mac[side]} flex h-[64px] w-[96px] flex-col items-center justify-center rounded-md border-2 border-accent bg-tile text-accent`}>
        <Laptop size={20} />
        <span className="text-footnote font-semibold">Mac</span>
      </div>
    </div>
  );
}

function HotkeyField({ value, onSave }: { value: string; onSave: (v: string) => void }) {
  const [rec, setRecState] = useState(false);
  // while recording, the PC's hook must not fire the current hotkey (pressing it again keeps it)
  const setRec = (on: boolean) => {
    if (on !== rec) void api.hotkeyRecording(on).catch(() => {});
    setRecState(on);
  };
  return (
    <button className={`${field} min-w-[140px] font-mono ${rec ? "border-accent text-accent" : ""}`}
      onClick={() => setRec(true)} onBlur={() => setRec(false)}
      onKeyDown={(e) => {
        if (!rec) return;
        e.preventDefault();
        if (e.key === "Escape") return setRec(false);
        const h = hotkeyFromEvent(e.nativeEvent);
        if (h && isUsableHotkey(h)) {
          setRec(false);
          onSave(h);
        }
      }}>
      {rec ? "Press keys…" : value}
    </button>
  );
}

function PcView(p: ViewProps) {
  const { snap, save } = p;
  const c = snap.config;
  const s = snap.status;
  const [copied, setCopied] = useState(false);
  const [shown, setShown] = useState(false);
  const [confirmKey, setConfirmKey] = useState(false);
  // from the live status: after another shared monitor is picked, its inputs arrive with the probe
  const known = (s.ddc.inputs.length ? s.ddc.inputs : [0x11, 0x12]).map((value) => ({ value, name: inputName(value) }));
  // the PC's own input is no choice for the Mac, but the configured value is always listed:
  // a select without its value shows another entry than the one in use
  const inputs = known.filter((i) => i.value !== c.pc_input || i.value === c.mac_input);
  if (!inputs.some((i) => i.value === c.mac_input)) inputs.push({ value: c.mac_input, name: inputName(c.mac_input) });
  return (
    <>
      <Card icon={<MousePointer2 size={18} />} title="Screen edge"
        aside={<Toggle label="Cross the edge to the Mac" on={c.edge_enabled} onChange={(v) => save({ edge_enabled: v })} />}>
        <LayoutPicker side={c.side} onPick={(side) => save({ side })} />
        <p className="text-footnote text-muted">
          Push the pointer past the {c.side.toLowerCase()} edge to control the Mac, and push it back to return.
          Drag-and-drop never crosses. <b className="font-semibold text-ink-80">Scroll Lock</b> always takes control back.
        </p>
        {s.game_mode && <p className="text-footnote text-accent">Game mode: a full-screen app is in front. MirrorMagic stays out of its input (no crossing, no hotkey, no added latency) until you leave it.</p>}
        {s.control !== "local" && <Btn onClick={api.takeBack}>Take control back</Btn>}
      </Card>

      <Card icon={<Monitor size={18} />} title="Monitor mode"
        aside={<Btn primary onClick={api.toggleMonitor}>{s.monitor_on_mac ? "Show the PC" : "Show the Mac"}</Btn>}>
        <Row label="Hotkey" hint="Switches the monitor to the Mac and puts keyboard and mouse there, and back">
          <div className="flex items-center gap-2">
            <HotkeyField value={c.monitor_hotkey} onSave={(monitor_hotkey) => save({ monitor_hotkey })} />
            {c.monitor_hotkey !== DEFAULT_HOTKEY && <Btn onClick={() => save({ monitor_hotkey: DEFAULT_HOTKEY })}>Reset</Btn>}
          </div>
        </Row>
        {s.ddc.monitors.length > 1 && (
          <Row label="Shared monitor" hint="The monitor both the PC and the Mac are plugged into">
            <select aria-label="Shared monitor" className={field} value={c.ddc_monitor || s.ddc.monitor} onChange={(e) => save({ ddc_monitor: e.target.value })}>
              {s.ddc.monitors.map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
          </Row>
        )}
        <Row label="The Mac is connected to" hint={s.ddc.monitor ? `${s.ddc.monitor} · the PC uses ${inputName(c.pc_input)}` : s.ddc.last || "Looking for the monitor…"}>
          <select aria-label="Monitor input of the Mac" className={field} value={c.mac_input} onChange={(e) => save({ mac_input: Number(e.target.value) })}>
            {inputs.map((i) => <option key={i.value} value={i.value}>{i.name}</option>)}
          </select>
        </Row>
        {s.ddc.last && <p className="text-footnote text-muted">Last switch: {s.ddc.last}</p>}
      </Card>

      <Card icon={<Keyboard size={18} />} title="Pairing">
        <Row label="Pairing key" hint="Enter this once in MirrorMagic on the Mac">
          <div className="flex items-center gap-2">
            {/* hidden until asked for: the key is all a machine needs, and windows get shared in calls */}
            <code className="rounded-md bg-parchment px-3 py-2 font-mono text-caption tracking-wider select-text">{shown ? c.pairing_key : c.pairing_key.replace(/[^-]/g, "•")}</code>
            <Btn label={shown ? "Hide the pairing key" : "Show the pairing key"} onClick={() => setShown(!shown)}>{shown ? <EyeOff size={16} /> : <Eye size={16} />}</Btn>
            <Btn label="Copy the pairing key" onClick={() => p.attempt(async () => { await navigator.clipboard.writeText(c.pairing_key); setCopied(true); setTimeout(() => setCopied(false), 1500); })}>
              {copied ? <Check size={16} /> : <Copy size={16} />}
            </Btn>
          </div>
        </Row>
        <Row label="New key" hint={confirmKey ? "The Mac disconnects at once and needs the new key typed in" : "Disconnects Macs that use the old key"}>
          {confirmKey ? (
            <div className="flex gap-2">
              <Btn onClick={() => setConfirmKey(false)}>Keep the key</Btn>
              <Btn primary onClick={() => p.attempt(async () => { setConfirmKey(false); await api.newPairingKey(); setShown(true); p.reload(); })}>Replace it</Btn>
            </div>
          ) : <Btn onClick={() => setConfirmKey(true)}>Generate</Btn>}
        </Row>
        {s.connected && <p className="text-footnote text-muted">Connected: {s.peer} ({s.peer_addr}){s.can_inject ? "" : " · the Mac has not allowed Accessibility yet"}</p>}
      </Card>

      <Card icon={<Camera size={18} />} title="Camera for the Mac">
        <Row label="Camera" hint={s.camera_detail || "Starts by itself when an app on the Mac opens “OBS Virtual Camera”"}>
          <div className="flex items-center gap-3">
            <span className={`text-caption ${s.camera_on ? "text-ok" : "text-muted"}`}>{s.camera_on ? "Streaming" : !c.camera ? "Off" : s.camera_wanted ? "Starting…" : "Idle"}</span>
            <Toggle label="Let the Mac use the webcam" on={c.camera} onChange={(v) => save({ camera: v })} />
          </div>
        </Row>
        <Row label="Microphone" hint={s.mic_detail || "A PC microphone streams while a Mac app uses its '– PC' device"}>
          <Toggle label="Let the Mac use the PC microphones" on={c.mic} onChange={(v) => save({ mic: v })} />
        </Row>
        <Row label="Resolution">
          <select aria-label="Camera resolution" className={field} value={`${c.camera_width}x${c.camera_height}`}
            onChange={(e) => { const [w, h] = e.target.value.split("x").map(Number); save({ camera_width: w, camera_height: h }); }}>
            <option value="1280x720">1280 × 720</option>
            <option value="1920x1080">1920 × 1080</option>
            <option value="640x480">640 × 480</option>
          </select>
        </Row>
      </Card>

      <Shared {...p} />
    </>
  );
}

function inputName(v: number) {
  return ({ 0x01: "VGA 1", 0x03: "DVI 1", 0x04: "DVI 2", 0x0f: "DisplayPort 1", 0x10: "DisplayPort 2", 0x11: "HDMI 1", 0x12: "HDMI 2", 0x1b: "USB-C" } as Record<number, string>)[v] ?? `input 0x${v.toString(16)}`;
}

// ---------------------------------------------------------------- Mac

function MacView(p: ViewProps) {
  const { snap, save } = p;
  const c = snap.config;
  const s = snap.status;
  const [key, setKey] = useState(c.pairing_key);
  const [host, setHost] = useState(c.host);
  const [found, setFound] = useState<[string, string][] | null>(null);
  const [finding, setFinding] = useState(false);
  const [speed, setSpeed] = useState(c.speed);
  const [obsBusy, setObsBusy] = useState(false);
  return (
    <>
      {!s.can_inject && (
        <div className="flex items-center justify-between gap-4 rounded-2xl bg-tile p-5">
          <div className="flex items-center gap-3">
            <ShieldAlert className="text-warn" />
            <div>
              <div className="text-callout font-semibold">Allow Accessibility</div>
              <div className="text-footnote text-muted">macOS only lets MirrorMagic move the pointer and type after you allow it.</div>
            </div>
          </div>
          <Btn primary onClick={api.openPrivacy}>Open settings</Btn>
        </div>
      )}

      <Card icon={<Monitor size={18} />} title="PC">
        <Row label="Pairing key" hint={snap.pairingValid ? "Saved" : "Shown in MirrorMagic on the PC"}>
          <div className="flex gap-2">
            <input aria-label="Pairing key" className={`${field} w-[290px] font-mono`} value={key} placeholder="XXXXX-XXXXX-XXXXX-XXXXX-XXXXXX" onChange={(e) => setKey(e.target.value)} />
            <Btn primary disabled={key.trim().toUpperCase() === c.pairing_key} onClick={() => save({ pairing_key: key.trim().toUpperCase() })}>Save</Btn>
          </div>
        </Row>
        <Row label="PC address" hint="Empty: find it on the network automatically">
          <div className="flex gap-2">
            <input aria-label="PC address" className={`${field} w-[180px]`} value={host} placeholder="automatic" onChange={(e) => setHost(e.target.value)} onBlur={() => host !== c.host && save({ host: host.trim() })} />
            <Btn disabled={finding} onClick={() => p.attempt(async () => { setFinding(true); try { setFound(await api.discover()); } finally { setFinding(false); } })}><Search size={16} />{finding ? "Looking…" : "Find"}</Btn>
          </div>
        </Row>
        {found && (
          <div className="flex flex-wrap gap-2 text-footnote">
            {found.length === 0 ? <span className="text-muted">No PC answered. Is MirrorMagic running on it?</span>
              : found.map(([ip, name]) => <Btn key={ip} onClick={() => { setHost(ip); save({ host: ip }); setFound(null); }}>{name} · {ip}</Btn>)}
          </div>
        )}
      </Card>

      <Card icon={<Laptop size={18} />} title="Screens">
        {s.displays.map((d) => (
          <Row key={`${d.x},${d.y}`} label={d.name} hint={`${Math.round(d.w)} × ${Math.round(d.h)} points`}>
            <span className="text-footnote text-muted">{d.builtin ? "Next to the PC monitor" : "Shared monitor (monitor mode)"}</span>
          </Row>
        ))}
        <Row label="Pointer speed" hint={`${speed.toFixed(2)}×`}>
          {/* saved when the slider is let go, not on every step of the drag */}
          <input aria-label="Pointer speed" type="range" min={0.5} max={2} step={0.05} value={speed} onChange={(e) => setSpeed(Number(e.target.value))}
            onPointerUp={() => speed !== c.speed && save({ speed })} onKeyUp={() => speed !== c.speed && save({ speed })} onBlur={() => speed !== c.speed && save({ speed })} className="w-[180px] accent-[var(--accent)]" />
        </Row>
        <Row label="Natural scrolling" hint="Reverse the PC mouse wheel">
          <Toggle label="Natural scrolling" on={c.invert_scroll} onChange={(v) => save({ invert_scroll: v })} />
        </Row>
        <Row label="Windows shortcuts" hint={c.windows_shortcuts ? "Ctrl works as ⌘ (Ctrl+C, Ctrl+V …); the Win key becomes Control" : "Mac layout: Ctrl = Control, Win = Option, Alt = ⌘. F-keys = Mac top row, Win+F-key = real F-key"}>
          <Toggle label="Windows shortcuts" on={c.windows_shortcuts} onChange={(v) => save({ windows_shortcuts: v })} />
        </Row>
        <Row label="German AltGr keys" hint="AltGr+Q types @, AltGr+8 types [ … like on the PC">
          <select aria-label="German AltGr keys" className={field} value={c.german_fixups === null ? "auto" : c.german_fixups ? "on" : "off"}
            onChange={(e) => save({ german_fixups: e.target.value === "auto" ? null : e.target.value === "on" })}>
            <option value="auto">Automatic</option>
            <option value="on">On</option>
            <option value="off">Off</option>
          </select>
        </Row>
      </Card>

      <Card icon={<Camera size={18} />} title="PC camera"
        aside={<Btn primary disabled={obsBusy} onClick={async () => { setObsBusy(true); try { await api.setupObs(); } catch (e) { p.setError(String(e)); } setObsBusy(false); }}>{obsBusy ? "Setting up…" : "Set up OBS"}</Btn>}>
        <p className="text-footnote text-muted">
          Pick <b className="font-semibold text-ink-80">OBS Virtual Camera</b> in Teams, Zoom or the browser. MirrorMagic turns the PC camera on while an app uses it.
        </p>
        <Row label="OBS" hint={s.obs.detail || (s.obs.installed ? "Installed" : "Install OBS Studio from obsproject.com")}>
          <span className={`text-caption ${s.obs.virtualcam ? "text-ok" : "text-muted"}`}>{s.obs.virtualcam ? "Virtual camera on" : s.obs.running ? "Running" : s.obs.installed ? "Not running" : "Not installed"}</span>
        </Row>
        <Row label="Start automatically" hint={s.camera_detail || (s.camera_wanted ? "An app is using the camera" : "No app is using the camera")}>
          <Toggle label="Start the PC camera automatically" on={c.camera_auto} onChange={(v) => save({ camera_auto: v })} />
        </Row>
        <Row label="PC audio" hint={s.mic_detail || "Each PC microphone and speaker appears on the Mac as '<name> – PC': pick them in Teams, Zoom or the browser"}>
          <Btn onClick={api.openAudioPrivacy}>Open settings</Btn>
        </Row>
        <p className="text-footnote text-muted">
          The app using a '– PC' microphone (Teams, Zoom, the browser) must be added with + and switched on under <b className="font-semibold text-ink-80">System Audio Recording Only</b>, then restarted. Otherwise macOS gives it silence, while the '– PC' speakers still work.
        </p>
        <Row label="Stream now" hint="Keep the PC camera on regardless">
          <ManualCamera />
        </Row>
      </Card>

      <Shared {...p} />
    </>
  );
}

function ManualCamera() {
  const [on, setOn] = useState(false);
  return <Toggle label="Stream the PC camera now" on={on} onChange={(v) => { setOn(v); api.cameraNow(v); }} />;
}
