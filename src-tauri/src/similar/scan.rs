//! 類似判定用の情報（ハッシュ・解像度・サイズ）をバックグラウンドで集める。
//!
//! 結果は画像の元の場所のパスで持つ。並び順を変えて画像の番号が変わっても使い続けられる。

use super::group::Fingerprint;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// 走査する画像 1 枚
#[derive(Debug, Clone)]
pub struct ScanJob {
    /// 仕分け元での元の場所（結果のキー）
    pub key: PathBuf,
    /// 今ある場所（ここから読む）
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub generation: u64,
    /// 判定を終えた枚数（失敗を含む）
    pub done: usize,
    /// `done` と、まだ判定していない枚数の合計
    pub total: usize,
    pub running: bool,
    /// 読めなかった枚数
    pub failed: usize,
}

pub type PrintFn = Arc<dyn Fn(&Path) -> Result<Fingerprint, String> + Send + Sync>;

#[derive(Default)]
struct Inner {
    generation: u64,
    /// 仕分け元を読み直すたびに変わる。読んでいる途中だった古い結果を見分ける
    epoch: u64,
    /// 判定を終えた画像。読めなかったものは None
    table: HashMap<PathBuf, Option<Fingerprint>>,
    queue: VecDeque<ScanJob>,
    /// 待ち、または読んでいる途中の画像（同じ画像を 2 回読まない）
    pending: HashSet<PathBuf>,
    /// 今の epoch で読んでいる途中の枚数
    in_flight: usize,
    /// 中身を読み比べた結果（パスの組 → 完全に同じか）
    exact: HashMap<(PathBuf, PathBuf), bool>,
    workers: usize,
    load: Option<PrintFn>,
}

impl Inner {
    fn status(&self) -> ScanStatus {
        let done = self.table.len();
        ScanStatus {
            generation: self.generation,
            done,
            total: done + self.queue.len() + self.in_flight,
            running: !self.queue.is_empty() || self.in_flight > 0,
            failed: self.table.values().filter(|v| v.is_none()).count(),
        }
    }
}

#[derive(Default)]
pub struct SimilarScan {
    inner: Mutex<Inner>,
}

impl SimilarScan {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 仕分け元を読み直した: 結果を捨て、進行中の走査を中止する
    pub fn reset(&self, generation: u64) {
        let mut s = self.lock();
        s.generation = generation;
        s.epoch += 1;
        s.table.clear();
        s.exact.clear();
        s.queue.clear();
        s.pending.clear();
        s.in_flight = 0;
    }

    /// 並び順が変わって番号を付け替えた: 結果は残し、待ちの分だけ捨てる
    pub fn renumber(&self, generation: u64) {
        let mut s = self.lock();
        s.generation = generation;
        for job in std::mem::take(&mut s.queue) {
            s.pending.remove(&job.key);
        }
    }

    /// まだ結果のない画像（前に読めなかったものを含む）の走査を始める。`generation` が今のものと違えば何もしない
    pub fn start(self: &Arc<Self>, generation: u64, jobs: Vec<ScanJob>, workers: usize, load: PrintFn) -> ScanStatus {
        let mut s = self.lock();
        if s.generation != generation {
            return s.status();
        }
        for job in jobs {
            // 前に読めなかった画像は読み直す（順番が来たときに移動の途中だった、一時的に読めなかった、など）
            if matches!(s.table.get(&job.key), Some(None)) {
                s.table.remove(&job.key);
            }
            if !s.table.contains_key(&job.key) && s.pending.insert(job.key.clone()) {
                s.queue.push_back(job);
            }
        }
        s.load = Some(load);
        let wanted = workers.max(1).min(s.queue.len());
        while s.workers < wanted {
            s.workers += 1;
            let scan = Arc::clone(self);
            std::thread::spawn(move || scan.work());
        }
        s.status()
    }

    /// ワーカー: 待ちがなくなるまで 1 枚ずつ判定する
    fn work(&self) {
        loop {
            let (job, load, epoch) = {
                let mut s = self.lock();
                let next = s.queue.pop_front().zip(s.load.clone());
                let Some((job, load)) = next else {
                    s.workers -= 1;
                    return;
                };
                s.in_flight += 1;
                (job, load, s.epoch)
            };
            // デコーダが異常終了しても、その 1 枚を「読めなかった」として先へ進む
            let result = catch_unwind(AssertUnwindSafe(|| load(&job.path))).ok().and_then(Result::ok);
            let mut s = self.lock();
            // 読んでいる間に仕分け元が変わっていたら、結果は捨てる
            if s.epoch == epoch {
                s.in_flight -= 1;
                s.pending.remove(&job.key);
                s.table.insert(job.key, result);
            }
        }
    }

    pub fn status(&self) -> ScanStatus {
        self.lock().status()
    }

    /// 2 つのファイルの中身が同じか。一度確かめた組は覚えておき、グループを作り直すたびに読み直さない
    pub fn exact(&self, a: &Path, b: &Path, compare: impl FnOnce() -> bool) -> bool {
        let key = if a <= b { (a.to_path_buf(), b.to_path_buf()) } else { (b.to_path_buf(), a.to_path_buf()) };
        let epoch = {
            let s = self.lock();
            if let Some(&known) = s.exact.get(&key) {
                return known;
            }
            s.epoch
        };
        // ファイルを読む間はロックを持たない（走査を止めない）
        let same = compare();
        let mut s = self.lock();
        if s.epoch == epoch {
            s.exact.insert(key, same);
        }
        same
    }

    /// 判定できた画像の情報
    pub fn get(&self, key: &Path) -> Option<Fingerprint> {
        self.lock().table.get(key).copied().flatten()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Barrier;
    use std::time::{Duration, Instant};

    fn jobs(names: &[&str]) -> Vec<ScanJob> {
        names.iter().map(|n| ScanJob { key: PathBuf::from(n), path: PathBuf::from(n) }).collect()
    }

    fn print(hash: u64) -> Fingerprint {
        Fingerprint { hash, width: 4, height: 3, size: 10 }
    }

    /// 名前の長さをハッシュにする偽の判定。呼ばれた回数を数える
    fn counting(calls: &Arc<AtomicUsize>) -> PrintFn {
        let calls = calls.clone();
        Arc::new(move |p: &Path| {
            calls.fetch_add(1, SeqCst);
            Ok(print(p.to_string_lossy().len() as u64))
        })
    }

    /// 走査が終わるまで待つ
    pub(crate) fn wait(scan: &SimilarScan) -> ScanStatus {
        let start = Instant::now();
        loop {
            let st = scan.status();
            if !st.running {
                return st;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "走査が終わらない: {st:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    const FIVE: [&str; 5] = ["a", "bb", "ccc", "dddd", "eeeee"];

    #[test]
    fn scans_every_job_and_reports_progress() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        assert_eq!(scan.status(), ScanStatus { generation: 1, done: 0, total: 0, running: false, failed: 0 });
        let calls = Arc::new(AtomicUsize::new(0));
        let started = scan.start(1, jobs(&FIVE), 3, counting(&calls));
        assert_eq!((started.generation, started.total), (1, 5));
        assert_eq!(wait(&scan), ScanStatus { generation: 1, done: 5, total: 5, running: false, failed: 0 });
        assert_eq!(scan.get(Path::new("ccc")), Some(print(3)));
        assert_eq!(scan.get(Path::new("zzz")), None);
        assert_eq!(calls.load(SeqCst), 5);
    }

    #[test]
    fn failures_are_counted_and_not_returned() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let load: PrintFn =
            Arc::new(|p: &Path| if p == Path::new("bb") { Err("壊れている".into()) } else { Ok(print(1)) });
        scan.start(1, jobs(&FIVE), 2, load);
        assert_eq!(wait(&scan), ScanStatus { generation: 1, done: 5, total: 5, running: false, failed: 1 });
        assert_eq!(scan.get(Path::new("bb")), None);
        assert!(scan.get(Path::new("a")).is_some());
    }

    #[test]
    fn a_panicking_loader_counts_as_a_failure_and_the_scan_finishes() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let load: PrintFn = Arc::new(|p: &Path| {
            if p == Path::new("ccc") {
                panic!("デコーダの異常終了");
            }
            Ok(print(1))
        });
        // ワーカー 1 本でも、panic のあと残りを処理し終える
        scan.start(1, jobs(&FIVE), 1, load);
        assert_eq!(wait(&scan), ScanStatus { generation: 1, done: 5, total: 5, running: false, failed: 1 });
        assert_eq!(scan.get(Path::new("ccc")), None);
        assert!(scan.get(Path::new("eeeee")).is_some());
    }

    #[test]
    fn failed_images_are_retried_on_the_next_start() {
        // 判定の順番が来たときに一時的に読めなかった画像（移動の途中、SMB の一時的な失敗など）
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let flaky = |calls: &Arc<AtomicUsize>, fail: bool| -> PrintFn {
            let calls = calls.clone();
            Arc::new(move |p: &Path| {
                calls.fetch_add(1, SeqCst);
                if fail && p == Path::new("bb") {
                    Err("今は読めない".into())
                } else {
                    Ok(print(1))
                }
            })
        };
        scan.start(1, jobs(&FIVE), 2, flaky(&calls, true));
        assert_eq!(wait(&scan).failed, 1);
        // 失敗した画像だけをもう一度読む
        let st = scan.start(1, jobs(&FIVE), 2, flaky(&calls, false));
        assert_eq!((st.done, st.total, st.failed), (4, 5, 0), "読み直す分は判定待ちに戻す");
        assert_eq!(wait(&scan), ScanStatus { generation: 1, done: 5, total: 5, running: false, failed: 0 });
        assert!(scan.get(Path::new("bb")).is_some());
        assert_eq!(calls.load(SeqCst), 6);
    }

    #[test]
    fn exact_match_results_are_remembered_until_the_folder_is_reloaded() {
        let scan = SimilarScan::new();
        scan.reset(1);
        let reads = AtomicUsize::new(0);
        let compare = |answer: bool| {
            reads.fetch_add(1, SeqCst);
            answer
        };
        let (a, b, c) = (Path::new("a"), Path::new("b"), Path::new("c"));
        assert!(scan.exact(a, b, || compare(true)));
        assert!(scan.exact(a, b, || compare(false)), "覚えた結果を返し、読み直さない");
        assert!(scan.exact(b, a, || compare(false)), "向きを変えても同じ組");
        assert!(!scan.exact(a, c, || compare(false)));
        assert_eq!(reads.load(SeqCst), 2);

        scan.renumber(2);
        assert!(scan.exact(a, b, || compare(false)), "並び順を変えても覚えている");
        scan.reset(3);
        assert!(!scan.exact(a, b, || compare(false)), "読み直したら確かめ直す");
        assert_eq!(reads.load(SeqCst), 3);
    }

    #[test]
    fn known_images_are_not_loaded_again() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let calls = Arc::new(AtomicUsize::new(0));
        scan.start(1, jobs(&FIVE), 2, counting(&calls));
        wait(&scan);
        let mut more = jobs(&FIVE);
        more.extend(jobs(&["ffffff", "ffffff"]));
        scan.start(1, more, 2, counting(&calls));
        assert_eq!(wait(&scan), ScanStatus { generation: 1, done: 6, total: 6, running: false, failed: 0 });
        assert_eq!(calls.load(SeqCst), 6, "新しい 1 枚だけ読む（同じ画像を 2 回足しても 1 回）");
    }

    /// 最初の 1 枚を読んでいる途中で止められる偽の判定
    fn gated() -> (PrintFn, Arc<Barrier>, Arc<Barrier>) {
        let (entered, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
        let first = Mutex::new(true);
        let (e, r) = (entered.clone(), release.clone());
        let load: PrintFn = Arc::new(move |_: &Path| {
            if std::mem::take(&mut *first.lock().unwrap()) {
                e.wait();
                r.wait();
            }
            Ok(print(9))
        });
        (load, entered, release)
    }

    #[test]
    fn reset_discards_results_of_an_older_scan() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let (load, entered, release) = gated();
        scan.start(1, jobs(&FIVE), 1, load);
        entered.wait(); // 1 枚目を読んでいる途中
        scan.reset(2); // 仕分け元を切り替えた
        release.wait();
        assert_eq!(wait(&scan), ScanStatus { generation: 2, done: 0, total: 0, running: false, failed: 0 });
        assert_eq!(scan.get(Path::new("a")), None, "古い走査の結果を混ぜない");
    }

    #[test]
    fn a_new_scan_can_start_while_an_older_one_is_winding_down() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let (load, entered, release) = gated();
        scan.start(1, jobs(&["old1", "old2"]), 1, load);
        entered.wait();
        scan.reset(2);
        let calls = Arc::new(AtomicUsize::new(0));
        scan.start(2, jobs(&FIVE), 2, counting(&calls));
        release.wait();
        assert_eq!(wait(&scan), ScanStatus { generation: 2, done: 5, total: 5, running: false, failed: 0 });
        assert_eq!(scan.get(Path::new("old1")), None);
        assert_eq!(scan.get(Path::new("bb")), Some(print(2)));
    }

    #[test]
    fn start_with_a_stale_generation_does_nothing() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let st = scan.start(1, jobs(&FIVE), 2, counting(&calls));
        assert_eq!(st, ScanStatus { generation: 2, done: 0, total: 0, running: false, failed: 0 });
        assert_eq!(wait(&scan).total, 0);
        assert_eq!(calls.load(SeqCst), 0);
    }

    #[test]
    fn renumber_keeps_results_and_accepts_the_new_generation() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let calls = Arc::new(AtomicUsize::new(0));
        scan.start(1, jobs(&FIVE), 2, counting(&calls));
        wait(&scan);
        scan.renumber(2);
        assert_eq!(scan.get(Path::new("a")), Some(print(1)), "結果は残す");
        let st = scan.start(2, jobs(&FIVE), 2, counting(&calls));
        assert_eq!(st, ScanStatus { generation: 2, done: 5, total: 5, running: false, failed: 0 });
        assert_eq!(calls.load(SeqCst), 5, "読み直さない");
    }

    #[test]
    fn renumber_during_a_scan_keeps_finished_results_and_drops_the_queue() {
        let scan = Arc::new(SimilarScan::new());
        scan.reset(1);
        let (load, entered, release) = gated();
        scan.start(1, jobs(&FIVE), 1, load);
        entered.wait();
        scan.renumber(2);
        release.wait();
        let st = wait(&scan);
        assert_eq!((st.generation, st.running), (2, false));
        assert!(st.done <= 1 && st.total == st.done, "待ちの分は捨てる: {st:?}");
    }
}
