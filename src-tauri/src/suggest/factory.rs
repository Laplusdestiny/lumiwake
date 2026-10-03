//! 設定から Suggester を作る。外部送信への同意の確認はここで一元的に行う。

use super::dummy::OffSuggester;
use super::{SuggestError, SuggestRequest, Suggester, Suggestion};
use crate::config::ai::{Ai, AiBackend};
use std::sync::Arc;

/// 使えないバックエンド（未導入・未同意など）。診断を求められても何も送らず、理由を返す
pub struct UnavailableSuggester {
    backend: AiBackend,
    reason: String,
}

impl UnavailableSuggester {
    pub fn new(backend: AiBackend, reason: impl Into<String>) -> Self {
        UnavailableSuggester { backend, reason: reason.into() }
    }
}

impl Suggester for UnavailableSuggester {
    fn backend(&self) -> AiBackend {
        self.backend
    }

    fn model(&self) -> String {
        String::new()
    }

    fn available(&self) -> bool {
        false
    }

    fn suggest(&self, _req: &SuggestRequest) -> Result<Suggestion, SuggestError> {
        Err(SuggestError::Unavailable(self.reason.clone()))
    }
}

pub fn make_suggester(ai: &Ai) -> Arc<dyn Suggester> {
    match ai.backend {
        AiBackend::Off => Arc::new(OffSuggester),
        AiBackend::Local => Arc::new(UnavailableSuggester::new(AiBackend::Local, "ローカル推論はまだ使えません")),
        AiBackend::Systemone => {
            if !ai.systemone.external_consent {
                // 同意が済むまでは、外部送信するバックエンドそのものを作らない
                return Arc::new(UnavailableSuggester::new(
                    AiBackend::Systemone,
                    "画像の外部送信への同意が済んでいないため、System One は使えません",
                ));
            }
            Arc::new(UnavailableSuggester::new(AiBackend::Systemone, "System One はまだ使えません"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;

    fn ask(s: &dyn Suggester) -> Result<Suggestion, SuggestError> {
        let img = DynamicImage::new_rgb8(1, 1);
        s.suggest(&SuggestRequest { image_hash: "h", image: &img, choices: &[] })
    }

    #[test]
    fn backend_follows_the_setting() {
        for b in [AiBackend::Off, AiBackend::Local, AiBackend::Systemone] {
            let ai = Ai { backend: b, ..Ai::default() };
            assert_eq!(make_suggester(&ai).backend(), b);
        }
    }

    #[test]
    fn systemone_without_consent_refuses_and_says_why() {
        let ai = Ai { backend: AiBackend::Systemone, ..Ai::default() };
        assert!(!ai.systemone.external_consent);
        match ask(&*make_suggester(&ai)) {
            Err(SuggestError::Unavailable(m)) => assert!(m.contains("同意"), "{m}"),
            other => panic!("同意前は診断できないはず: {other:?}"),
        }
    }

    #[test]
    fn off_is_disabled() {
        let ai = Ai { backend: AiBackend::Off, ..Ai::default() };
        assert!(matches!(ask(&*make_suggester(&ai)), Err(SuggestError::Disabled)));
    }
}
