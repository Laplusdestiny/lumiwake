//! ファイルシステム操作の抽象と、欠損・重複を残さない移動。

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn copy_new(&self, from: &Path, to: &Path) -> io::Result<u64> {
        let mut src = File::open(from)?;
        let meta = src.metadata()?;
        let mut dst = OpenOptions::new().write(true).create_new(true).open(to)?;
        let copied = io::copy(&mut src, &mut dst)?;
        // 撮影日順で並べる人もいるので更新日時を引き継ぐ（失敗しても致命的ではない）
        if let Ok(modified) = meta.modified() {
            let _ = dst.set_modified(modified);
        }
        dst.sync_all()?;
        drop(dst);
        let _ = std::fs::set_permissions(to, meta.permissions());
        Ok(copied)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn exists(&self, path: &Path) -> bool {
        // 壊れたシンボリックリンクも「ある」とみなして上書きしない
        std::fs::symlink_metadata(path).is_ok()
    }

    fn len(&self, path: &Path) -> io::Result<u64> {
        Ok(std::fs::metadata(path)?.len())
    }
}

/// rename が別ボリュームのために失敗したか
fn is_cross_device(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    // EXDEV (Unix) / ERROR_NOT_SAME_DEVICE (Windows)
    #[cfg(unix)]
    const CROSS_DEVICE: i32 = 18;
    #[cfg(windows)]
    const CROSS_DEVICE: i32 = 17;
    #[cfg(any(unix, windows))]
    if err.raw_os_error() == Some(CROSS_DEVICE) {
        return true;
    }
    false
}

/// `from` を `to` へ移動する。`to` が既に存在する場合は上書きせず `AlreadyExists` で失敗する。
///
/// 別ボリューム間では、`to` と同じフォルダの一時ファイルへコピーしてサイズを検証し、
/// 一時ファイルを `to` へ確定してから元を削除する。どの段階で失敗しても、
/// 元ファイルだけが残る（コピー途中のファイルや一時ファイルは片付ける）。
pub fn safe_move(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<()> {
    if fs.exists(to) {
        return Err(already_exists(to));
    }
    match fs.rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => copy_then_remove(fs, from, to),
        Err(e) => Err(e),
    }
}

fn copy_then_remove(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<()> {
    let tmp = temp_path_for(to);
    let cleanup = |e: io::Error| {
        let _ = fs.remove_file(&tmp);
        e
    };

    let expected = fs.len(from)?;
    fs.copy_new(from, &tmp).map_err(cleanup)?;
    let actual = fs.len(&tmp).map_err(cleanup)?;
    if actual != expected {
        return Err(cleanup(io::Error::other(format!(
            "コピー後のサイズが一致しません（元 {expected} バイト、コピー {actual} バイト）"
        ))));
    }
    // コピー中に同名ファイルが作られていたら上書きしない
    if fs.exists(to) {
        return Err(cleanup(already_exists(to)));
    }
    fs.rename(&tmp, to).map_err(cleanup)?;

    if let Err(e) = fs.remove_file(from) {
        // 元を消せなかったので、コピーを取り消して重複を残さない
        return match fs.remove_file(to) {
            Ok(()) => Err(e),
            Err(e2) => Err(io::Error::other(format!(
                "元ファイルを削除できず（{e}）、コピーの取り消しにも失敗しました（{e2}）。{} と {} の両方が残っています",
                from.display(),
                to.display()
            ))),
        };
    }
    Ok(())
}

fn already_exists(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, format!("同名のファイルがあります: {}", path.display()))
}

/// コピー先と同じフォルダに作る一時ファイル名（スキャナはこの名前を無視する）
fn temp_path_for(to: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    to.with_file_name(format!(".{name}.lumiwake-tmp-{}-{n}", std::process::id()))
}

/// `dir` の中で `file_name` と衝突しないパスを返す（`a.jpg` → `a (2).jpg` → `a (3).jpg` …）。
pub fn unique_path(fs: &dyn Fs, dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !fs.exists(&candidate) {
        return candidate;
    }
    let name = Path::new(file_name);
    let stem = name.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = name.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2u64..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !fs.exists(p)).expect("連番は尽きない")
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
            // 仕分け元フォルダ（src）にある元ファイルだけを削除できないことにする
            let in_src_dir = path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "src");
            if self.fail_remove_source.load(Ordering::SeqCst) && in_src_dir {
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
        let mut v: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
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
