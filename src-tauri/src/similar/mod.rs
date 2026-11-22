//! 類似画像の判定。計算だけを行い、ファイル操作はしない（削除は `fileops` が持つ）。

use crate::decoder::Registry;
use std::path::Path;
use store::PrintStore;

pub mod group;
pub mod hash;
pub mod names;
pub mod scan;
pub mod store;

/// 既定の閾値（ハッシュの違うビット数がこれ以下なら類似とみなす）
pub const DEFAULT_MAX_DISTANCE: u32 = 6;
/// 画面からゆるめられる上限
pub const MAX_DISTANCE_LIMIT: u32 = 16;

/// 距離を類似度（%）にする
pub fn similarity_percent(distance: u32) -> u8 {
    let d = distance.min(hash::BITS) as f64;
    ((1.0 - d / hash::BITS as f64) * 100.0).round() as u8
}

/// 判定を同時に走らせる数の上限。原寸でデコードするので、1 本あたり画像 1 枚分のメモリを使う
pub const MAX_SCAN_WORKERS: usize = 8;

/// CPU のコア数から、判定を同時にいくつ走らせるかを決める
pub fn scan_workers(cores: usize) -> usize {
    // 1 コアは画面と先読みのために空けておく（判定中も仕分けを続けられるように）
    cores.saturating_sub(1).clamp(1, MAX_SCAN_WORKERS)
}

/// 画像 1 枚を読み、判定用の情報（ハッシュ・解像度・ファイルサイズ）を作る
pub fn fingerprint(registry: &Registry, path: &Path) -> Result<group::Fingerprint, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let img = registry.decode(path)?;
    Ok(group::Fingerprint { hash: hash::dhash(&img), width: img.width(), height: img.height(), size })
}

/// 保存した記録があれば、デコードせずにそれを返す。なければデコードして記録する。
/// 更新日時を取れない画像は記録を使わず、保存もしない
pub fn fingerprint_cached(registry: &Registry, prints: &PrintStore, path: &Path) -> Result<group::Fingerprint, String> {
    let stamp = store::stamp(path).map_err(|e| e.to_string())?;
    let now = store::now_secs();
    if let Some(print) = stamp.and_then(|s| prints.lookup(path, s, now)) {
        return Ok(print);
    }
    let print = fingerprint(registry, path)?;
    if let Some(stamp) = stamp {
        prints.insert(path, stamp, print, now);
        if prints.unsaved() >= store::FLUSH_EVERY {
            if let Err(e) = prints.save(now) {
                eprintln!("類似判定の記録を保存できませんでした: {e}");
            }
        }
    }
    Ok(print)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::decoder::tests::write_dummy;
    use crate::similar::store::{self, PrintStore, FLUSH_EVERY};

    /// 中身だけを壊す（サイズと更新日時は元のまま）。これを読めれば、デコードせずに記録を使ったことになる
    pub(crate) fn corrupt_keeping_stamp(p: &Path) {
        let meta = std::fs::metadata(p).unwrap();
        let modified = meta.modified().unwrap();
        std::fs::write(p, vec![0u8; meta.len() as usize]).unwrap();
        std::fs::File::options().write(true).open(p).unwrap().set_modified(modified).unwrap();
    }

    fn set_modified_later(p: &Path) {
        let later = std::fs::metadata(p).unwrap().modified().unwrap() + std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(p).unwrap().set_modified(later).unwrap();
    }

    #[test]
    fn cached_fingerprint_skips_decoding_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        write_dummy(&p, 8, 6);
        let registry = Registry::builtin();
        let prints = PrintStore::new(dir.path().join("cache.json"));
        let first = fingerprint_cached(&registry, &prints, &p).unwrap();
        assert_eq!(first, fingerprint(&registry, &p).unwrap());
        corrupt_keeping_stamp(&p);
        assert!(fingerprint(&registry, &p).is_err(), "中身はもう画像ではない");
        assert_eq!(fingerprint_cached(&registry, &prints, &p).unwrap(), first, "記録を使う");
    }

    #[test]
    fn cached_fingerprint_rereads_files_that_changed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        write_dummy(&p, 8, 6);
        let registry = Registry::builtin();
        let prints = PrintStore::new(dir.path().join("cache.json"));
        fingerprint_cached(&registry, &prints, &p).unwrap();
        // 同じサイズのまま更新日時だけ変わった（編集された）: 読み直すので、壊れた中身は読めない
        corrupt_keeping_stamp(&p);
        set_modified_later(&p);
        assert!(fingerprint_cached(&registry, &prints, &p).is_err());
        // 別の画像に差し替えた: 新しい結果を返し、記録も置き換える
        write_dummy(&p, 16, 12);
        set_modified_later(&p);
        let replaced = fingerprint_cached(&registry, &prints, &p).unwrap();
        assert_eq!((replaced.width, replaced.height), (16, 12));
        let now = store::stamp(&p).unwrap().unwrap();
        assert_eq!(prints.lookup(&p, now, store::now_secs()), Some(replaced));
    }

    #[test]
    fn cached_fingerprint_reports_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let prints = PrintStore::new(dir.path().join("cache.json"));
        assert!(fingerprint_cached(&Registry::builtin(), &prints, &dir.path().join("missing.png")).is_err());
        assert_eq!(prints.unsaved(), 0);
    }

    #[test]
    fn cached_fingerprint_saves_every_200_new_records() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache").join("similar-cache.json");
        let prints = PrintStore::new(file.clone());
        let registry = Registry::builtin();
        for i in 0..FLUSH_EVERY {
            let p = dir.path().join(format!("{i}.png"));
            write_dummy(&p, 2, 2);
            fingerprint_cached(&registry, &prints, &p).unwrap();
            assert_eq!(file.exists(), i + 1 == FLUSH_EVERY, "{i} 枚目");
        }
        assert_eq!(prints.unsaved(), 0);
    }

    #[test]
    fn fingerprint_reads_hash_resolution_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
        write_dummy(&a, 8, 6);
        std::fs::copy(&a, &b).unwrap();
        let registry = Registry::builtin();
        let print = fingerprint(&registry, &a).unwrap();
        assert_eq!((print.width, print.height), (8, 6));
        assert_eq!(print.size, std::fs::metadata(&a).unwrap().len());
        assert_eq!(fingerprint(&registry, &b).unwrap(), print, "同じ中身なら同じ情報");
    }

    #[test]
    fn scan_workers_use_the_cores_but_leave_one_for_the_screen() {
        assert_eq!([1, 2, 3, 4, 8, 12, 64].map(scan_workers), [1, 1, 2, 3, 7, 8, 8]);
        assert_eq!(scan_workers(0), 1, "コア数がわからなくても 1 本は動かす");
    }

    #[test]
    fn fingerprint_fails_for_missing_or_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("broken.png");
        std::fs::write(&broken, "画像ではない").unwrap();
        let registry = Registry::builtin();
        assert!(fingerprint(&registry, &broken).is_err());
        assert!(fingerprint(&registry, &dir.path().join("missing.png")).is_err());
    }
}
