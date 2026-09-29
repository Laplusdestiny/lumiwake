//! キーの組み合わせ（Ctrl/Shift/Alt + キー）の表記と正規化。
//!
//! 正規形は `Ctrl+Shift+Alt+キー名` の順で、キー名はフロントエンドが
//! `KeyboardEvent.code` から作る名前と一致させる（`src/keys.ts`）。
//! Shift を押すと `KeyboardEvent.key` は記号に変わるため、物理キー（code）で判定する。

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        f.write_str(&self.key)
    }
}

/// 名前付きキー（英字・数字・F キー以外）
const NAMED_KEYS: &[&str] = &[
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
    "Left",
    "Right",
    "Up",
    "Down",
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
    "NumAdd",
    "NumSubtract",
    "NumMultiply",
    "NumDivide",
    "NumDecimal",
    "NumEnter",
];

/// 表記ゆれを吸収してキー名を正規化する
fn normalize_key(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let alias = match lower.as_str() {
        "esc" => Some("Escape"),
        "del" => Some("Delete"),
        "ins" => Some("Insert"),
        "return" => Some("Enter"),
        "spacebar" => Some("Space"),
        "arrowleft" => Some("Left"),
        "arrowright" => Some("Right"),
        "arrowup" => Some("Up"),
        "arrowdown" => Some("Down"),
        "pgup" => Some("PageUp"),
        "pgdn" | "pgdown" => Some("PageDown"),
        _ => None,
    };
    if let Some(a) = alias {
        return Some(a.to_string());
    }
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() == 1 && chars[0].is_ascii_alphanumeric() {
        return Some(chars[0].to_ascii_uppercase().to_string());
    }
    // テンキー: Num1 / Numpad1
    for prefix in ["numpad", "num"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("Num{rest}"));
            }
            let named = format!("Num{rest}");
            if let Some(k) = NAMED_KEYS.iter().find(|k| k.eq_ignore_ascii_case(&named)) {
                return Some(k.to_string());
            }
        }
    }
    // F1〜F24
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) {
            return Some(format!("F{n}"));
        }
    }
    NAMED_KEYS.iter().find(|k| k.eq_ignore_ascii_case(raw)).map(|k| k.to_string())
}

impl KeyCombo {
    /// `"Ctrl+Shift+1"` のような表記を解釈する（大文字小文字・修飾キーの順序は問わない）
    pub fn parse(s: &str) -> Option<KeyCombo> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let mut combo = KeyCombo { ctrl: false, shift: false, alt: false, key: String::new() };
        for part in s.split('+').map(str::trim) {
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "ctl" => &mut combo.ctrl,
                "shift" => &mut combo.shift,
                "alt" | "option" => &mut combo.alt,
                _ => {
                    // 修飾キー以外はちょうど 1 つ
                    if !combo.key.is_empty() {
                        return None;
                    }
                    combo.key = normalize_key(part)?;
                    continue;
                }
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        (!combo.key.is_empty()).then_some(combo)
    }
}

/// WebView・OS が先に処理する、または処理する可能性が高いキー。
/// 割り当てても効かないことがあるため、設定時に警告する。
pub fn reserved_reason(combo: &KeyCombo) -> Option<&'static str> {
    let s = combo.to_string();
    Some(match s.as_str() {
        "F5" | "Ctrl+R" | "Ctrl+Shift+R" | "Ctrl+F5" => "WebView の再読み込み",
        "F12" | "Ctrl+Shift+I" | "Ctrl+Shift+J" | "Ctrl+Shift+C" => "WebView の開発者ツール",
        "Ctrl+P" => "WebView の印刷",
        "Ctrl+F" | "F3" | "Ctrl+G" | "Ctrl+Shift+G" => "WebView の検索",
        "Ctrl+U" => "WebView のソース表示",
        "F7" => "WebView のキャレットブラウズ",
        "Alt+Left" | "Alt+Right" | "Alt+Home" => "WebView の戻る／進む",
        "Alt+F4" => "OS のウィンドウを閉じる",
        "Ctrl+W" | "Ctrl+N" | "Ctrl+Shift+N" | "Ctrl+T" => "ブラウザ操作（環境により効かない）",
        "Tab" | "Shift+Tab" => "フォーカス移動",
        "Escape" => "Lumiwake がダイアログを閉じる・全画面を抜けるのに使用",
        "Ctrl+Comma" => "Lumiwake が設定画面を開くのに使用",
        "Ctrl+Q" => "Lumiwake の終了に使用",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> Option<String> {
        KeyCombo::parse(s).map(|c| c.to_string())
    }

    #[test]
    fn normalizes_modifier_order_and_case() {
        assert_eq!(norm("shift+ctrl+1").as_deref(), Some("Ctrl+Shift+1"));
        assert_eq!(norm("Alt+a").as_deref(), Some("Alt+A"));
        assert_eq!(norm("control + Shift + Alt + f2").as_deref(), Some("Ctrl+Shift+Alt+F2"));
        assert_eq!(norm("space").as_deref(), Some("Space"));
        assert_eq!(norm("Del").as_deref(), Some("Delete"));
        assert_eq!(norm("ArrowLeft").as_deref(), Some("Left"));
        assert_eq!(norm("Numpad3").as_deref(), Some("Num3"));
        assert_eq!(norm("numadd").as_deref(), Some("NumAdd"));
        assert_eq!(norm("z+ctrl").as_deref(), Some("Ctrl+Z"), "修飾キーの位置は問わない");
    }

    #[test]
    fn rejects_invalid_combos() {
        assert_eq!(norm(""), None);
        assert_eq!(norm("Ctrl+"), None);
        assert_eq!(norm("Hyper+1"), None);
        assert_eq!(norm("Ctrl+Ctrl+1"), None);
        assert_eq!(norm("1+2"), None);
        assert_eq!(norm("Shift"), None);
        assert_eq!(norm("F25"), None);
        assert_eq!(norm("あ"), None);
        assert_eq!(norm("12"), None);
    }

    #[test]
    fn reports_reserved_keys() {
        assert!(reserved_reason(&KeyCombo::parse("f5").unwrap()).is_some());
        assert!(reserved_reason(&KeyCombo::parse("ctrl+r").unwrap()).is_some());
        assert!(reserved_reason(&KeyCombo::parse("alt+left").unwrap()).is_some());
        assert!(reserved_reason(&KeyCombo::parse("Ctrl+1").unwrap()).is_none());
    }
}
