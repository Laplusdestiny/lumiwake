//! 何もしない実装（`off`）と、UI 確認・テスト用のダミー実装。

use super::{ScoreKind, Scored, SuggestError, SuggestRequest, Suggester, Suggestion};
use crate::config::ai::AiBackend;

/// `ai.backend = "off"` のとき。診断は常に無効として返す
pub struct OffSuggester;

impl Suggester for OffSuggester {
    fn backend(&self) -> AiBackend {
        AiBackend::Off
    }

    fn model(&self) -> String {
        String::new()
    }

    fn suggest(&self, _req: &SuggestRequest) -> Result<Suggestion, SuggestError> {
        Err(SuggestError::Disabled)
    }
}

/// 画像ハッシュと選択肢の id だけから決まる、再現可能なスコアを返す（画像は見ない）。
/// 実際のバックエンドが入るまでの UI 確認と、サービス層のテストに使う。
pub struct DummySuggester {
    pub kind: ScoreKind,
}

impl DummySuggester {
    /// 0.05〜1.0 の疑似乱数（FNV-1a）
    fn weight(hash: &str, id: &str) -> f32 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in hash.bytes().chain([0xff]).chain(id.bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        0.05 + (h % 1000) as f32 / 1000.0 * 0.95
    }
}

impl Suggester for DummySuggester {
    fn backend(&self) -> AiBackend {
        match self.kind {
            ScoreKind::Probability => AiBackend::Systemone,
            ScoreKind::Match => AiBackend::Local,
        }
    }

    fn model(&self) -> String {
        "dummy".into()
    }

    fn rescoreable(&self) -> bool {
        self.kind == ScoreKind::Match
    }

    fn suggest(&self, req: &SuggestRequest) -> Result<Suggestion, SuggestError> {
        let weights: Vec<f32> = req.choices.iter().map(|c| Self::weight(req.image_hash, &c.id)).collect();
        let (scores, none_of_above) = match self.kind {
            ScoreKind::Match => (weights, None),
            ScoreKind::Probability => {
                // 「該当なし」も 1 つの選択肢として、合計が 1 になるよう正規化する
                let none = Self::weight(req.image_hash, "\u{0}none");
                let total: f32 = weights.iter().sum::<f32>() + none;
                (weights.iter().map(|w| w / total).collect(), Some(none / total))
            }
        };
        Ok(Suggestion {
            kind: self.kind,
            scores: req.choices.iter().zip(scores).map(|(c, score)| Scored { id: c.id.clone(), score }).collect(),
            none_of_above,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::Choice;
    use image::DynamicImage;

    fn choices() -> Vec<Choice> {
        ["a", "b", "c"]
            .iter()
            .map(|i| Choice { id: (*i).into(), label: (*i).into(), description: String::new() })
            .collect()
    }

    fn run(s: &dyn Suggester, hash: &str, choices: &[Choice]) -> Result<Suggestion, SuggestError> {
        let img = DynamicImage::new_rgb8(1, 1);
        s.suggest(&SuggestRequest { image_hash: hash, image: &img, choices })
    }

    #[test]
    fn off_is_always_disabled() {
        assert!(matches!(run(&OffSuggester, "h", &choices()), Err(SuggestError::Disabled)));
        assert_eq!(OffSuggester.backend(), AiBackend::Off);
    }

    #[test]
    fn dummy_is_deterministic_and_depends_on_the_image() {
        let d = DummySuggester { kind: ScoreKind::Match };
        let a1 = run(&d, "hash-1", &choices()).unwrap();
        assert_eq!(a1, run(&d, "hash-1", &choices()).unwrap());
        assert_ne!(a1, run(&d, "hash-2", &choices()).unwrap());
        assert!(a1.scores.iter().all(|s| (0.0..=1.0).contains(&s.score)));
        assert_eq!(a1.none_of_above, None, "一致度に「該当なし」はない");
        assert_eq!(d.backend(), AiBackend::Local);
    }

    #[test]
    fn dummy_probabilities_sum_to_one_with_none_of_above() {
        let d = DummySuggester { kind: ScoreKind::Probability };
        let s = run(&d, "hash-1", &choices()).unwrap();
        let total: f32 = s.scores.iter().map(|x| x.score).sum::<f32>() + s.none_of_above.unwrap();
        assert!((total - 1.0).abs() < 1e-5);
        assert_eq!(s.scores.len(), 3);
        assert_eq!(d.backend(), AiBackend::Systemone);
    }
}
