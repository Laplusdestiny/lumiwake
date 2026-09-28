// キー入力を Rust 側（src-tauri/src/config/keys.rs）と同じ正規形の文字列にする。
// 正規形: 修飾キーを Ctrl → Shift → Alt の順に並べ、最後にキー名（例: "Ctrl+Shift+1"）。
// Shift を押すと KeyboardEvent.key は記号に変わるため、物理キー（code）で判定する。

const NAMED = new Set([
  "Space",
  "Enter",
  "Tab",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "Escape",
  "Minus",
  "Equal",
  "BracketLeft",
  "BracketRight",
  "Backslash",
  "Semicolon",
  "Quote",
  "Backquote",
  "Comma",
  "Period",
  "Slash",
  "IntlRo",
  "IntlYen",
]);

const ALIASES: Record<string, string> = {
  ArrowLeft: "Left",
  ArrowRight: "Right",
  ArrowUp: "Up",
  ArrowDown: "Down",
  NumpadAdd: "NumAdd",
  NumpadSubtract: "NumSubtract",
  NumpadMultiply: "NumMultiply",
  NumpadDivide: "NumDivide",
  NumpadDecimal: "NumDecimal",
  NumpadEnter: "NumEnter",
};

/** KeyboardEvent.code をキー名にする。修飾キー単体や扱わないキーは null */
export function codeToKey(code: string): string | null {
  let m: RegExpMatchArray | null;
  if ((m = code.match(/^Key([A-Z])$/))) return m[1];
  if ((m = code.match(/^Digit([0-9])$/))) return m[1];
  if ((m = code.match(/^Numpad([0-9])$/))) return `Num${m[1]}`;
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (code in ALIASES) return ALIASES[code];
  if (NAMED.has(code)) return code;
  return null;
}

export interface KeyLike {
  code: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

export function eventToCombo(e: KeyLike): string | null {
  const key = codeToKey(e.code);
  if (!key) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");
  parts.push(key);
  return parts.join("+");
}

const PRETTY: Record<string, string> = { Left: "←", Right: "→", Up: "↑", Down: "↓", Escape: "Esc" };

/** 画面に出すための短い表記 */
export function displayCombo(combo: string): string {
  return combo
    .split("+")
    .map((p) => PRETTY[p] ?? p)
    .join("+");
}

/**
 * WebView が既定の動作（再読み込み・印刷・検索・戻る など）に使うキー。
 * 仕分け中に押しても画面が再読み込みされたりしないよう、既定動作を止める。
 */
const BROWSER_KEYS = new Set([
  "F5",
  "Ctrl+R",
  "Ctrl+Shift+R",
  "Ctrl+F5",
  "Ctrl+P",
  "Ctrl+F",
  "F3",
  "Ctrl+G",
  "Ctrl+Shift+G",
  "Ctrl+U",
  "F7",
  "Alt+Left",
  "Alt+Right",
  "Alt+Home",
  "Ctrl+W",
  "Ctrl+N",
  "Ctrl+Shift+N",
  "Ctrl+T",
  "Ctrl+Equal",
  "Ctrl+Minus",
  "Ctrl+0",
  "Ctrl+S",
  "Ctrl+O",
  "Ctrl+A",
]);

export function isBrowserKey(combo: string): boolean {
  return BROWSER_KEYS.has(combo);
}
