//! 画面に渡す候補の見え方。バックエンドの違いはここで吸収し、画面は `kind` と `state` だけを見ればよい。

use super::choices::{build_choices, target_id};
use super::service::Diagnosis;
use super::{rank, Level, ScoreKind, SuggestError};
use crate::config::ai::{Ai, AiBackend};
use crate::config::Target;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    /// 設定の `targets` の何番目か（キーでの振り分けはこの番号で行う）
    pub target: usize,
    pub key: String,
    pub name: String,
    pub path: String,
    pub score: f32,
    pub level: Level,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ViewState {
    /// AI 候補を使わない（帯もカードも出さない）
    Off,
    /// 使えない理由つき（未同意・未導入など）
    Unavailable {
        message: String,
    },
    /// 診断に失敗した（仕分けは続けられる）
    Failed {
        message: String,
    },
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionView {
    pub backend: AiBackend,
    #[serde(flatten)]
    pub state: ViewState,
    /// 確率か一致度か。`Match` は較正された確率ではないので「一致度」と表記する
    pub kind: Option<ScoreKind>,
    /// 画像を外部へ送っているか（ヘッダーの「外部送信中」表示）
    pub sends_images: bool,
    pub cards: Vec<CardView>,
    /// 「該当なし」の確率（Space＝スキップに対応するカード）。確率を返すバックエンドのみ
    pub none_of_above: Option<f32>,
    /// 診断のあとに追加され、まだ評価されていない振り分け先の名前（再診断の案内に使う）
    pub unevaluated: Vec<String>,
    pub from_cache: bool,
}

fn thresholds(ai: &Ai, backend: AiBackend) -> (f32, f32) {
    match backend {
        AiBackend::Systemone => (ai.systemone.high, ai.systemone.low),
        _ => (ai.local.high, ai.local.low),
    }
}

pub fn build_view(
    ai: &Ai,
    targets: &[Target],
    backend: AiBackend,
    result: Result<Diagnosis, SuggestError>,
) -> SuggestionView {
    let base = |state: ViewState| SuggestionView {
        backend,
        state,
        kind: None,
        sends_images: super::choices::sends_images(backend),
        cards: Vec::new(),
        none_of_above: None,
        unevaluated: Vec::new(),
        from_cache: false,
    };
    let diagnosis = match result {
        Ok(d) => d,
        Err(SuggestError::Disabled) => return base(ViewState::Off),
        Err(SuggestError::Unavailable(message)) => return base(ViewState::Unavailable { message }),
        Err(SuggestError::Failed(message)) => return base(ViewState::Failed { message }),
    };

    let (high, low) = thresholds(ai, backend);
    let choices = build_choices(targets, backend);
    let ranked = rank(&diagnosis.suggestion, &choices, ai.top_k, high, low);
    let ids: Vec<String> = targets.iter().map(|t| target_id(&t.path)).collect();
    let cards = ranked
        .into_iter()
        .filter_map(|(scored, level)| {
            let i = ids.iter().position(|id| *id == scored.id)?;
            let t = &targets[i];
            Some(CardView {
                target: i,
                key: t.key.clone(),
                name: t.display_name(),
                path: t.path.to_string_lossy().into_owned(),
                score: scored.score,
                level,
            })
        })
        .collect();
    let probability = diagnosis.suggestion.kind == ScoreKind::Probability;
    SuggestionView {
        kind: Some(diagnosis.suggestion.kind),
        cards,
        none_of_above: diagnosis.suggestion.none_of_above.filter(|_| probability),
        unevaluated: if probability {
            diagnosis.unevaluated.iter().map(|c| c.label.clone()).collect()
        } else {
            Vec::new()
        },
        from_cache: diagnosis.from_cache,
        ..base(ViewState::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::{Choice, Scored, Suggestion};
    use std::path::Path;

    fn target(key: &str, name: &str, path: &str, exclude: bool) -> Target {
        Target {
            key: key.into(),
            name: name.into(),
            path: Path::new(path).to_path_buf(),
            description: String::new(),
            exclude_external: exclude,
        }
    }

    fn targets() -> Vec<Target> {
        vec![
            target("1", "風景", "/nonexistent-lumiwake/風景", false),
            target("2", "人物", "/nonexistent-lumiwake/人物", false),
            target("3", "書類", "/nonexistent-lumiwake/書類", true),
        ]
    }

    fn id(i: usize) -> String {
        target_id(&targets()[i].path)
    }

    fn diagnosis(kind: ScoreKind, scores: &[(usize, f32)], none: Option<f32>, unevaluated: &[usize]) -> Diagnosis {
        Diagnosis {
            suggestion: Suggestion {
                kind,
                scores: scores.iter().map(|(i, s)| Scored { id: id(*i), score: *s }).collect(),
                none_of_above: none,
            },
            unevaluated: unevaluated
                .iter()
                .map(|i| Choice { id: id(*i), label: targets()[*i].display_name(), description: String::new() })
                .collect(),
            diagnosed_at: "t".into(),
            from_cache: true,
        }
    }

    #[test]
    fn local_view_shows_ranked_cards_with_target_keys_and_paths() {
        let ai = Ai::default(); // top_k 3, local 0.8 / 0.4
        let d = diagnosis(ScoreKind::Match, &[(0, 0.5), (1, 0.9), (2, 0.2)], None, &[]);
        let v = build_view(&ai, &targets(), AiBackend::Local, Ok(d));
        assert_eq!(v.state, ViewState::Ready);
        assert_eq!(v.kind, Some(ScoreKind::Match));
        assert!(!v.sends_images);
        let got: Vec<_> = v.cards.iter().map(|c| (c.target, c.key.as_str(), c.name.as_str(), c.level)).collect();
        assert_eq!(got, [(1, "2", "人物", Level::High), (0, "1", "風景", Level::Mid)], "低（書類）は出さない");
        assert_eq!(v.cards[0].path, "/nonexistent-lumiwake/人物");
        assert_eq!(v.none_of_above, None);
    }

    #[test]
    fn systemone_view_has_none_of_above_and_unevaluated_names() {
        let ai = Ai { backend: AiBackend::Systemone, ..Ai::default() };
        let d = diagnosis(ScoreKind::Probability, &[(0, 0.6), (1, 0.2)], Some(0.2), &[1]);
        let v = build_view(&ai, &targets(), AiBackend::Systemone, Ok(d));
        assert!(v.sends_images, "外部送信中の表示に使う");
        assert_eq!(v.none_of_above, Some(0.2));
        assert_eq!(v.unevaluated, ["人物"]);
        assert_eq!(v.cards.len(), 1, "systemone のしきい値は low 0.50 なので 0.6 だけ");
        assert_eq!(v.cards[0].level, Level::Mid);
    }

    #[test]
    fn excluded_folders_never_appear_as_cards_for_an_external_backend() {
        let ai = Ai { backend: AiBackend::Systemone, top_k: 5, ..Ai::default() };
        // 万一キャッシュ等に残っていても、送信除外のフォルダは候補にしない
        let d = diagnosis(ScoreKind::Probability, &[(0, 0.5), (2, 0.5)], Some(0.0), &[]);
        let v = build_view(&ai, &targets(), AiBackend::Systemone, Ok(d));
        assert_eq!(v.cards.iter().map(|c| c.target).collect::<Vec<_>>(), [0]);
    }

    #[test]
    fn unevaluated_is_only_for_probability_backends() {
        let ai = Ai::default();
        let d = diagnosis(ScoreKind::Match, &[(0, 0.9)], None, &[1]);
        assert!(build_view(&ai, &targets(), AiBackend::Local, Ok(d)).unevaluated.is_empty());
    }

    #[test]
    fn top_k_limits_the_number_of_cards() {
        let ai = Ai { top_k: 1, ..Ai::default() };
        let d = diagnosis(ScoreKind::Match, &[(0, 0.9), (1, 0.85)], None, &[]);
        assert_eq!(build_view(&ai, &targets(), AiBackend::Local, Ok(d)).cards.len(), 1);
    }

    #[test]
    fn errors_map_to_states_without_cards() {
        let ai = Ai::default();
        let t = targets();
        let off = build_view(&ai, &t, AiBackend::Off, Err(SuggestError::Disabled));
        assert_eq!(off.state, ViewState::Off);
        let un = build_view(&ai, &t, AiBackend::Systemone, Err(SuggestError::Unavailable("同意が必要".into())));
        assert_eq!(un.state, ViewState::Unavailable { message: "同意が必要".into() });
        assert!(un.sends_images && un.cards.is_empty());
        let f = build_view(&ai, &t, AiBackend::Local, Err(SuggestError::Failed("x".into())));
        assert_eq!(f.state, ViewState::Failed { message: "x".into() });
    }

    #[test]
    fn view_serializes_with_flat_camel_case_state() {
        let v = build_view(&Ai::default(), &targets(), AiBackend::Off, Err(SuggestError::Disabled));
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["state"], "off");
        assert_eq!(j["backend"], "off");
        assert_eq!(j["sendsImages"], false);
        let u = build_view(&Ai::default(), &targets(), AiBackend::Local, Err(SuggestError::Unavailable("m".into())));
        let j = serde_json::to_value(&u).unwrap();
        assert_eq!((j["state"].as_str(), j["message"].as_str()), (Some("unavailable"), Some("m")));
    }
}
