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
