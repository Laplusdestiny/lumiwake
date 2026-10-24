//! 診断キャッシュのキー: 画像ファイルの中身の BLAKE3 ハッシュ。
//!
//! パスではなく中身で識別するので、振り分けで移動・改名しても、別フォルダから開き直しても同じキーになる。

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// ファイルの中身のハッシュを 16 進文字列で返す（全体を読み込まずに順次処理する）
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_gives_the_same_hash_regardless_of_path_and_name() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jpg");
        std::fs::create_dir(dir.path().join("moved")).unwrap();
        let b = dir.path().join("moved/renamed.png");
        std::fs::write(&a, b"image bytes").unwrap();
        std::fs::write(&b, b"image bytes").unwrap();
        let h = hash_file(&a).unwrap();
        assert_eq!(h, hash_file(&b).unwrap());
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_content_gives_a_different_hash() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, b"one").unwrap();
        std::fs::write(&b, b"two").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn large_files_hash_the_same_as_in_one_shot() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        assert_eq!(hash_file(&p).unwrap(), blake3::hash(&data).to_hex().to_string());
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(hash_file(&dir.path().join("none")).is_err());
    }
}
