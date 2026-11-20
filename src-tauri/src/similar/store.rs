//! 類似判定の結果をファイルに保存し、アプリを終了してもまた使えるようにする。
//!
//! 記録は画像の今ある場所のパスで持ち、更新日時とサイズが一致したときだけ使う
//! （中身が変わった画像に古い結果を使わない）。保存ファイルは最初に必要になったときに 1 回だけ読む。

use super::group::Fingerprint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// 保存ファイルの形式。ハッシュの計算方法を変えたら上げる（古い記録は全部捨てる）
pub const STORE_VERSION: u32 = 1;
/// 最後に使ってからこれだけたった記録は、書き出すときに捨てる
pub const MAX_AGE_SECS: u64 = 90 * 24 * 60 * 60;
/// 記録の上限。超えたら最後に使った日時の古いものから捨てる
pub const MAX_RECORDS: usize = 200_000;
/// 判定の途中でも、新しい記録がこれだけたまったら書き出す（異常終了しても大半が残るように）
pub const FLUSH_EVERY: usize = 200;

/// 照合に使うファイルの状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// 更新日時（UNIX エポックからのナノ秒）
    pub modified_ns: u64,
    pub size: u64,
}

/// 今のファイルの状態。更新日時を取れない（エポックより前など）ときは `Ok(None)`
pub fn stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_nanos()).ok());
    Ok(modified.map(|modified_ns| FileStamp { modified_ns, size: meta.len() }))
}

/// 今の時刻（UNIX 秒）
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    stamp: FileStamp,
    hash: u64,
    width: u32,
    height: u32,
    /// 最後に使った日時（UNIX 秒）
    last_used: u64,
}

/// 保存ファイルの 1 件
#[derive(Serialize, Deserialize)]
struct Record {
    path: String,
    modified_ns: u64,
    size: u64,
    hash: u64,
    width: u32,
    height: u32,
    last_used: u64,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    records: Vec<Record>,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    entries: HashMap<PathBuf, Entry>,
    /// 前回の書き出しから新しく判定して加えた件数
    unsaved: usize,
    /// 前回の書き出しから何か変わったか（最後に使った日時の更新・移動先への複製を含む）
    changed: bool,
}

pub struct PrintStore {
    file: PathBuf,
    limit: usize,
    inner: Mutex<Inner>,
    /// 書き出しを 1 本ずつにする（同じ一時ファイルを取り合わず、古い写しが後から上書きしないように）
    saving: Mutex<()>,
}

impl PrintStore {
    /// 保存ファイルの場所を決めるだけで、まだ読まない（起動を遅くしない）
    pub fn new(file: PathBuf) -> Self {
        Self::with_limit(file, MAX_RECORDS)
    }

    fn with_limit(file: PathBuf, limit: usize) -> Self {
        PrintStore { file, limit, inner: Mutex::new(Inner::default()), saving: Mutex::new(()) }
    }

    /// 読み込み済みの記録。初回だけファイルを読む（読む間はロックを持ち、ほかのスレッドを待たせる）
    fn lock(&self) -> MutexGuard<'_, Inner> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.loaded {
            inner.entries = read_file(&self.file);
            inner.loaded = true;
        }
        inner
    }

    /// パス・更新日時・サイズが一致する記録があれば、その判定結果を返す
    pub fn lookup(&self, path: &Path, stamp: FileStamp, now: u64) -> Option<Fingerprint> {
        let mut inner = self.lock();
        let entry = inner.entries.get_mut(path).filter(|e| e.stamp == stamp)?;
        let print = Fingerprint { hash: entry.hash, width: entry.width, height: entry.height, size: stamp.size };
        if entry.last_used < now {
            entry.last_used = now;
            inner.changed = true;
        }
        Some(print)
    }

    /// 新しく判定した結果を記録する。パスを UTF-8 で表せない画像は保存できないので記録しない
    pub fn insert(&self, path: &Path, stamp: FileStamp, print: Fingerprint, now: u64) {
        if path.to_str().is_none() {
            return;
        }
        let mut inner = self.lock();
        let entry = Entry { stamp, hash: print.hash, width: print.width, height: print.height, last_used: now };
        inner.entries.insert(path.to_path_buf(), entry);
        inner.unsaved += 1;
        inner.changed = true;
    }

    /// 移動した画像の記録を、移動先のパスでも使えるようにする（移動元の記録は残す）
    pub fn copy_record(&self, from: &Path, to: &Path, now: u64) {
        if to.to_str().is_none() {
            return;
        }
        let mut inner = self.lock();
        if let Some(entry) = inner.entries.get(from).copied() {
            inner.entries.insert(to.to_path_buf(), Entry { last_used: now, ..entry });
            inner.changed = true;
        }
    }

    /// 前回の書き出しから新しく判定して加えた件数
    pub fn unsaved(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).unsaved
    }

    /// 古い記録を捨てて書き出す。前回から変わっていなければ書かない。
    /// ファイルに書く間は記録のロックを持たない（判定を止めない）。書き出し同士は順番に行い、
    /// 後から始めた書き出しの写しが必ず後に書かれる
    pub fn save(&self, now: u64) -> io::Result<()> {
        let _saving = self.saving.lock().unwrap_or_else(|e| e.into_inner());
        let (records, unsaved) = {
            let mut inner = self.lock();
            if !inner.changed {
                return Ok(());
            }
            inner.entries.retain(|_, e| now.saturating_sub(e.last_used) <= MAX_AGE_SECS);
            if inner.entries.len() > self.limit {
                let mut by_age: Vec<(u64, PathBuf)> =
                    inner.entries.iter().map(|(p, e)| (e.last_used, p.clone())).collect();
                by_age.sort();
                let excess = inner.entries.len() - self.limit;
                for (_, p) in by_age.into_iter().take(excess) {
                    inner.entries.remove(&p);
                }
            }
            let mut records: Vec<Record> = inner
                .entries
                .iter()
                .filter_map(|(p, e)| {
                    Some(Record {
                        path: p.to_str()?.to_owned(),
                        modified_ns: e.stamp.modified_ns,
                        size: e.stamp.size,
                        hash: e.hash,
                        width: e.width,
                        height: e.height,
                        last_used: e.last_used,
                    })
                })
                .collect();
            records.sort_by(|a, b| a.path.cmp(&b.path));
            let unsaved = inner.unsaved;
            inner.unsaved = 0;
            inner.changed = false;
            (records, unsaved)
        };
        let result = write_file(&self.file, &StoreFile { version: STORE_VERSION, records });
        if result.is_err() {
            // 書けなかったので、次の機会にもう一度書く
            let mut inner = self.lock();
            inner.unsaved += unsaved;
            inner.changed = true;
        }
        result
    }
}

/// 保存ファイルを読む。ない・壊れている・形式が違うときは空として扱う
fn read_file(file: &Path) -> HashMap<PathBuf, Entry> {
    let Ok(text) = std::fs::read_to_string(file) else { return HashMap::new() };
    let Ok(stored) = serde_json::from_str::<StoreFile>(&text) else { return HashMap::new() };
    if stored.version != STORE_VERSION {
        return HashMap::new();
    }
    stored
        .records
        .into_iter()
        .map(|r| {
            let entry = Entry {
                stamp: FileStamp { modified_ns: r.modified_ns, size: r.size },
                hash: r.hash,
                width: r.width,
                height: r.height,
                last_used: r.last_used,
            };
            (PathBuf::from(r.path), entry)
        })
        .collect()
}

/// 一時ファイルに書いてから置き換える（途中で落ちても壊れたファイルを残さない）
fn write_file(file: &Path, stored: &StoreFile) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    let bytes = serde_json::to_vec(stored).map_err(io::Error::other)?;
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const DAY: u64 = 24 * 60 * 60;

    fn st(ns: u64, size: u64) -> FileStamp {
        FileStamp { modified_ns: ns, size }
    }

    fn fp(hash: u64, size: u64) -> Fingerprint {
        Fingerprint { hash, width: 4, height: 3, size }
    }

    fn file(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("cache").join("similar-cache.json")
    }

    #[test]
    fn uses_a_record_only_when_path_time_and_size_match() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        assert_eq!(store.lookup(Path::new("/a"), st(1, 10), 0), Some(fp(7, 10)));
        assert_eq!(store.lookup(Path::new("/a"), st(2, 10), 0), None, "更新日時が違う");
        assert_eq!(store.lookup(Path::new("/a"), st(1, 11), 0), None, "サイズが違う");
        assert_eq!(store.lookup(Path::new("/b"), st(1, 10), 0), None, "別の画像");
    }

    #[test]
    fn survives_a_new_store_on_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        store.save(0).unwrap();
        // アプリを終了して、また起動した
        let again = PrintStore::new(file(&dir));
        assert_eq!(again.lookup(Path::new("/a"), st(1, 10), 0), Some(fp(7, 10)));
    }

    #[test]
    fn broken_old_or_missing_files_are_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        assert_eq!(PrintStore::new(path.clone()).lookup(Path::new("/a"), st(1, 10), 0), None, "ファイルがない");

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "壊れた").unwrap();
        assert_eq!(PrintStore::new(path.clone()).lookup(Path::new("/a"), st(1, 10), 0), None, "壊れている");

        let store = PrintStore::new(path.clone());
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        store.save(0).unwrap();
        let old = std::fs::read_to_string(&path)
            .unwrap()
            .replace(&format!("\"version\":{STORE_VERSION}"), &format!("\"version\":{}", STORE_VERSION + 1));
        std::fs::write(&path, old).unwrap();
        assert_eq!(PrintStore::new(path.clone()).lookup(Path::new("/a"), st(1, 10), 0), None, "形式が違う");

        // 次の書き出しで作り直す
        let store = PrintStore::new(path.clone());
        store.insert(Path::new("/b"), st(2, 20), fp(8, 20), 0);
        store.save(0).unwrap();
        assert_eq!(PrintStore::new(path).lookup(Path::new("/b"), st(2, 20), 0), Some(fp(8, 20)));
    }

    #[test]
    fn save_skips_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(&dir);
        let store = PrintStore::new(path.clone());
        store.save(0).unwrap();
        assert!(!path.exists(), "記録がなければ書かない");
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        store.save(0).unwrap();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        store.save(0).unwrap();
        assert!(!path.exists(), "前回から変わっていなければ書かない");
    }

    #[test]
    fn save_writes_through_a_temporary_file_and_fails_quietly_when_the_folder_is_unwritable() {
        let dir = tempfile::tempdir().unwrap();
        // 保存先の親がファイルなので、書き出せない
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "").unwrap();
        let store = PrintStore::new(blocker.join("similar-cache.json"));
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        assert!(store.save(0).is_err());
        assert_eq!(store.lookup(Path::new("/a"), st(1, 10), 0), Some(fp(7, 10)), "使い続けられる");

        let path = file(&dir);
        let store = PrintStore::new(path.clone());
        store.insert(Path::new("/a"), st(1, 10), fp(7, 10), 0);
        store.save(0).unwrap();
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["similar-cache.json"], "一時ファイルを残さない");
    }

    #[test]
    fn drops_records_unused_for_90_days() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        store.insert(Path::new("/old"), st(1, 10), fp(1, 10), 0);
        store.insert(Path::new("/used"), st(1, 10), fp(2, 10), 0);
        store.insert(Path::new("/new"), st(1, 10), fp(3, 10), 89 * DAY);
        assert!(store.lookup(Path::new("/used"), st(1, 10), 89 * DAY).is_some());
        store.save(90 * DAY + 1).unwrap();
        let again = PrintStore::new(file(&dir));
        assert_eq!(again.lookup(Path::new("/old"), st(1, 10), 0), None);
        assert!(again.lookup(Path::new("/used"), st(1, 10), 0).is_some(), "使った記録は残る");
        assert!(again.lookup(Path::new("/new"), st(1, 10), 0).is_some());
    }

    #[test]
    fn keeps_the_newest_when_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::with_limit(file(&dir), 2);
        for (i, name) in ["/1", "/2", "/3"].iter().enumerate() {
            store.insert(Path::new(name), st(1, 10), fp(i as u64, 10), i as u64 + 1);
        }
        store.save(3).unwrap();
        let again = PrintStore::new(file(&dir));
        assert_eq!(again.lookup(Path::new("/1"), st(1, 10), 3), None);
        assert!(again.lookup(Path::new("/2"), st(1, 10), 3).is_some());
        assert!(again.lookup(Path::new("/3"), st(1, 10), 3).is_some());
        assert_eq!(MAX_RECORDS, 200_000);
    }

    #[test]
    fn copies_a_record_to_the_moved_path_and_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        store.insert(Path::new("/src/a"), st(1, 10), fp(7, 10), 0);
        store.copy_record(Path::new("/src/a"), Path::new("/dst/a"), 0);
        assert_eq!(store.lookup(Path::new("/dst/a"), st(1, 10), 0), Some(fp(7, 10)));
        assert_eq!(store.lookup(Path::new("/src/a"), st(1, 10), 0), Some(fp(7, 10)), "取り消しで戻ったときに使う");
        store.copy_record(Path::new("/src/x"), Path::new("/dst/x"), 0);
        assert_eq!(store.lookup(Path::new("/dst/x"), st(1, 10), 0), None);
    }

    #[test]
    fn counts_unsaved_records_until_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        for name in ["/1", "/2", "/3"] {
            store.insert(Path::new(name), st(1, 10), fp(1, 10), 0);
        }
        assert_eq!(store.unsaved(), 3);
        store.save(0).unwrap();
        assert_eq!(store.unsaved(), 0);
        store.lookup(Path::new("/1"), st(1, 10), 0);
        assert_eq!(store.unsaved(), 0, "引いただけでは増えない");
        assert_eq!(FLUSH_EVERY, 200);
    }

    #[test]
    fn first_lookups_from_many_threads_all_see_the_saved_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = PrintStore::new(file(&dir));
        for i in 0..10 {
            store.insert(Path::new(&format!("/{i}")), st(1, 10), fp(i, 10), 0);
        }
        store.save(0).unwrap();
        let again = Arc::new(PrintStore::new(file(&dir)));
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let again = again.clone();
                std::thread::spawn(move || again.lookup(Path::new(&format!("/{t}")), st(1, 10), 0))
            })
            .collect();
        for (t, h) in handles.into_iter().enumerate() {
            assert_eq!(h.join().unwrap(), Some(fp(t as u64, 10)));
        }
    }

    #[test]
    fn concurrent_saves_never_leave_an_older_snapshot_behind() {
        // 判定の終わりの書き出し（ワーカー）と終了時の書き出しが重なっても、新しいほうの内容が残る
        for round in 0..200 {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(PrintStore::new(file(&dir)));
            let handles: Vec<_> = ["/a", "/b", "/c", "/d"]
                .into_iter()
                .map(|name| {
                    let store = store.clone();
                    std::thread::spawn(move || {
                        store.insert(Path::new(name), st(1, 10), fp(1, 10), 0);
                        store.save(0)
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap().unwrap_or_else(|e| panic!("{round} 回目: 書き出しに失敗 {e}"));
            }
            let again = PrintStore::new(file(&dir));
            for name in ["/a", "/b", "/c", "/d"] {
                assert!(again.lookup(Path::new(name), st(1, 10), 0).is_some(), "{round} 回目: {name} が消えた");
            }
        }
    }

    #[test]
    fn stamp_reads_time_and_size_of_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"12345").unwrap();
        let s = stamp(&p).unwrap().unwrap();
        assert_eq!(s.size, 5);
        assert!(s.modified_ns > 0);
        assert!(stamp(&dir.path().join("missing")).is_err());
        assert_eq!(MAX_AGE_SECS, 90 * DAY);
    }
}
