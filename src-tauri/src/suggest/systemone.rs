//! System One 互換 API（Cloudflare Workers AI の Clef / Clef-flash など）への問い合わせ。
//!
//! 仕様は Cloudflare の公式モデルページと、その入出力スキーマ（schema-input.json / schema-output.json）に従う。
//! - リクエスト: `model`・`state`・`questions` が必須、画像は `images`（PNG/JPEG/WebP の埋め込みのみ。リモート URL は不可）
//! - 1 画像につき choice 型の質問 1 問。選択肢は「現在の振り分け先＋該当なし」
//! - レスポンス: `answers.<質問 ID> = { type: "choice", choice, probabilities, confidence }`
//!
//! 選択肢の ID にはフォルダ名やパスを使わず、`f0`・`f1`…と `none` を使う（API に渡す文字列を最小にし、
//! 日本語や記号による制約も避ける）。対応表は呼び出しごとに作り、レスポンスの解釈に使う。

use super::{Choice, ScoreKind, Scored, Suggestion};
use crate::config::ai::AiSystemOne;
use base64::Engine;
use serde_json::{json, Value};

/// 「該当なし」の選択肢 ID
pub const NONE_ID: &str = "none";
/// 質問の ID（英数字・`_`・`.`・`-`、100 文字以内）
const QUESTION_ID: &str = "dest";
/// choice 型の選択肢は 2〜255 個。「該当なし」を含めて収める
const MAX_CHOICES: usize = 254;

/// エンドポイントの末尾（`.../clef-flash`）からモデル名を判別する。`clef` / `clef-flash` のみ
pub fn model_from_endpoint(endpoint: &str) -> Option<String> {
    let path = endpoint.split(['?', '#']).next()?.trim_end_matches('/');
    let last = path.rsplit('/').next()?;
    matches!(last, "clef" | "clef-flash").then(|| last.to_string())
}

/// 送るモデル名。エンドポイントのモデルと食い違うと API が 400 を返すので、明示されていても検証する
pub fn resolve_model(cfg: &AiSystemOne) -> Result<String, String> {
    let from_endpoint = model_from_endpoint(&cfg.endpoint);
    match (cfg.model.as_str(), from_endpoint) {
        ("", Some(m)) => Ok(m),
        ("", None) => Err("エンドポイントの末尾が clef / clef-flash ではないため、モデルを判別できません。設定の model を指定してください".into()),
        (m, _) if !matches!(m, "clef" | "clef-flash") => Err(format!("model は clef か clef-flash を指定してください（{m}）")),
        (m, Some(e)) if m != e => Err(format!("model（{m}）とエンドポイントのモデル（{e}）が一致しません")),
        (m, _) => Ok(m.to_string()),
    }
}

/// 送信先として使えるか。置き換え忘れの `{account}` や、暗号化されない通信先には送らない
pub fn check_endpoint(endpoint: &str) -> Result<(), String> {
    if endpoint.contains('{') || endpoint.contains('}') {
        return Err("エンドポイントの {account} を Cloudflare のアカウント ID に置き換えてください".into());
    }
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| endpoint.starts_with(p));
    if !endpoint.starts_with("https://") && !local {
        return Err("エンドポイントは https:// で始まる必要があります".into());
    }
    Ok(())
}

/// 送るリクエストと、選択肢 ID → 振り分け先の対応表（`None` は「該当なし」）
pub struct Request {
    pub body: Value,
    pub options: Vec<(String, Option<String>)>,
}

pub fn build_request(model: &str, choices: &[Choice], jpeg: &[u8]) -> Result<Request, String> {
    if choices.is_empty() {
        return Err("診断する振り分け先がありません".into());
    }
    if choices.len() > MAX_CHOICES {
        return Err(format!("振り分け先が多すぎます（{} 件まで）", MAX_CHOICES));
    }
    let mut criteria = serde_json::Map::new();
    let mut options = Vec::new();
    for (i, c) in choices.iter().enumerate() {
        let id = format!("f{i}");
        let text = if c.description.trim().is_empty() {
            c.label.clone()
        } else {
            format!("{}：{}", c.label, c.description.trim())
        };
        criteria.insert(id.clone(), Value::String(text));
        options.push((id, Some(c.id.clone())));
    }
    criteria.insert(NONE_ID.into(), Value::String("どの振り分け先にも当てはまらない".into()));
    options.push((NONE_ID.into(), None));

    let body = json!({
        "model": model,
        "state": "写真の整理。画像 1 枚を、用意されたフォルダのどれに入れるかを決める。",
        "questions": {
            QUESTION_ID: {
                "type": "choice",
                "instructions": "添付した画像は、どのフォルダに入れるのが最もふさわしいですか。どれにも当てはまらなければ none を選んでください。",
                "criteria": criteria,
            }
        },
        "images": [{
            "content_type": "image/jpeg",
            "base64": base64::engine::general_purpose::STANDARD.encode(jpeg),
        }],
    });
    Ok(Request { body, options })
}

/// レスポンスの本文から診断結果を作る。形が仕様と違うときはエラー（推測で補わない）
pub fn parse_response(body: &[u8], options: &[(String, Option<String>)]) -> Result<Suggestion, String> {
    let v: Value = serde_json::from_slice(body).map_err(|_| "応答を JSON として解釈できません".to_string())?;
    let answer = v
        .get("answers")
        .and_then(|a| a.get(QUESTION_ID))
        .ok_or_else(|| "応答に回答（answers）が含まれていません".to_string())?;
    if answer.get("type").and_then(Value::as_str) != Some("choice") {
        return Err("応答の回答が choice 型ではありません".into());
    }
    let probs = answer
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or_else(|| "応答に確率（probabilities）が含まれていません".to_string())?;
    let prob_of = |id: &str| -> Result<f32, String> {
        match probs.get(id) {
            None => Ok(0.0),
            Some(p) => p
                .as_f64()
                .filter(|p| (0.0..=1.0).contains(p))
                .map(|p| p as f32)
                .ok_or_else(|| "応答の確率が 0〜1 の数値ではありません".to_string()),
        }
    };
    let mut scores = Vec::new();
    let mut none_of_above = 0.0;
    for (opt, target) in options {
        let p = prob_of(opt)?;
        match target {
            Some(id) => scores.push(Scored { id: id.clone(), score: p }),
            None => none_of_above = p,
        }
    }
    Ok(Suggestion { kind: ScoreKind::Probability, scores, none_of_above: Some(none_of_above) })
}

/// エラー応答から表示用の短い理由を取り出す（Cloudflare の `errors[0].message`。無ければ None）
pub fn error_message(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let m = v.get("errors")?.get(0)?.get("message")?.as_str()?;
    Some(m.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, label: &str, desc: &str) -> Choice {
        Choice { id: id.into(), label: label.into(), description: desc.into() }
    }

    fn cfg(endpoint: &str, model: &str) -> AiSystemOne {
        AiSystemOne { endpoint: endpoint.into(), model: model.into(), ..AiSystemOne::default() }
    }

    const EP: &str = "https://api.cloudflare.com/client/v4/accounts/abc123/ai/run/@cf/cloudflare/clef-flash";

    #[test]
    fn model_is_taken_from_the_endpoint_and_must_agree_with_an_explicit_one() {
        assert_eq!(resolve_model(&cfg(EP, "")).unwrap(), "clef-flash");
        assert_eq!(resolve_model(&cfg(&EP.replace("clef-flash", "clef"), "")).unwrap(), "clef");
        assert_eq!(resolve_model(&cfg(&format!("{EP}/"), "clef-flash")).unwrap(), "clef-flash");
        assert!(resolve_model(&cfg(EP, "clef")).unwrap_err().contains("一致しません"));
        assert!(resolve_model(&cfg(EP, "gpt")).is_err());
        assert!(resolve_model(&cfg("https://example.com/v1/run", "")).unwrap_err().contains("判別できません"));
        assert_eq!(
            resolve_model(&cfg("https://example.com/v1/run", "clef")).unwrap(),
            "clef",
            "自前ホストは model を明示"
        );
    }

    #[test]
    fn endpoint_must_be_resolved_and_encrypted() {
        assert!(check_endpoint(EP).is_ok());
        let tpl = AiSystemOne::default().endpoint;
        assert!(check_endpoint(&tpl).unwrap_err().contains("{account}"), "置き換え忘れでは送らない");
        assert!(check_endpoint("http://example.com/clef").is_err());
        assert!(check_endpoint("http://localhost:8080/clef").is_ok(), "自前ホストのローカル確認は許す");
        assert!(check_endpoint("").is_err());
    }

    #[test]
    fn request_has_the_documented_shape() {
        let choices = [choice("/p/a", "風景", "山や海"), choice("/p/b", "人物", "")];
        let r = build_request("clef-flash", &choices, b"\xff\xd8jpeg").unwrap();
        let b = &r.body;
        assert_eq!(b["model"], "clef-flash");
        assert!(b["state"].as_str().is_some_and(|s| !s.is_empty()));
        let q = &b["questions"]["dest"];
        assert_eq!(q["type"], "choice");
        assert!(q["instructions"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(q["criteria"]["f0"], "風景：山や海", "説明文があればラベルと合わせて渡す");
        assert_eq!(q["criteria"]["f1"], "人物");
        assert!(q["criteria"]["none"].is_string(), "該当なしの選択肢がある");
        assert_eq!(q["criteria"].as_object().unwrap().len(), 3);
        assert_eq!(b["images"].as_array().unwrap().len(), 1, "1 画像 1 リクエスト");
        assert_eq!(b["images"][0]["content_type"], "image/jpeg");
        let decoded = base64::engine::general_purpose::STANDARD.decode(b["images"][0]["base64"].as_str().unwrap());
        assert_eq!(decoded.unwrap(), b"\xff\xd8jpeg");
    }

    #[test]
    fn folder_paths_are_never_sent() {
        let choices = [choice("/home/secret/家族旅行", "旅行", "")];
        let body = serde_json::to_string(&build_request("clef", &choices, b"x").unwrap().body).unwrap();
        assert!(!body.contains("/home/secret") && !body.contains("家族旅行"), "パスは API に渡さない");
    }

    #[test]
    fn choice_count_is_validated() {
        assert!(build_request("clef", &[], b"x").is_err());
        let many: Vec<_> = (0..255).map(|i| choice(&format!("/p/{i}"), "x", "")).collect();
        assert!(build_request("clef", &many, b"x").is_err());
        let max: Vec<_> = (0..254).map(|i| choice(&format!("/p/{i}"), "x", "")).collect();
        assert!(build_request("clef", &max, b"x").is_ok(), "該当なしを含めて 255 個");
    }

    fn response(probs: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "model": "clef-flash",
            "answers": { "dest": { "type": "choice", "choice": "f0", "probabilities": probs, "confidence": 0.9 } },
            "usage": { "input_tokens": 300, "output_tokens": 4 }
        }))
        .unwrap()
    }

    #[test]
    fn response_probabilities_map_back_to_destinations() {
        let choices = [choice("/p/a", "a", ""), choice("/p/b", "b", "")];
        let req = build_request("clef-flash", &choices, b"x").unwrap();
        let s = parse_response(&response(json!({"f0": 0.7, "f1": 0.2, "none": 0.1})), &req.options).unwrap();
        assert_eq!(s.kind, ScoreKind::Probability);
        assert_eq!(s.scores, vec![Scored { id: "/p/a".into(), score: 0.7 }, Scored { id: "/p/b".into(), score: 0.2 }]);
        assert!((s.none_of_above.unwrap() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn missing_options_count_as_zero_and_unknown_ones_are_ignored() {
        let req = build_request("clef", &[choice("/p/a", "a", ""), choice("/p/b", "b", "")], b"x").unwrap();
        let s = parse_response(&response(json!({"f0": 1.0, "zzz": 0.5})), &req.options).unwrap();
        assert_eq!(s.scores[1].score, 0.0);
        assert_eq!(s.none_of_above, Some(0.0));
    }

    #[test]
    fn malformed_responses_are_errors_not_guesses() {
        let req = build_request("clef", &[choice("/p/a", "a", "")], b"x").unwrap();
        let o = &req.options;
        assert!(parse_response(b"not json", o).is_err());
        assert!(parse_response(br#"{"answers":{}}"#, o).is_err());
        assert!(parse_response(br#"{"answers":{"dest":{"type":"score","score":1}}}"#, o).is_err());
        assert!(parse_response(br#"{"answers":{"dest":{"type":"choice"}}}"#, o).is_err());
        assert!(parse_response(&response(json!({"f0": 1.5})), o).is_err(), "範囲外の確率");
        assert!(parse_response(&response(json!({"f0": "high"})), o).is_err());
    }

    #[test]
    fn error_message_reads_cloudflare_style_errors() {
        let body = br#"{"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#;
        assert_eq!(error_message(body).as_deref(), Some("Authentication error"));
        assert_eq!(error_message(b"<html>"), None);
        assert_eq!(error_message(br#"{"errors":[]}"#), None);
    }
}

// ---- 通信と Suggester ----

use super::image_prep::prepare_jpeg;
use super::{SuggestError, SuggestRequest, Suggester};
use crate::config::ai::AiBackend;
use std::sync::Arc;
use std::time::Duration;

/// 画像つきのリクエストは 13〜30 秒かかるという報告があるため、余裕をもたせる
const TIMEOUT: Duration = Duration::from_secs(90);

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// HTTP の送受信。テストでは差し替えて、実際の通信を行わない
pub trait Transport: Send + Sync {
    fn post_json(&self, url: &str, bearer: &str, body: &[u8]) -> Result<HttpResponse, String>;
}

pub struct UreqTransport;

impl Transport for UreqTransport {
    fn post_json(&self, url: &str, bearer: &str, body: &[u8]) -> Result<HttpResponse, String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            // 4xx/5xx も応答として受け取り、こちらで理由を読む
            .http_status_as_error(false)
            .build();
        let agent: ureq::Agent = config.into();
        let mut resp = agent
            .post(url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body })
    }
}

/// API キーを読む場所。設定ファイルには書かず、環境変数から読む
pub type KeySource = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub fn env_key_source() -> KeySource {
    Arc::new(|name| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
}

pub struct SystemOneSuggester {
    cfg: AiSystemOne,
    transport: Arc<dyn Transport>,
    key: KeySource,
}

impl SystemOneSuggester {
    pub fn new(cfg: AiSystemOne) -> Self {
        Self::with(cfg, Arc::new(UreqTransport), env_key_source())
    }

    pub fn with(cfg: AiSystemOne, transport: Arc<dyn Transport>, key: KeySource) -> Self {
        SystemOneSuggester { cfg, transport, key }
    }
}

/// HTTP ステータスから、画面に出す理由を作る（API キーや画像は含めない）
fn describe_failure(status: u16, body: &[u8]) -> String {
    let detail = error_message(body).map(|m| format!("（{m}）")).unwrap_or_default();
    match status {
        401 | 403 => format!("API キーが無効か、権限がありません{detail}"),
        400 | 422 => format!("リクエストが受け付けられませんでした{detail}"),
        404 => format!("エンドポイントが見つかりません。アカウント ID とモデル名を確認してください{detail}"),
        429 => format!("リクエストが多すぎます。しばらく待ってから再診断してください{detail}"),
        s if s >= 500 => format!("サーバー側のエラーです（{s}）{detail}"),
        s => format!("想定外の応答です（{s}）{detail}"),
    }
}

impl Suggester for SystemOneSuggester {
    fn backend(&self) -> AiBackend {
        AiBackend::Systemone
    }

    fn model(&self) -> String {
        resolve_model(&self.cfg).unwrap_or_default()
    }

    fn suggest(&self, req: &SuggestRequest) -> Result<Suggestion, SuggestError> {
        // 画像を読んだり送ったりする前に、送れない理由をすべて確かめる
        let model = resolve_model(&self.cfg).map_err(SuggestError::Unavailable)?;
        check_endpoint(&self.cfg.endpoint).map_err(SuggestError::Unavailable)?;
        let key = (self.key)(&self.cfg.api_key_env).ok_or_else(|| {
            SuggestError::Unavailable(format!("環境変数 {} に API キーが設定されていません", self.cfg.api_key_env))
        })?;

        let jpeg = prepare_jpeg(req.image, self.cfg.max_image_kb).map_err(SuggestError::Failed)?;
        let request = build_request(&model, req.choices, &jpeg).map_err(SuggestError::Failed)?;
        let body = serde_json::to_vec(&request.body).map_err(|e| SuggestError::Failed(e.to_string()))?;

        let resp = self
            .transport
            .post_json(&self.cfg.endpoint, &key, &body)
            .map_err(|e| SuggestError::Failed(format!("通信に失敗しました: {e}")))?;
        if resp.status != 200 {
            return Err(SuggestError::Failed(describe_failure(resp.status, &resp.body)));
        }
        parse_response(&resp.body, &request.options).map_err(SuggestError::Failed)
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use image::DynamicImage;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    /// 呼び出しを記録して、決めた応答を返す
    struct Mock {
        calls: Mutex<Vec<(String, String, Vec<u8>)>>,
        reply: Result<(u16, Vec<u8>), String>,
    }

    impl Mock {
        fn new(reply: Result<(u16, Vec<u8>), String>) -> Arc<Self> {
            Arc::new(Mock { calls: Mutex::new(Vec::new()), reply })
        }
        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl Transport for Mock {
        fn post_json(&self, url: &str, bearer: &str, body: &[u8]) -> Result<HttpResponse, String> {
            self.calls.lock().unwrap().push((url.into(), bearer.into(), body.to_vec()));
            self.reply.clone().map(|(status, body)| HttpResponse { status, body })
        }
    }

    const EP: &str = "https://api.cloudflare.com/client/v4/accounts/abc/ai/run/@cf/cloudflare/clef-flash";

    fn suggester(ep: &str, mock: &Arc<Mock>, key: Option<&'static str>) -> SystemOneSuggester {
        let cfg = AiSystemOne { endpoint: ep.into(), ..AiSystemOne::default() };
        SystemOneSuggester::with(cfg, mock.clone(), Arc::new(move |_| key.map(String::from)))
    }

    fn choices() -> Vec<Choice> {
        vec![
            Choice { id: "/p/a".into(), label: "a".into(), description: String::new() },
            Choice { id: "/p/b".into(), label: "b".into(), description: String::new() },
        ]
    }

    fn run(s: &SystemOneSuggester) -> Result<Suggestion, SuggestError> {
        let img = DynamicImage::new_rgb8(32, 32);
        s.suggest(&SuggestRequest { image_hash: "h", image: &img, choices: &choices() })
    }

    fn ok_body() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "model": "clef-flash",
            "answers": { "dest": { "type": "choice", "choice": "f1", "confidence": 0.8,
                "probabilities": { "f0": 0.1, "f1": 0.8, "none": 0.1 } } },
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
        .unwrap()
    }

    #[test]
    fn a_successful_call_sends_the_key_as_bearer_and_returns_probabilities() {
        let mock = Mock::new(Ok((200, ok_body())));
        let s = run(&suggester(EP, &mock, Some("secret-token"))).unwrap();
        assert_eq!(s.scores[1], Scored { id: "/p/b".into(), score: 0.8 });
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, EP);
        assert_eq!(calls[0].1, "secret-token");
        let body: Value = serde_json::from_slice(&calls[0].2).unwrap();
        assert_eq!(body["model"], "clef-flash");
        assert!(!String::from_utf8_lossy(&calls[0].2).contains("secret-token"), "キーは本文に入れない");
    }

    #[test]
    fn nothing_is_sent_when_it_cannot_be_sent() {
        let mock = Mock::new(Ok((200, ok_body())));
        // キー未設定
        let e = run(&suggester(EP, &mock, None)).unwrap_err();
        assert!(matches!(&e, SuggestError::Unavailable(m) if m.contains("LUMIWAKE_SYSTEMONE_KEY")), "{e}");
        // {account} の置き換え忘れ
        let tpl = AiSystemOne::default().endpoint;
        assert!(matches!(run(&suggester(&tpl, &mock, Some("k"))), Err(SuggestError::Unavailable(_))));
        // モデルを判別できないエンドポイント
        assert!(matches!(
            run(&suggester("https://example.com/x", &mock, Some("k"))),
            Err(SuggestError::Unavailable(_))
        ));
        // 暗号化されていない通信先
        assert!(matches!(
            run(&suggester("http://example.com/clef", &mock, Some("k"))),
            Err(SuggestError::Unavailable(_))
        ));
        assert_eq!(mock.count(), 0, "どの場合もリクエストは出ていない");
    }

    #[test]
    fn an_image_that_cannot_be_shrunk_enough_is_not_sent() {
        let mock = Mock::new(Ok((200, ok_body())));
        // どこまで縮小しても 1KB には収まらない、ノイズの多い画像
        let cfg = AiSystemOne { endpoint: EP.into(), max_image_kb: 1, ..AiSystemOne::default() };
        let s = SystemOneSuggester::with(cfg, mock.clone(), Arc::new(|_| Some("k".into())));
        let mut state = 7u32;
        let noise = image::RgbImage::from_fn(300, 300, |_, _| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            image::Rgb([(state >> 8) as u8, (state >> 16) as u8, (state >> 24) as u8])
        });
        let img = DynamicImage::ImageRgb8(noise);
        let e = s.suggest(&SuggestRequest { image_hash: "h", image: &img, choices: &choices() }).unwrap_err();
        assert!(matches!(e, SuggestError::Failed(_)));
        assert_eq!(mock.count(), 0, "上限を超える画像は送らない");
    }

    #[test]
    fn http_failures_are_explained_without_leaking_the_key() {
        let cases = [
            (401, "API キー"),
            (403, "API キー"),
            (400, "受け付けられません"),
            (404, "エンドポイント"),
            (429, "多すぎます"),
            (503, "サーバー"),
        ];
        for (status, expect) in cases {
            let body = serde_json::to_vec(&json!({"errors":[{"message":"Authentication error"}]})).unwrap();
            let mock = Mock::new(Ok((status, body)));
            let e = run(&suggester(EP, &mock, Some("secret-token"))).unwrap_err().to_string();
            assert!(e.contains(expect), "{status}: {e}");
            assert!(!e.contains("secret-token"), "{status}: {e}");
        }
    }

    #[test]
    fn network_errors_and_bad_bodies_are_failures_not_panics() {
        let mock = Mock::new(Err("connection refused".into()));
        assert!(
            matches!(run(&suggester(EP, &mock, Some("k"))), Err(SuggestError::Failed(m)) if m.contains("通信に失敗"))
        );
        let mock = Mock::new(Ok((200, b"<html>".to_vec())));
        assert!(matches!(run(&suggester(EP, &mock, Some("k"))), Err(SuggestError::Failed(_))));
    }

    #[test]
    fn ureq_transport_posts_json_with_bearer_to_a_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let mut got = Vec::new();
            // ヘッダーと本文（Content-Length 分）を読み切る
            loop {
                let n = sock.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(h) = text.find("\r\n\r\n") {
                    let len = text[..h]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if got.len() >= h + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = br#"{"ok":true}"#;
            write!(sock, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.len()).unwrap();
            sock.write_all(reply).unwrap();
            String::from_utf8_lossy(&got).to_string()
        });
        let resp = UreqTransport.post_json(&format!("http://127.0.0.1:{port}/run"), "tok", br#"{"a":1}"#).unwrap();
        assert_eq!(resp.status, 429, "4xx も応答として受け取る");
        assert_eq!(resp.body, br#"{"ok":true}"#);
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /run "), "{request}");
        assert!(request.to_ascii_lowercase().contains("authorization: bearer tok"));
        assert!(request.to_ascii_lowercase().contains("content-type: application/json"));
        assert!(request.ends_with(r#"{"a":1}"#));
    }
}
