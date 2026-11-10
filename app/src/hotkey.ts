export const DEFAULT_HOTKEY = "Ctrl+Alt+M";

type KeyLike = {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  getModifierState?: (key: string) => boolean;
};

// Turns a keydown into the hotkey string the Rust side parses ("Ctrl+Alt+M").
// Returns null while only modifiers are held.
export function hotkeyFromEvent(e: KeyLike): string | null {
  if (["Control", "Alt", "Shift", "Meta", "AltGraph", "OS"].includes(e.key)) return null;
  // On German (and other European) layouts Windows treats Ctrl+Alt as AltGr. For keys that
  // produce an AltGr character (M → µ, Q → @, 7 → {, …) the browser reports AltGraph instead of
  // Ctrl+Alt, so count AltGraph as Ctrl+Alt: that is the combination the user pressed.
  const altGr = e.getModifierState?.("AltGraph") ?? false;
  const parts: string[] = [];
  if (e.ctrlKey || altGr) parts.push("Ctrl");
  if (e.altKey || altGr) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Win");
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  parts.push(key);
  return parts.join("+");
}

// A hotkey needs Ctrl, Alt or Win unless it is a function key, so normal typing never triggers
// it (Shift+A is a capital A). Same rule as Hotkey::is_usable on the Rust side.
export function isUsableHotkey(h: string): boolean {
  const parts = h.split("+");
  const key = parts[parts.length - 1];
  return parts.slice(0, -1).some((m) => m !== "Shift") || /^F([1-9]|1[0-9]|20)$/.test(key);
}
