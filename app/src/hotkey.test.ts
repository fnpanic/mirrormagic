import { describe, expect, it } from "vitest";
import { hotkeyFromEvent, isUsableHotkey } from "./hotkey";

const ev = (code: string, key: string, mods: Partial<{ ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }> = {}) => ({
  code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods,
});

describe("hotkeyFromEvent", () => {
  it("formats modifier combos like the Rust parser expects", () => {
    expect(hotkeyFromEvent(ev("KeyM", "m", { ctrlKey: true, altKey: true }))).toBe("Ctrl+Alt+M");
    expect(hotkeyFromEvent(ev("Digit2", "2", { altKey: true }))).toBe("Alt+2");
    expect(hotkeyFromEvent(ev("F12", "F12", { shiftKey: true, metaKey: true }))).toBe("Shift+Win+F12");
    expect(hotkeyFromEvent(ev("ScrollLock", "ScrollLock"))).toBe("ScrollLock");
  });
  it("waits while only modifiers are down", () => {
    expect(hotkeyFromEvent(ev("ControlLeft", "Control", { ctrlKey: true }))).toBeNull();
    expect(hotkeyFromEvent(ev("AltRight", "AltGraph"))).toBeNull();
  });
});

describe("isUsableHotkey", () => {
  it("rejects bare letters, accepts combos and F-keys", () => {
    expect(isUsableHotkey("M")).toBe(false);
    expect(isUsableHotkey("Ctrl+Alt+M")).toBe(true);
    expect(isUsableHotkey("F13")).toBe(true);
    expect(isUsableHotkey("Shift+F12")).toBe(true);
    expect(isUsableHotkey("Shift+M")).toBe(false); // a capital M while typing
  });
});

describe("AltGr layouts", () => {
  it("records Ctrl+Alt+M on a German keyboard, where the browser reports AltGraph (µ)", () => {
    const altGrM = { code: "KeyM", key: "µ", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, getModifierState: (k: string) => k === "AltGraph" };
    expect(hotkeyFromEvent(altGrM)).toBe("Ctrl+Alt+M");
    expect(isUsableHotkey(hotkeyFromEvent(altGrM)!)).toBe(true);
    // with both flags set (other browsers) it must not double up
    expect(hotkeyFromEvent({ ...altGrM, ctrlKey: true, altKey: true })).toBe("Ctrl+Alt+M");
  });
});
