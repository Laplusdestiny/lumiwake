//! AI 振り分け候補: バックエンドに依存しない型と、候補の順位付け。
//!
//! どのバックエンドも出力は「振り分け先ごとのスコア」に統一し、画面側はバックエンドを意識しない。
//! AI は候補を示すだけで、ファイルを動かすのは常にユーザーのキー操作（ファイル操作モジュール）。

pub mod choices;
pub mod dummy;
pub mod factory;
pub mod hash;
pub mod image_prep;
pub mod outcomes;
pub mod prefetch;
pub mod service;
pub mod store;
pub mod time;
pub mod view;

use crate::config::ai::AiBackend;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// 候補に挙げる振り分け先。`id` はキャッシュ上の識別子（現状は正規化したフルパス）で、画面には出さない
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    /// 表示名（指示文のラベル）
    pub label: String,
    /// 何を入れるフォルダかの説明。空ならラベルだけで判断する
    pub description: String,
}

/// スコアの意味。`Match`（一致度）は較正された確率ではないので、画面では確率と区別して表記する
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreKind {
    Probability,
    Match,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scored {
    pub id: String,
    pub score: f32,
}

/// 1 枚の画像に対する診断結果（全選択肢ぶん）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub kind: ScoreKind,
    pub scores: Vec<Scored>,
    /// 「該当なし」の確率（systemone のみ。local は None）
    pub none_of_above: Option<f32>,
}

/// 診断の入力
pub struct SuggestRequest<'a> {
    /// 画像ファイルの中身のハッシュ（キャッシュのキー）
    pub image_hash: &'a str,
    pub image: &'a DynamicImage,
    pub choices: &'a [Choice],
}

#[derive(Debug, thiserror::Error)]
pub enum SuggestError {
    #[error("AI 候補は無効です")]
    Disabled,
    #[error("{0}")]
    Unavailable(String),
    #[error("診断に失敗しました: {0}")]
    Failed(String),
}

pub trait Suggester: Send + Sync {
    fn backend(&self) -> AiBackend;
    /// キャッシュの記録に使うモデル名
    fn model(&self) -> String;
    fn suggest(&self, req: &SuggestRequest) -> Result<Suggestion, SuggestError>;
    /// 診断を試みる価値があるか。使えない（無効・未同意・未導入）なら、先読みで画像を読みに行かない
    fn available(&self) -> bool {
        true
    }
    /// 振り分け先が増えたとき、外部リクエストなしでその場で採点し直せるか（local は埋め込みのキャッシュで可能）
    fn rescoreable(&self) -> bool {
        false
    }
}

/// 候補カードでの扱い。高：強調表示、中：通常表示、低：表示しない
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    High,
    Mid,
    Low,
}

pub fn level(score: f32, high: f32, low: f32) -> Level {
    if score >= high {
        Level::High
    } else if score >= low {
        Level::Mid
    } else {
        Level::Low
    }
}

/// 候補として見せるもの。現在の振り分け先（`current`）にあるものだけを、スコアの高い順に上位 `k` 件、
/// 「低」を除いて返す。スコアが同じなら `current` の並び順を保つ。
pub fn rank<'a>(s: &'a Suggestion, current: &[Choice], k: usize, high: f32, low: f32) -> Vec<(&'a Scored, Level)> {
    let order = |id: &str| current.iter().position(|c| c.id == id);
    let mut v: Vec<_> = s
        .scores
        .iter()
        .filter(|x| order(&x.id).is_some())
        .map(|x| (x, level(x.score, high, low)))
        .filter(|(_, l)| *l != Level::Low)
        .collect();
    v.sort_by(|(a, _), (b, _)| b.score.total_cmp(&a.score).then_with(|| order(&a.id).cmp(&order(&b.id))));
    v.truncate(k);
    v
}

/// 診断時の選択肢に含まれていなかった（＝未評価の）現在の振り分け先
pub fn unevaluated<'a>(evaluated_ids: &[String], current: &'a [Choice]) -> Vec<&'a Choice> {
    current.iter().filter(|c| !evaluated_ids.contains(&c.id)).collect()
}

/// 振り分け先を取り除いたとき、残りの確率を足して 1 になるよう正規化し直す（リクエストなし）。
/// `Match`（一致度）は確率ではないので、取り除くだけで値は変えない。
pub fn without_choice(s: &Suggestion, removed_id: &str) -> Suggestion {
    let mut out = s.clone();
    out.scores.retain(|x| x.id != removed_id);
    if out.kind == ScoreKind::Probability {
        let total: f32 = out.scores.iter().map(|x| x.score).sum::<f32>() + out.none_of_above.unwrap_or(0.0);
        if total > 0.0 {
            for x in &mut out.scores {
                x.score /= total;
            }
            if let Some(n) = out.none_of_above.as_mut() {
                *n /= total;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str) -> Choice {
        Choice { id: id.into(), label: id.into(), description: String::new() }
    }

    fn scored(id: &str, score: f32) -> Scored {
        Scored { id: id.into(), score }
    }

    fn prob(scores: &[(&str, f32)], none: Option<f32>) -> Suggestion {
        Suggestion {
            kind: ScoreKind::Probability,
            scores: scores.iter().map(|(i, s)| scored(i, *s)).collect(),
            none_of_above: none,
        }
    }

    #[test]
    fn level_uses_inclusive_thresholds() {
        assert_eq!(level(0.80, 0.80, 0.40), Level::High);
        assert_eq!(level(0.79, 0.80, 0.40), Level::Mid);
        assert_eq!(level(0.40, 0.80, 0.40), Level::Mid);
        assert_eq!(level(0.39, 0.80, 0.40), Level::Low);
    }

    #[test]
    fn rank_returns_top_k_without_low_scores_in_descending_order() {
        let s = prob(&[("a", 0.1), ("b", 0.5), ("c", 0.85), ("d", 0.45), ("e", 0.6)], None);
        let cur: Vec<_> = ["a", "b", "c", "d", "e"].into_iter().map(choice).collect();
        let r = rank(&s, &cur, 3, 0.8, 0.4);
        let ids: Vec<_> = r.iter().map(|(x, _)| x.id.as_str()).collect();
        assert_eq!(ids, ["c", "e", "b"], "低（a）は出さず、上位 3 件");
        assert_eq!(r[0].1, Level::High);
        assert_eq!(r[1].1, Level::Mid);
    }

    #[test]
    fn rank_ignores_choices_that_no_longer_exist_and_breaks_ties_by_current_order() {
        let s = prob(&[("gone", 0.9), ("b", 0.5), ("a", 0.5)], None);
        let cur = vec![choice("a"), choice("b")];
        let ids: Vec<_> = rank(&s, &cur, 5, 0.8, 0.4).iter().map(|(x, _)| x.id.clone()).collect();
        assert_eq!(ids, ["a", "b"], "削除済みの振り分け先は出さず、同点は現在の並び順");
    }

    #[test]
    fn unevaluated_lists_targets_added_after_the_diagnosis() {
        let cur = vec![choice("a"), choice("b"), choice("c")];
        let evaluated = vec!["a".to_string(), "c".to_string()];
        let ids: Vec<_> = unevaluated(&evaluated, &cur).iter().map(|c| c.id.clone()).collect();
        assert_eq!(ids, ["b"]);
        assert!(unevaluated(&["a".into(), "b".into(), "c".into()], &cur).is_empty());
    }

    #[test]
    fn removing_a_choice_renormalizes_probabilities() {
        let s = prob(&[("a", 0.5), ("b", 0.3), ("c", 0.1)], Some(0.1));
        let r = without_choice(&s, "a");
        assert_eq!(r.scores.len(), 2);
        let total: f32 = r.scores.iter().map(|x| x.score).sum::<f32>() + r.none_of_above.unwrap();
        assert!((total - 1.0).abs() < 1e-6, "合計は 1");
        assert!((r.scores[0].score - 0.6).abs() < 1e-6, "b: 0.3 / 0.5");
        assert!((r.none_of_above.unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(s.scores.len(), 3, "元の結果は変えない");
    }

    #[test]
    fn removing_the_only_probability_mass_does_not_divide_by_zero() {
        let s = prob(&[("a", 1.0), ("b", 0.0)], None);
        let r = without_choice(&s, "a");
        assert_eq!(r.scores, vec![scored("b", 0.0)]);
    }

    #[test]
    fn removing_a_choice_keeps_match_scores_unchanged() {
        let s = Suggestion {
            kind: ScoreKind::Match,
            scores: vec![scored("a", 0.7), scored("b", 0.4)],
            none_of_above: None,
        };
        let r = without_choice(&s, "a");
        assert_eq!(r.scores, vec![scored("b", 0.4)], "一致度は確率ではないので正規化しない");
    }
}
