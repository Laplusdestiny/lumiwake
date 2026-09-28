//! ファイルシステム操作の抽象と、欠損・重複を残さない移動。

use std::io;
use std::path::{Path, PathBuf};

/// テストで障害を注入できるよう、ファイル操作を差し替え可能にしておく。
pub trait Fs: Send + Sync {
    /// 同一ボリューム内の移動。別ボリュームなら `ErrorKind::CrossesDevices` などで失敗する。
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// `to` を新規作成して内容をコピーし、ディスクへ書き出す。`to` が既にあれば失敗する。
    fn copy_new(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn len(&self, path: &Path) -> io::Result<u64>;
}

/// 実際のファイルシステム
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl Fs for RealFs {
    fn rename(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        todo!()
    }
    fn copy_new(&self, _from: &Path, _to: &Path) -> io::Result<u64> {
        todo!()
    }
    fn remove_file(&self, _path: &Path) -> io::Result<()> {
        todo!()
    }
    fn exists(&self, _path: &Path) -> bool {
        todo!()
    }
    fn len(&self, _path: &Path) -> io::Result<u64> {
        todo!()
    }
}

/// `from` を `to` へ移動する。`to` が既に存在する場合は上書きせず `AlreadyExists` で失敗する。
///
/// 別ボリューム間では、`to` と同じフォルダの一時ファイルへコピーしてサイズを検証し、
/// 一時ファイルを `to` へ確定してから元を削除する。どの段階で失敗しても、
/// 元ファイルだけが残る（コピー途中のファイルや一時ファイルは片付ける）。
pub fn safe_move(_fs: &dyn Fs, _from: &Path, _to: &Path) -> io::Result<()> {
    todo!()
}

/// `dir` の中で `file_name` と衝突しないパスを返す（`a.jpg` → `a (2).jpg` → `a (3).jpg` …）。
pub fn unique_path(_fs: &dyn Fs, _dir: &Path, _file_name: &str) -> PathBuf {
    todo!()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// 障害を注入できる Fs。別フォルダ間の rename を「別ボリューム」として失敗させる。
    #[derive(Default)]
    pub struct FaultyFs {
        pub cross_device: bool,
        pub fail_copy_midway: bool,
        pub truncate_copy: bool,
        pub fail_remove_source: AtomicBool,
    }

    impl Fs for FaultyFs {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.cross_device && from.parent() != to.parent() {
                return Err(io::Error::new(io::ErrorKind::CrossesDevices, "cross-device link"));
            }
            RealFs.rename(from, to)
        }
        fn copy_new(&self, from: &Path, to: &Path) -> io::Result<u64> {
            if self.fail_copy_midway {
                // 途中まで書いたところで失敗したことにする
                let data = fs::read(from)?;
                fs::write(to, &data[..data.len() / 2])?;
                return Err(io::Error::other("network error during copy"));
            }
            if self.truncate_copy {
                let data = fs::read(from)?;
                fs::write(to, &data[..data.len() - 1])?;
                return Ok(data.len() as u64 - 1);
            }
            RealFs.copy_new(from, to)
        }
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            if self.fail_remove_source.load(Ordering::SeqCst) && path.file_name().is_some_and(|n| n == "src.jpg") {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"));
            }
            RealFs.remove_file(path)
        }
        fn exists(&self, path: &Path) -> bool {
            RealFs.exists(path)
        }
        fn len(&self, path: &Path) -> io::Result<u64> {
            RealFs.len(path)
        }
    }

    /// ディレクトリ内のファイル名一覧（一時ファイルの取り残し検出用）
    pub fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        src_dir: PathBuf,
        dst_dir: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("src");
        let dst_dir = tmp.path().join("dst");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&dst_dir).unwrap();
        fs::write(src_dir.join("src.jpg"), b"original image bytes").unwrap();
        Fixture { _tmp: tmp, src_dir, dst_dir }
    }

    #[test]
    fn moves_within_same_volume() {
        let f = fixture();
        let to = f.dst_dir.join("src.jpg");
        safe_move(&RealFs, &f.src_dir.join("src.jpg"), &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"original image bytes");
        assert!(listing(&f.src_dir).is_empty());
    }

    #[test]
    fn never_overwrites_existing_destination() {
        let f = fixture();
        let to = f.dst_dir.join("src.jpg");
        fs::write(&to, b"existing").unwrap();
        let err = safe_move(&RealFs, &f.src_dir.join("src.jpg"), &to).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&to).unwrap(), b"existing");
        assert_eq!(fs::read(f.src_dir.join("src.jpg")).unwrap(), b"original image bytes");
    }

    #[test]
    fn cross_device_move_copies_then_removes_source() {
        let f = fixture();
        let fs_ = FaultyFs { cross_device: true, ..Default::default() };
        let to = f.dst_dir.join("src.jpg");
        safe_move(&fs_, &f.src_dir.join("src.jpg"), &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"original image bytes");
        assert!(listing(&f.src_dir).is_empty());
        assert_eq!(listing(&f.dst_dir), ["src.jpg"], "一時ファイルが残っていない");
    }

    #[test]
    fn failed_copy_leaves_only_the_source() {
        let f = fixture();
        let fs_ = FaultyFs { cross_device: true, fail_copy_midway: true, ..Default::default() };
        let err = safe_move(&fs_, &f.src_dir.join("src.jpg"), &f.dst_dir.join("src.jpg"));
        assert!(err.is_err());
        assert_eq!(fs::read(f.src_dir.join("src.jpg")).unwrap(), b"original image bytes");
        assert!(listing(&f.dst_dir).is_empty(), "コピー途中のファイルを片付ける");
    }

    #[test]
    fn size_mismatch_after_copy_is_rejected() {
        let f = fixture();
        let fs_ = FaultyFs { cross_device: true, truncate_copy: true, ..Default::default() };
        let err = safe_move(&fs_, &f.src_dir.join("src.jpg"), &f.dst_dir.join("src.jpg"));
        assert!(err.is_err());
        assert_eq!(fs::read(f.src_dir.join("src.jpg")).unwrap(), b"original image bytes");
        assert!(listing(&f.dst_dir).is_empty());
    }

    #[test]
    fn failing_to_remove_source_rolls_back_the_copy() {
        let f = fixture();
        let fs_ = FaultyFs { cross_device: true, ..Default::default() };
        fs_.fail_remove_source.store(true, Ordering::SeqCst);
        let err = safe_move(&fs_, &f.src_dir.join("src.jpg"), &f.dst_dir.join("src.jpg"));
        assert!(err.is_err());
        assert_eq!(fs::read(f.src_dir.join("src.jpg")).unwrap(), b"original image bytes");
        assert!(listing(&f.dst_dir).is_empty(), "重複を残さない");
    }

    #[test]
    fn copy_preserves_modified_time() {
        let f = fixture();
        let src = f.src_dir.join("src.jpg");
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        fs::File::options().write(true).open(&src).unwrap().set_modified(old).unwrap();
        let to = f.dst_dir.join("src.jpg");
        RealFs.copy_new(&src, &to).unwrap();
        assert_eq!(fs::metadata(&to).unwrap().modified().unwrap(), old);
    }

    #[test]
    fn copy_new_refuses_existing_file() {
        let f = fixture();
        let to = f.dst_dir.join("x.jpg");
        fs::write(&to, b"keep").unwrap();
        assert!(RealFs.copy_new(&f.src_dir.join("src.jpg"), &to).is_err());
        assert_eq!(fs::read(&to).unwrap(), b"keep");
    }

    #[test]
    fn unique_path_appends_counter() {
        let f = fixture();
        assert_eq!(unique_path(&RealFs, &f.dst_dir, "a.jpg"), f.dst_dir.join("a.jpg"));
        fs::write(f.dst_dir.join("a.jpg"), b"").unwrap();
        assert_eq!(unique_path(&RealFs, &f.dst_dir, "a.jpg"), f.dst_dir.join("a (2).jpg"));
        fs::write(f.dst_dir.join("a (2).jpg"), b"").unwrap();
        assert_eq!(unique_path(&RealFs, &f.dst_dir, "a.jpg"), f.dst_dir.join("a (3).jpg"));
        fs::write(f.dst_dir.join("noext"), b"").unwrap();
        assert_eq!(unique_path(&RealFs, &f.dst_dir, "noext"), f.dst_dir.join("noext (2)"));
    }
}
