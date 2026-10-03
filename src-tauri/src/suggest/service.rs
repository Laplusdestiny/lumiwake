//! 診断のキャッシュ照会・再診断・重複防止。
//!
//! 診断は 1 画像につき 1 回。結果は画像の中身のハッシュでキャッシュし、先読みで診断した結果も
//! 表示時に再利用する（スキップした画像の結果も残す）。

use super::store::{Store, StoreError};
use super::{
    choices::sends_images, unevaluated, without_choice, Choice, SuggestError, SuggestRequest, Suggester, Suggestion,
};
use crate::config::ai::AiBackend;
use image::DynamicImage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

pub fn backend_name(b: AiBackend) -> &'static str {
    match b {
        AiBackend::Local => "local",
        AiBackend::Systemone => "systemone",
        AiBackend::Off => "off",
    }
}

/// 画面に渡す診断結果。現在の振り分け先に合わせて調整済み
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnosis {
    /// 現在の振り分け先に存在しないものを外し、確率は残りで正規化し直したもの
    pub suggestion: Suggestion,
    /// 診断のあとに追加され、まだ評価されていない振り分け先
    pub unevaluated: Vec<Choice>,
    pub diagnosed_at: String,
    /// キャッシュから返したか（新たに診断していないか）
    pub from_cache: bool,
}

/// (バックエンド名, 画像ハッシュ) ごとの診断の順番待ち
type Gates = Mutex<HashMap<(String, String), Arc<Mutex<()>>>>;

pub struct SuggestionService {
    store: Mutex<Store>,
    suggester: RwLock<Arc<dyn Suggester>>,
    /// 診断中の (バックエンド, ハッシュ)。同じ画像の診断が重なっても外部リクエストを 1 回にする
    in_flight: Gates,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn failed(e: StoreError) -> SuggestError {
    SuggestError::Failed(e.to_string())
}

impl SuggestionService {
    pub fn new(store: Store, suggester: Arc<dyn Suggester>) -> Self {
        SuggestionService {
            store: Mutex::new(store),
            suggester: RwLock::new(suggester),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    pub fn set_suggester(&self, suggester: Arc<dyn Suggester>) {
        *self.suggester.write().unwrap_or_else(|e| e.into_inner()) = suggester;
    }

    fn suggester(&self) -> Arc<dyn Suggester> {
        self.suggester.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn backend(&self) -> AiBackend {
        self.suggester().backend()
    }

    /// 診断を試みる価値があるか（先読みの要否の判断に使う）
    pub fn available(&self) -> bool {
        self.suggester().available()
    }

    /// キャッシュにある診断を、現在の振り分け先に合わせて返す。リクエストは出さない
    pub fn cached(&self, image_hash: &str, current: &[Choice]) -> Result<Option<Diagnosis>, SuggestError> {
        let backend = backend_name(self.backend());
        let stored = lock(&self.store).latest_diagnosis(image_hash, backend).map_err(failed)?;
        Ok(stored.map(|d| {
            let mut suggestion = d.suggestion;
            // 削除された振り分け先は、リクエストなしで取り除いて正規化し直す
            let removed: Vec<String> =
                d.choices.iter().filter(|id| !current.iter().any(|c| &c.id == *id)).cloned().collect();
            for id in &removed {
                suggestion = without_choice(&suggestion, id);
            }
            let unevaluated = unevaluated(&d.choices, current).into_iter().cloned().collect();
            Diagnosis { suggestion, unevaluated, diagnosed_at: d.diagnosed_at, from_cache: true }
        }))
    }

    /// 診断する。キャッシュがあればそれを返す（`force` なら必ず診断し直し、前の結果は履歴に残す）。
    /// 振り分け先が増えていて、バックエンドが外部リクエストなしで採点し直せる（local）なら採点し直す。
    ///
    /// 画像のデコードはバックエンドを呼ぶときだけ必要なので、`load_image` はキャッシュで済めば呼ばれない。
    pub fn diagnose(
        &self,
        image_hash: &str,
        load_image: impl FnOnce() -> Result<DynamicImage, String>,
        current: &[Choice],
        force: bool,
    ) -> Result<Diagnosis, SuggestError> {
        let suggester = self.suggester();
        if suggester.backend() == AiBackend::Off {
            return Err(SuggestError::Disabled);
        }
        if current.is_empty() {
            return Err(SuggestError::Unavailable("振り分け先が設定されていません".into()));
        }
        let backend = backend_name(suggester.backend());

        // 同じ画像の診断を 1 つにまとめる。後から来た側は、先の診断が終わるのを待ってキャッシュを使う
        let key = (backend.to_string(), image_hash.to_string());
        let gate = lock(&self.in_flight).entry(key.clone()).or_default().clone();
        let _turn = lock(&gate);
        let result = self.diagnose_locked(&*suggester, image_hash, load_image, current, force);
        drop(_turn);
        let mut map = lock(&self.in_flight);
        // 待っている人がいなければ（この関数内の gate と map の 2 つだけなら）掃除する
        if map.get(&key).is_some_and(|g| Arc::strong_count(g) <= 2) {
            map.remove(&key);
        }
        result
    }

    fn diagnose_locked(
        &self,
        suggester: &dyn Suggester,
        image_hash: &str,
        load_image: impl FnOnce() -> Result<DynamicImage, String>,
        current: &[Choice],
        force: bool,
    ) -> Result<Diagnosis, SuggestError> {
        if !force {
            if let Some(hit) = self.cached(image_hash, current)? {
                if hit.unevaluated.is_empty() || !suggester.rescoreable() {
                    return Ok(hit);
                }
            }
        }
        let image = load_image().map_err(SuggestError::Failed)?;
        let suggestion = suggester.suggest(&SuggestRequest { image_hash, image: &image, choices: current })?;
        let at = super::time::now_iso8601();
        let ids: Vec<String> = current.iter().map(|c| c.id.clone()).collect();
        lock(&self.store)
            .record_diagnosis(image_hash, backend_name(suggester.backend()), &suggester.model(), &ids, &suggestion, &at)
            .map_err(failed)?;
        Ok(Diagnosis { suggestion, unevaluated: Vec::new(), diagnosed_at: at, from_cache: false })
    }

    /// 振り分けが確定した（ファイルを移動した）ことを記録する
    pub fn record_outcome(&self, image_hash: &str, destination: &str) -> Result<(), SuggestError> {
        lock(&self.store).record_outcome(image_hash, destination, &super::time::now_iso8601()).map_err(failed)
    }

    /// 振り分けを Undo したので、その記録を取り消す
    pub fn undo_outcome(&self, image_hash: &str, destination: &str) -> Result<bool, SuggestError> {
        lock(&self.store).undo_outcome(image_hash, destination).map_err(failed)
    }

    /// 振り分け先へ実際に振り分けられた画像のハッシュ（local の knn の手本）
    pub fn outcome_hashes(&self, destination: &str) -> Result<Vec<String>, SuggestError> {
        lock(&self.store).outcome_hashes(destination).map_err(failed)
    }

    /// このバックエンドが画像を外部へ送るか（画面の「外部送信中」表示に使う）
    pub fn sends_images(&self) -> bool {
        sends_images(self.backend())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::dummy::{DummySuggester, OffSuggester};
    use crate::suggest::ScoreKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// 呼ばれた回数を数えるダミー（外部リクエストの回数の代わり）
    struct Counting {
        inner: DummySuggester,
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl Suggester for Counting {
        fn backend(&self) -> AiBackend {
            self.inner.backend()
        }
        fn model(&self) -> String {
            self.inner.model()
        }
        fn rescoreable(&self) -> bool {
            self.inner.rescoreable()
        }
        fn suggest(&self, req: &SuggestRequest) -> Result<Suggestion, SuggestError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            self.inner.suggest(req)
        }
    }

    fn setup(kind: ScoreKind) -> (SuggestionService, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let s = Counting { inner: DummySuggester { kind }, calls: calls.clone(), delay: Duration::ZERO };
        (SuggestionService::new(Store::open_in_memory().unwrap(), Arc::new(s)), calls)
    }

    fn choices(ids: &[&str]) -> Vec<Choice> {
        ids.iter().map(|i| Choice { id: (*i).into(), label: (*i).into(), description: String::new() }).collect()
    }

    fn img() -> DynamicImage {
        DynamicImage::new_rgb8(2, 2)
    }

    #[test]
    fn an_image_is_diagnosed_only_once() {
        let (svc, calls) = setup(ScoreKind::Probability);
        let c = choices(&["a", "b"]);
        let first = svc.diagnose("h", || Ok(img()), &c, false).unwrap();
        assert!(!first.from_cache);
        let second = svc.diagnose("h", || Ok(img()), &c, false).unwrap();
        assert!(second.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "2 回目はリクエストを出さない");
        assert_eq!(first.suggestion, second.suggestion);
        svc.diagnose("other", || Ok(img()), &c, false).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "別の画像は診断する");
    }

    #[test]
    fn cache_is_keyed_by_content_so_a_moved_file_is_not_diagnosed_again() {
        let (svc, calls) = setup(ScoreKind::Probability);
        let c = choices(&["a"]);
        svc.diagnose("same-content-hash", || Ok(img()), &c, false).unwrap();
        // 移動・改名しても、ハッシュ（中身）が同じなら同じ画像
        svc.diagnose("same-content-hash", || Ok(img()), &c, false).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn forced_rediagnosis_calls_again_and_keeps_history() {
        let (svc, calls) = setup(ScoreKind::Probability);
        let c = choices(&["a", "b"]);
        svc.diagnose("h", || Ok(img()), &c, false).unwrap();
        let again = svc.diagnose("h", || Ok(img()), &c, true).unwrap();
        assert!(!again.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let hist = lock(&svc.store).diagnosis_history("h").unwrap();
        assert_eq!(hist.len(), 2);
        assert_eq!(hist.iter().filter(|d| d.is_latest).count(), 1);
    }

    #[test]
    fn concurrent_diagnoses_of_one_image_make_a_single_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let s = Counting {
            inner: DummySuggester { kind: ScoreKind::Probability },
            calls: calls.clone(),
            delay: Duration::from_millis(80),
        };
        let svc = Arc::new(SuggestionService::new(Store::open_in_memory().unwrap(), Arc::new(s)));
        let c = choices(&["a", "b"]);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (svc, c) = (svc.clone(), c.clone());
                std::thread::spawn(move || svc.diagnose("h", || Ok(img()), &c, false).unwrap())
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "先読みと表示が重なっても二重リクエストにならない");
        assert!(results.windows(2).all(|w| w[0].suggestion == w[1].suggestion));
        assert!(lock(&svc.in_flight).is_empty(), "終わったら管理用の項目を残さない");
    }

    #[test]
    fn removed_folders_are_dropped_and_probabilities_renormalized_without_a_request() {
        let (svc, calls) = setup(ScoreKind::Probability);
        svc.diagnose("h", || Ok(img()), &choices(&["a", "b", "c"]), false).unwrap();
        let after = svc.diagnose("h", || Ok(img()), &choices(&["a", "c"]), false).unwrap();
        assert!(after.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let ids: Vec<_> = after.suggestion.scores.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
        let total: f32 =
            after.suggestion.scores.iter().map(|s| s.score).sum::<f32>() + after.suggestion.none_of_above.unwrap();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(after.unevaluated.is_empty());
    }

    #[test]
    fn a_new_folder_is_reported_unevaluated_for_a_request_based_backend() {
        let (svc, calls) = setup(ScoreKind::Probability);
        svc.diagnose("h", || Ok(img()), &choices(&["a", "b"]), false).unwrap();
        let after = svc.diagnose("h", || Ok(img()), &choices(&["a", "b", "new"]), false).unwrap();
        assert!(after.from_cache, "自動では再診断しない");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(after.unevaluated.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["new"]);
        // 再診断のキーを押すと評価される
        let redo = svc.diagnose("h", || Ok(img()), &choices(&["a", "b", "new"]), true).unwrap();
        assert!(redo.suggestion.scores.iter().any(|s| s.id == "new"));
        assert!(svc.cached("h", &choices(&["a", "b", "new"])).unwrap().unwrap().unevaluated.is_empty());
    }

    #[test]
    fn a_new_folder_is_scored_in_place_for_a_rescoreable_backend() {
        let (svc, _calls) = setup(ScoreKind::Match);
        svc.diagnose("h", || Ok(img()), &choices(&["a"]), false).unwrap();
        let after = svc.diagnose("h", || Ok(img()), &choices(&["a", "new"]), false).unwrap();
        assert!(after.suggestion.scores.iter().any(|s| s.id == "new"), "その場で採点し直す");
        assert!(after.unevaluated.is_empty());
    }

    #[test]
    fn the_image_is_decoded_only_when_the_backend_is_called() {
        let (svc, _calls) = setup(ScoreKind::Probability);
        let c = choices(&["a"]);
        let loads = AtomicUsize::new(0);
        let load = || {
            loads.fetch_add(1, Ordering::SeqCst);
            Ok(img())
        };
        svc.diagnose("h", load, &c, false).unwrap();
        svc.diagnose("h", load, &c, false).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 1, "キャッシュで済むときはデコードしない");
        let broken = svc.diagnose("other", || Err("壊れた画像".to_string()), &c, false);
        assert!(matches!(broken, Err(SuggestError::Failed(m)) if m == "壊れた画像"));
        assert!(lock(&svc.store).diagnosis_history("other").unwrap().is_empty(), "失敗は記録しない");
    }

    #[test]
    fn cached_never_calls_the_backend() {
        let (svc, calls) = setup(ScoreKind::Probability);
        assert!(svc.cached("h", &choices(&["a"])).unwrap().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn off_and_empty_choices_make_no_request_and_record_nothing() {
        let svc = SuggestionService::new(Store::open_in_memory().unwrap(), Arc::new(OffSuggester));
        assert!(matches!(svc.diagnose("h", || Ok(img()), &choices(&["a"]), false), Err(SuggestError::Disabled)));
        let (svc, calls) = setup(ScoreKind::Match);
        assert!(matches!(svc.diagnose("h", || Ok(img()), &[], false), Err(SuggestError::Unavailable(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(lock(&svc.store).diagnosis_history("h").unwrap().is_empty());
    }

    #[test]
    fn switching_the_backend_uses_a_separate_cache() {
        let (svc, calls) = setup(ScoreKind::Probability);
        svc.diagnose("h", || Ok(img()), &choices(&["a"]), false).unwrap();
        svc.set_suggester(Arc::new(DummySuggester { kind: ScoreKind::Match }));
        assert!(svc.cached("h", &choices(&["a"])).unwrap().is_none(), "local の結果は systemone と別");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn outcomes_can_be_recorded_and_undone() {
        let (svc, _) = setup(ScoreKind::Match);
        svc.record_outcome("h", "/p/a").unwrap();
        assert_eq!(lock(&svc.store).outcome_hashes("/p/a").unwrap(), ["h"]);
        assert!(svc.undo_outcome("h", "/p/a").unwrap());
        assert!(lock(&svc.store).outcome_hashes("/p/a").unwrap().is_empty());
    }
}
