//! スキャナ: 仕分け元フォルダを走査し、対象の画像を列挙する。
//!
//! - 振り分け先フォルダと削除フォルダは走査対象から除外する
//! - 画像形式は拡張子で判定し、デコーダがない形式は `unsupported` に分けて返す
//! - シンボリックリンクのフォルダはたどらない（循環を避ける）

use crate::format::Format;
use serde::Serialize;
use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub include_subdirs: bool,
    /// 走査しないフォルダ（振り分け先・削除フォルダ）
    pub exclude: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanResult {
    /// 表示・仕分けの対象になる画像（自然順）
    pub images: Vec<PathBuf>,
    /// 画像だがデコーダが用意されていないため読み込まなかったもの
    pub unsupported: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("仕分け元フォルダが見つかりません: {0}")]
    NotFound(PathBuf),
    #[error("仕分け元フォルダが振り分け先または削除フォルダと同じ（またはその中）です: {0}")]
    RootExcluded(PathBuf),
    #[error("フォルダを読み込めませんでした: {0}")]
    Io(#[from] io::Error),
}

/// フォルダを走査する。`is_supported` はその形式を表示できるかを返す。
pub fn scan(
    opts: &ScanOptions,
    is_supported: impl Fn(Format) -> bool,
) -> Result<ScanResult, ScanError> {
    let root =
        dunce::canonicalize(&opts.root).map_err(|_| ScanError::NotFound(opts.root.clone()))?;
    if !root.is_dir() {
        return Err(ScanError::NotFound(opts.root.clone()));
    }
    // 存在しない除外先は無視する（まだ作られていない振り分け先など）
    let exclude: Vec<PathBuf> = opts
        .exclude
        .iter()
        .filter_map(|p| dunce::canonicalize(p).ok())
        .collect();
    if exclude.iter().any(|ex| root.starts_with(ex)) {
        return Err(ScanError::RootExcluded(opts.root.clone()));
    }

    let mut result = ScanResult::default();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            // ルート自体が読めないのはエラー、サブフォルダは読めなければ飛ばす
            Err(e) if dir == root => return Err(e.into()),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if opts.include_subdirs && !is_excluded(&path, &exclude) {
                    stack.push(path);
                }
                continue;
            }
            // シンボリックリンクはファイルを指す場合のみ対象にする
            if file_type.is_symlink() && !path.is_file() {
                continue;
            }
            if is_internal_file(&path) {
                continue;
            }
            if let Some(format) = Format::from_path(&path) {
                if is_supported(format) {
                    result.images.push(path);
                } else {
                    result.unsupported.push(path);
                }
            }
        }
    }
    result.images.sort_by(|a, b| natural_cmp_path(a, b));
    result.unsupported.sort_by(|a, b| natural_cmp_path(a, b));
    Ok(result)
}

fn is_excluded(dir: &Path, exclude: &[PathBuf]) -> bool {
    match dunce::canonicalize(dir) {
        Ok(c) => exclude.iter().any(|ex| c.starts_with(ex)),
        // 解決できないフォルダ（壊れたリンクなど）は走査しない
        Err(_) => true,
    }
}

/// Lumiwake がファイル操作中に作る一時ファイル
pub fn is_internal_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.contains(".lumiwake-tmp"))
}

/// パスを自然順（IMG_2 < IMG_10）で比較する。大文字小文字は区別しない。
pub fn natural_cmp_path(a: &Path, b: &Path) -> Ordering {
    let a = a.to_string_lossy().to_lowercase();
    let b = b.to_string_lossy().to_lowercase();
    natural_cmp(&a, &b)
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_number(&mut ai);
                let nb = take_number(&mut bi);
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta
                    .len()
                    .cmp(&tb.len())
                    .then_with(|| ta.cmp(tb))
                    .then_with(|| na.len().cmp(&nb.len()));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied() {
        if !c.is_ascii_digit() {
            break;
        }
        s.push(c);
        it.next();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"dummy").unwrap();
    }

    fn names(paths: &[PathBuf], root: &Path) -> Vec<String> {
        let root = dunce::canonicalize(root).unwrap();
        paths
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    fn all_supported(_: Format) -> bool {
        true
    }

    #[test]
    fn lists_images_in_natural_order_without_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        for n in [
            "IMG_10.jpg",
            "IMG_2.JPG",
            "IMG_1.png",
            "memo.txt",
            "raw.cr2",
        ] {
            touch(&dir.path().join(n));
        }
        touch(&dir.path().join("sub/inner.jpg"));
        let r = scan(
            &ScanOptions {
                root: dir.path().into(),
                include_subdirs: false,
                exclude: vec![],
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(
            names(&r.images, dir.path()),
            ["IMG_1.png", "IMG_2.JPG", "IMG_10.jpg"]
        );
    }

    #[test]
    fn includes_subdirs_and_excludes_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("a.jpg"));
        touch(&root.join("sub/b.jpg"));
        touch(&root.join("sub/deep/c.jpg"));
        touch(&root.join("風景/sorted.jpg"));
        touch(&root.join("trash/deleted.jpg"));
        let r = scan(
            &ScanOptions {
                root: root.into(),
                include_subdirs: true,
                exclude: vec![
                    root.join("風景"),
                    root.join("trash"),
                    root.join("not-yet-created"),
                ],
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(
            names(&r.images, root),
            ["a.jpg", "sub/b.jpg", "sub/deep/c.jpg"]
        );
    }

    #[test]
    fn separates_formats_without_decoder() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.jpg"));
        touch(&dir.path().join("b.heic"));
        let r = scan(
            &ScanOptions {
                root: dir.path().into(),
                include_subdirs: false,
                exclude: vec![],
            },
            |f| f != Format::Heic,
        )
        .unwrap();
        assert_eq!(names(&r.images, dir.path()), ["a.jpg"]);
        assert_eq!(names(&r.unsupported, dir.path()), ["b.heic"]);
    }

    #[test]
    fn skips_internal_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.jpg"));
        touch(&dir.path().join(".a.jpg.lumiwake-tmp-1-0.jpg"));
        let r = scan(
            &ScanOptions {
                root: dir.path().into(),
                include_subdirs: false,
                exclude: vec![],
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, dir.path()), ["a.jpg"]);
    }

    #[test]
    fn rejects_root_inside_excluded_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dest/inner")).unwrap();
        let err = scan(
            &ScanOptions {
                root: dir.path().join("dest/inner"),
                include_subdirs: false,
                exclude: vec![dir.path().join("dest")],
            },
            all_supported,
        )
        .unwrap_err();
        assert!(matches!(err, ScanError::RootExcluded(_)));
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = scan(
            &ScanOptions {
                root: dir.path().join("nope"),
                include_subdirs: false,
                exclude: vec![],
            },
            all_supported,
        )
        .unwrap_err();
        assert!(matches!(err, ScanError::NotFound(_)));
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["b10", "b2", "a", "B1", "b02"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a", "B1", "b2", "b02", "b10"]);
    }
}
