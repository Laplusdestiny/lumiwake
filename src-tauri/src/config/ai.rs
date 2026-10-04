//! AI 振り分け候補の設定（`[ai]` セクション）。
//!
//! API キーはここに持たない。環境変数名（`api_key_env`）だけを書き、値は実行時に環境変数から読む。

use serde::{Deserialize, Serialize};

/// 候補提示のバックエンド
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiBackend {
    /// CLIP 系モデルを端末上で実行する（既定）
    #[default]
    Local,
    /// System One 互換 API（画像を縮小して外部へ送る）
    Systemone,
    /// AI 機能を使わない
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LocalStrategy {
    Zeroshot,
    Knn,
    #[default]
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiLocal {
    pub model: String,
    pub strategy: LocalStrategy,
    /// この一致度以上は候補カードを強調表示する
    pub high: f32,
    /// この一致度未満は候補に出さない
    pub low: f32,
}

impl Default for AiLocal {
    fn default() -> Self {
        AiLocal { model: "clip-vit-b32".into(), strategy: LocalStrategy::Hybrid, high: 0.80, low: 0.40 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSystemOne {
    pub endpoint: String,
    /// リクエストの `model`（`clef` / `clef-flash`）。空ならエンドポイントの末尾から判別する。
    /// エンドポイントのモデルと一致しないと API が 400 を返すため、通常は空のままでよい
    pub model: String,
    /// API キーを読む環境変数の名前（キー自体は設定ファイルに書かない）
    pub api_key_env: String,
    /// 送信前の縮小・再圧縮で収める画像サイズ（KB）
    pub max_image_kb: u32,
    pub high: f32,
    pub low: f32,
    /// 画像が外部へ送信されることへの同意。false の間は一切送らない
    pub external_consent: bool,
}

impl Default for AiSystemOne {
    fn default() -> Self {
        AiSystemOne {
            endpoint: "https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/clef-flash".into(),
            model: String::new(),
            api_key_env: "LUMIWAKE_SYSTEMONE_KEY".into(),
            max_image_kb: 190,
            high: 0.85,
            low: 0.50,
            external_consent: false,
        }
    }
}

pub const MAX_TOP_K: usize = 10;
pub const MAX_PREFETCH: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    pub backend: AiBackend,
    /// 表示する候補の数
    pub top_k: usize,
    /// 表示中の何枚先まで先に診断するか
    pub prefetch: usize,
    pub local: AiLocal,
    pub systemone: AiSystemOne,
}

impl Default for Ai {
    fn default() -> Self {
        Ai {
            backend: AiBackend::Local,
            top_k: 3,
            prefetch: 5,
            local: AiLocal::default(),
            systemone: AiSystemOne::default(),
        }
    }
}

impl Ai {
    /// 範囲外の値を許容範囲に収める
    pub fn normalize(&mut self) {
        self.top_k = self.top_k.clamp(1, MAX_TOP_K);
        self.prefetch = self.prefetch.min(MAX_PREFETCH);
        for t in [&mut self.local.high, &mut self.local.low, &mut self.systemone.high, &mut self.systemone.low] {
            *t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        }
        self.local.model = self.local.model.trim().to_string();
        self.systemone.endpoint = self.systemone.endpoint.trim().to_string();
        self.systemone.model = self.systemone.model.trim().to_string();
        self.systemone.api_key_env = self.systemone.api_key_env.trim().to_string();
        self.systemone.max_image_kb = self.systemone.max_image_kb.clamp(16, 4096);
    }

    /// 設定の問題点（画面に出す文言）。(エラーか, 文言)
    pub fn problems(&self) -> Vec<(bool, String)> {
        let mut v = Vec::new();
        if self.local.low > self.local.high {
            v.push((true, "ローカルのしきい値は「低」が「高」以下である必要があります".into()));
        }
        if self.systemone.low > self.systemone.high {
            v.push((true, "System One のしきい値は「低」が「高」以下である必要があります".into()));
        }
        if self.backend == AiBackend::Systemone {
            if !self.systemone.external_consent {
                v.push((false, "System One は、画像の外部送信への同意が済むまで使われません".into()));
            }
            if self.systemone.api_key_env.is_empty() {
                v.push((true, "System One の API キーを読む環境変数名が指定されていません".into()));
            }
            if self.systemone.endpoint.is_empty() {
                v.push((true, "System One のエンドポイントが指定されていません".into()));
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_requirements_example() {
        let a = Ai::default();
        assert_eq!(a.backend, AiBackend::Local, "既定は端末上で動くローカル");
        assert_eq!((a.top_k, a.prefetch), (3, 5));
        assert_eq!(a.local.strategy, LocalStrategy::Hybrid);
        assert_eq!((a.local.high, a.local.low), (0.80, 0.40));
        assert_eq!(a.systemone.api_key_env, "LUMIWAKE_SYSTEMONE_KEY");
        assert_eq!(a.systemone.max_image_kb, 190);
        assert_eq!((a.systemone.high, a.systemone.low), (0.85, 0.50));
        assert!(!a.systemone.external_consent, "同意は初期状態で未取得");
    }

    #[test]
    fn parses_the_toml_example_from_requirements() {
        let text = r#"
            backend = "systemone"
            top_k = 3
            prefetch = 5

            [local]
            model = "clip-vit-b32"
            strategy = "knn"
            high = 0.8
            low = 0.4

            [systemone]
            endpoint = "https://example.invalid/clef"
            api_key_env = "MY_KEY"
            max_image_kb = 190
            high = 0.85
            low = 0.5
            external_consent = true
        "#;
        let a: Ai = toml::from_str(text).unwrap();
        assert_eq!(a.backend, AiBackend::Systemone);
        assert_eq!(a.local.strategy, LocalStrategy::Knn);
        assert_eq!(a.systemone.api_key_env, "MY_KEY");
        assert!(a.systemone.external_consent);
    }

    #[test]
    fn partial_and_unknown_values() {
        let a: Ai = toml::from_str("top_k = 7").unwrap();
        assert_eq!(a.top_k, 7);
        assert_eq!(a.backend, AiBackend::Local, "書いていない項目は既定値");
        assert!(toml::from_str::<Ai>("backend = \"cloud\"").is_err());
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut a = Ai { top_k: 0, prefetch: 99, ..Ai::default() };
        a.local.high = 1.5;
        a.local.low = f32::NAN;
        a.systemone.max_image_kb = 0;
        a.systemone.api_key_env = "  KEY  ".into();
        a.systemone.model = " clef ".into();
        a.normalize();
        assert_eq!(a.systemone.model, "clef");
        assert_eq!(a.top_k, 1);
        assert_eq!(a.prefetch, MAX_PREFETCH);
        assert_eq!((a.local.high, a.local.low), (1.0, 0.0));
        assert_eq!(a.systemone.max_image_kb, 16);
        assert_eq!(a.systemone.api_key_env, "KEY");
    }

    #[test]
    fn problems_flag_bad_thresholds_and_missing_consent() {
        let mut a = Ai::default();
        assert!(a.problems().is_empty(), "既定（local）に問題はない");

        a.local.low = 0.9;
        assert!(a.problems().iter().any(|(err, m)| *err && m.contains("ローカル")));

        let mut a = Ai { backend: AiBackend::Systemone, ..Ai::default() };
        let p = a.problems();
        assert!(p.iter().any(|(err, m)| !*err && m.contains("同意")), "同意前は警告");
        assert!(!p.iter().any(|(err, _)| *err));

        a.systemone.api_key_env.clear();
        assert!(a.problems().iter().any(|(err, m)| *err && m.contains("環境変数")));
    }

    #[test]
    fn serialized_form_never_contains_a_secret_field() {
        let text = toml::to_string_pretty(&Ai::default()).unwrap();
        assert!(text.contains("api_key_env"));
        assert!(!text.contains("api_key ="), "キー本体を書くフィールドは持たない");
    }
}
