import { describe, expect, it } from "vitest";
import { codeToKey, displayCombo, eventToCombo, isBrowserKey } from "./keys";

const ev = (code: string, mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean }> = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe("codeToKey", () => {
  it("英字・数字・テンキー・F キー", () => {
    expect(codeToKey("KeyA")).toBe("A");
    expect(codeToKey("Digit1")).toBe("1");
    expect(codeToKey("Numpad3")).toBe("Num3");
    expect(codeToKey("NumpadAdd")).toBe("NumAdd");
    expect(codeToKey("F12")).toBe("F12");
    expect(codeToKey("ArrowLeft")).toBe("Left");
    expect(codeToKey("Space")).toBe("Space");
  });

  it("修飾キー単体や未知のキーは null", () => {
    expect(codeToKey("ShiftLeft")).toBeNull();
    expect(codeToKey("ControlRight")).toBeNull();
    expect(codeToKey("MetaLeft")).toBeNull();
    expect(codeToKey("F25")).toBeNull();
    expect(codeToKey("")).toBeNull();
  });
});

describe("eventToCombo", () => {
  it("修飾キーを Ctrl → Shift → Alt の順に並べる（Rust 側の正規形と同じ）", () => {
    expect(eventToCombo(ev("Digit1"))).toBe("1");
    expect(eventToCombo(ev("Digit1", { shift: true, ctrl: true }))).toBe("Ctrl+Shift+1");
    expect(eventToCombo(ev("KeyZ", { ctrl: true }))).toBe("Ctrl+Z");
    expect(eventToCombo(ev("F2", { alt: true, shift: true }))).toBe("Shift+Alt+F2");
  });

  it("Shift で記号になるキーも物理キーで判定する", () => {
    // Shift+1 は key が "!" になるが code は Digit1
    expect(eventToCombo(ev("Digit1", { shift: true }))).toBe("Shift+1");
  });
});

describe("表示", () => {
  it("矢印は記号で表示", () => {
    expect(displayCombo("Left")).toBe("←");
    expect(displayCombo("Ctrl+Right")).toBe("Ctrl+→");
  });

  it("WebView の既定キー", () => {
    expect(isBrowserKey("F5")).toBe(true);
    expect(isBrowserKey("Ctrl+R")).toBe(true);
    expect(isBrowserKey("1")).toBe(false);
  });
});
