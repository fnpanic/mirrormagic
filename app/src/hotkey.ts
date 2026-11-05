// Turns a keydown into the hotkey string the Rust side parses ("Ctrl+Alt+M").
// Returns null while only modifiers are held.
export function hotkeyFromEvent(e: { key: string; code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }): string | null {
  if (["Control", "Alt", "Shift", "Meta", "AltGraph", "OS"].includes(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Win");
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  parts.push(key);
  return parts.join("+");
}

// A hotkey needs a modifier unless it is a function key, so normal typing never triggers it.
export function isUsableHotkey(h: string): boolean {
  const parts = h.split("+");
  const key = parts[parts.length - 1];
  return parts.length > 1 || /^F([1-9]|1[0-9]|2[0-4])$/.test(key);
}
