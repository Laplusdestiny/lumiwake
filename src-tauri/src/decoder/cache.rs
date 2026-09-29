//! 表示用画像のキャッシュと先読み。
//!
//! 同じ画像を同時に要求された場合（先読み中に表示要求が来た場合など）は、
//! デコードを 1 回だけ行い、後から来た側は完了を待つ。

use super::Preview;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

enum Slot {
    Loading,
    Ready(Arc<Preview>),
    Failed(String),
}

struct Inner {
    map: HashMap<PathBuf, Slot>,
    /// 古い順（先頭から追い出す）
    order: VecDeque<PathBuf>,
}

pub struct PreviewCache {
    inner: Mutex<Inner>,
    ready: Condvar,
    capacity: usize,
}

impl PreviewCache {
    pub fn new(capacity: usize) -> Self {
        PreviewCache {
            inner: Mutex::new(Inner { map: HashMap::new(), order: VecDeque::new() }),
            ready: Condvar::new(),
            capacity: capacity.max(1),
        }
    }

    /// キャッシュにあれば返し、なければ `load` で作って入れる
    pub fn get_or_load(
        &self,
        key: &Path,
        load: impl FnOnce() -> Result<Preview, String>,
    ) -> Result<Arc<Preview>, String> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            match inner.map.get(key) {
                Some(Slot::Ready(p)) => {
                    let p = p.clone();
                    touch(&mut inner.order, key);
                    return Ok(p);
                }
                Some(Slot::Failed(e)) => return Err(e.clone()),
                Some(Slot::Loading) => inner = self.ready.wait(inner).unwrap(),
                None => break,
            }
        }
        inner.map.insert(key.to_path_buf(), Slot::Loading);
        drop(inner);

        let result = load();

        let mut inner = self.inner.lock().unwrap();
        let (slot, ret) = match result {
            Ok(p) => {
                let p = Arc::new(p);
                (Slot::Ready(p.clone()), Ok(p))
            }
            Err(e) => (Slot::Failed(e.clone()), Err(e)),
        };
        inner.map.insert(key.to_path_buf(), slot);
        touch(&mut inner.order, key);
        self.evict(&mut inner);
        drop(inner);
        self.ready.notify_all();
        ret
    }

    pub fn contains(&self, key: &Path) -> bool {
        self.inner.lock().unwrap().map.contains_key(key)
    }

    /// 仕分け元を読み直したときなどに全部捨てる（読み込み中のものは完了後に入る）
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.map.retain(|_, s| matches!(s, Slot::Loading));
        inner.order.clear();
    }

    fn evict(&self, inner: &mut Inner) {
        while inner.order.len() > self.capacity {
            let Some(old) = inner.order.pop_front() else { break };
            inner.map.remove(&old);
        }
    }
}

fn touch(order: &mut VecDeque<PathBuf>, key: &Path) {
    if let Some(pos) = order.iter().position(|p| p == key) {
        order.remove(pos);
    }
    order.push_back(key.to_path_buf());
}

/// 先読みの依頼: キャッシュのキーと、読み込むファイルの場所
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefetchJob {
    pub key: PathBuf,
    pub path: PathBuf,
}

struct Queue {
    jobs: Mutex<(VecDeque<PrefetchJob>, bool)>,
    cond: Condvar,
}

/// ファイルを読み込んで表示用画像を作る関数
pub type LoadFn = Arc<dyn Fn(&Path) -> Result<Preview, String> + Send + Sync>;

/// バックグラウンドのスレッドで次の数枚を先読みする
pub struct Prefetcher {
    queue: Arc<Queue>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Prefetcher {
    pub fn start(workers: usize, cache: Arc<PreviewCache>, load: LoadFn) -> Self {
        let queue = Arc::new(Queue { jobs: Mutex::new((VecDeque::new(), false)), cond: Condvar::new() });
        let workers = (0..workers.max(1))
            .map(|i| {
                let queue = queue.clone();
                let cache = cache.clone();
                let load = load.clone();
                thread::Builder::new()
                    .name(format!("lumiwake-prefetch-{i}"))
                    .spawn(move || loop {
                        let job = {
                            let mut guard = queue.jobs.lock().unwrap();
                            loop {
                                if guard.1 {
                                    return;
                                }
                                if let Some(job) = guard.0.pop_front() {
                                    break job;
                                }
                                guard = queue.cond.wait(guard).unwrap();
                            }
                        };
                        if !cache.contains(&job.key) {
                            let _ = cache.get_or_load(&job.key, || load(&job.path));
                        }
                    })
                    .expect("先読みスレッドを起動できません")
            })
            .collect();
        Prefetcher { queue, workers }
    }

    /// 先読みの予定を置き換える（前の予定のうち未着手のものは捨てる）
    pub fn schedule(&self, jobs: Vec<PrefetchJob>) {
        let mut guard = self.queue.jobs.lock().unwrap();
        guard.0 = jobs.into();
        drop(guard);
        self.queue.cond.notify_all();
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.queue.jobs.lock().unwrap().1 = true;
        self.queue.cond.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{make_preview, tests::write_dummy, Registry};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    fn dummy_preview() -> Preview {
        let enc = super::super::Encoded { bytes: vec![1, 2, 3], mime: "image/jpeg" };
        Preview { full: enc.clone(), thumb: enc, width: 1, height: 1 }
    }

    #[test]
    fn loads_once_and_evicts_oldest() {
        let cache = PreviewCache::new(2);
        let calls = AtomicUsize::new(0);
        let load = || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(dummy_preview())
        };
        cache.get_or_load(Path::new("a"), load).unwrap();
        cache.get_or_load(Path::new("a"), load).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cache.get_or_load(Path::new("b"), load).unwrap();
        cache.get_or_load(Path::new("a"), load).unwrap(); // a を新しくする
        cache.get_or_load(Path::new("c"), load).unwrap(); // b が追い出される
        assert!(cache.contains(Path::new("a")));
        assert!(!cache.contains(Path::new("b")));
        assert!(cache.contains(Path::new("c")));
    }

    #[test]
    fn concurrent_requests_share_one_decode() {
        let cache = Arc::new(PreviewCache::new(4));
        let calls = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let cache = cache.clone();
                let calls = calls.clone();
                thread::spawn(move || {
                    cache
                        .get_or_load(Path::new("same"), || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(50));
                            Ok(dummy_preview())
                        })
                        .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failures_are_cached_as_errors() {
        let cache = PreviewCache::new(2);
        assert!(cache.get_or_load(Path::new("x"), || Err("壊れています".into())).is_err());
        assert_eq!(cache.get_or_load(Path::new("x"), || Ok(dummy_preview())).unwrap_err(), "壊れています");
    }

    #[test]
    fn prefetcher_fills_the_cache_in_background() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| {
                let p = dir.path().join(format!("{i}.png"));
                write_dummy(&p, 64, 48);
                p
            })
            .collect();
        let cache = Arc::new(PreviewCache::new(8));
        let registry = Arc::new(Registry::builtin());
        let prefetcher = Prefetcher::start(2, cache.clone(), Arc::new(move |p: &Path| make_preview(&registry, p)));
        prefetcher.schedule(paths.iter().map(|p| PrefetchJob { key: p.clone(), path: p.clone() }).collect());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !paths.iter().all(|p| cache.contains(p)) {
            assert!(Instant::now() < deadline, "先読みが終わらない");
            thread::sleep(Duration::from_millis(10));
        }
        for p in &paths {
            // 先読み済み（または読み込み中）なので、ここで再びデコードされることはない
            cache.get_or_load(p, || panic!("先読みされていない: {}", p.display())).unwrap();
        }
        drop(prefetcher); // スレッドが止まること
    }
}
