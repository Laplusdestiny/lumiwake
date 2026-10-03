//! ファイル操作モジュールの振り分け通知（[`SortEvent`]）を受けて、診断サービスに記録する。
//!
//! 画像のハッシュ計算（SMB 上では遅いことがある）で画面の操作を止めないよう、別スレッドで処理する。
//! 振り分けと取り消しの順序が入れ替わらないよう、ワーカーは 1 つで通知を順に処理する。

use super::choices::target_id;
use super::hash::hash_file;
use super::service::SuggestionService;
use crate::fileops::SortEvent;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

enum Job {
    Events {
        generation: u64,
        events: Vec<SortEvent>,
    },
    /// 溜まった処理がすべて終わるまで待つ（テスト・終了時用）
    Flush(Sender<()>),
}

pub struct OutcomeRecorder {
    tx: Mutex<Sender<Job>>,
}

impl OutcomeRecorder {
    pub fn start(service: Arc<SuggestionService>) -> Self {
        let (tx, rx) = channel::<Job>();
        std::thread::spawn(move || {
            // 振り分けを記録したときのハッシュ。取り消し時は、戻したファイルを読み直さずこれで消す
            let mut recorded: HashMap<(u64, usize), (String, String)> = HashMap::new();
            while let Ok(job) = rx.recv() {
                match job {
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                    Job::Events { generation, events } => {
                        for e in events {
                            handle(&service, &mut recorded, generation, e);
                        }
                    }
                }
            }
        });
        OutcomeRecorder { tx: Mutex::new(tx) }
    }

    /// 通知を渡す（すぐ戻る）。`generation` は仕分け元を読み込むたびに変わる番号
    pub fn send(&self, generation: u64, events: Vec<SortEvent>) {
        if events.is_empty() {
            return;
        }
        let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        let _ = tx.send(Job::Events { generation, events });
    }

    /// これまでに渡した通知の処理が終わるまで待つ
    pub fn flush(&self) {
        let (done_tx, done_rx) = channel();
        let sent = {
            let tx = self.tx.lock().unwrap_or_else(|e| e.into_inner());
            tx.send(Job::Flush(done_tx)).is_ok()
        };
        if sent {
            let _ = done_rx.recv();
        }
    }
}

fn handle(
    service: &SuggestionService,
    recorded: &mut HashMap<(u64, usize), (String, String)>,
    generation: u64,
    event: SortEvent,
) {
    match event {
        SortEvent::Placed { item, dir, file } => {
            // ハッシュを取れない（ファイルが読めない）ときは、記録しないだけで仕分けには影響させない
            let Ok(hash) = hash_file(&file) else { return };
            let dest = target_id(&dir);
            if service.record_outcome(&hash, &dest).is_ok() {
                recorded.insert((generation, item), (hash, dest));
            }
        }
        SortEvent::PlacementUndone { item, .. } => {
            if let Some((hash, dest)) = recorded.remove(&(generation, item)) {
                let _ = service.undo_outcome(&hash, &dest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::dummy::OffSuggester;
    use crate::suggest::store::Store;
    use std::path::{Path, PathBuf};

    fn setup() -> (Arc<SuggestionService>, OutcomeRecorder) {
        let svc = Arc::new(SuggestionService::new(Store::open_in_memory().unwrap(), Arc::new(OffSuggester)));
        let rec = OutcomeRecorder::start(svc.clone());
        (svc, rec)
    }

    fn placed(item: usize, dir: &Path, file: &Path) -> SortEvent {
        SortEvent::Placed { item, dir: dir.to_path_buf(), file: file.to_path_buf() }
    }

    fn undone(item: usize, dir: &Path, file: &Path) -> SortEvent {
        SortEvent::PlacementUndone { item, dir: dir.to_path_buf(), file: file.to_path_buf() }
    }

    fn outcome_hashes(svc: &SuggestionService, dir: &Path) -> Vec<String> {
        svc.outcome_hashes(&target_id(dir)).unwrap()
    }

    fn file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn placed_records_the_content_hash_against_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let f = file(&a, "img1.jpg", "content 1");
        let (svc, rec) = setup();
        rec.send(1, vec![placed(0, &a, &f)]);
        rec.flush();
        assert_eq!(outcome_hashes(&svc, &a), [blake3::hash(b"content 1").to_hex().to_string()]);
    }

    #[test]
    fn undo_removes_the_record_even_if_the_file_is_no_longer_readable() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let f = file(&a, "img1.jpg", "content 1");
        let (svc, rec) = setup();
        rec.send(1, vec![placed(0, &a, &f), undone(0, &a, &tmp.path().join("gone.jpg"))]);
        rec.flush();
        assert!(outcome_hashes(&svc, &a).is_empty(), "取り消しは記録済みのハッシュで消す");
    }

    #[test]
    fn place_undo_place_leaves_exactly_one_record() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let fa = file(&a, "img1.jpg", "same");
        let (svc, rec) = setup();
        rec.send(1, vec![placed(0, &a, &fa)]);
        rec.send(1, vec![undone(0, &a, &fa)]);
        let fb = file(&b, "img1.jpg", "same");
        rec.send(1, vec![placed(0, &b, &fb)]);
        rec.flush();
        assert!(outcome_hashes(&svc, &a).is_empty());
        assert_eq!(outcome_hashes(&svc, &b).len(), 1, "最後に振り分けた先だけが残る");
    }

    #[test]
    fn an_unreadable_file_records_nothing_and_does_not_break_later_events() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let ok = file(&a, "ok.jpg", "ok");
        let (svc, rec) = setup();
        rec.send(
            1,
            vec![placed(0, &a, &a.join("missing.jpg")), undone(0, &a, &a.join("missing.jpg")), placed(1, &a, &ok)],
        );
        rec.flush();
        assert_eq!(outcome_hashes(&svc, &a).len(), 1);
    }

    #[test]
    fn items_of_different_sessions_do_not_collide() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let f1 = file(&a, "one.jpg", "one");
        let f2 = file(&a, "two.jpg", "two");
        let (svc, rec) = setup();
        // 仕分け元を切り替えると、同じ番号 0 が別の画像を指す
        rec.send(1, vec![placed(0, &a, &f1)]);
        rec.send(2, vec![placed(0, &a, &f2)]);
        rec.send(2, vec![undone(0, &a, &f2)]);
        rec.flush();
        assert_eq!(outcome_hashes(&svc, &a), [blake3::hash(b"one").to_hex().to_string()]);
    }
}
