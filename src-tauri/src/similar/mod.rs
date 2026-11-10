//! 類似画像の判定。計算だけを行い、ファイル操作はしない（削除は `fileops` が持つ）。

use crate::decoder::Registry;
use std::path::Path;

pub mod group;
pub mod hash;
pub mod names;
pub mod scan;

/// 既定の閾値（ハッシュの違うビット数がこれ以下なら類似とみなす）
pub const DEFAULT_MAX_DISTANCE: u32 = 6;
/// 画面からゆるめられる上限
pub const MAX_DISTANCE_LIMIT: u32 = 16;

/// 距離を類似度（%）にする
pub fn similarity_percent(distance: u32) -> u8 {
    let d = distance.min(hash::BITS) as f64;
    ((1.0 - d / hash::BITS as f64) * 100.0).round() as u8
}

/// 画像 1 枚を読み、判定用の情報（ハッシュ・解像度・ファイルサイズ）を作る
pub fn fingerprint(registry: &Registry, path: &Path) -> Result<group::Fingerprint, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let img = registry.decode(path)?;
    Ok(group::Fingerprint { hash: hash::dhash(&img), width: img.width(), height: img.height(), size })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::tests::write_dummy;

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
    fn fingerprint_fails_for_missing_or_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let broken = dir.path().join("broken.png");
        std::fs::write(&broken, "画像ではない").unwrap();
        let registry = Registry::builtin();
        assert!(fingerprint(&registry, &broken).is_err());
        assert!(fingerprint(&registry, &dir.path().join("missing.png")).is_err());
    }
}
