//! スキャナ: 仕分け元フォルダを走査し、対象の画像を列挙する。
//!
//! - 振り分け先フォルダと削除フォルダは走査対象から除外する
//! - 画像形式は拡張子で判定し、デコーダがない形式は `unsupported` に分けて返す
//! - シンボリックリンクのフォルダはたどらない（循環を避ける）
//! - 並び順は設定（ファイル名・更新日時・撮影日時・サイズ）に従う

use crate::config::SortOrder;
use crate::format::Format;
use serde::Serialize;
use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub include_subdirs: bool,
    /// 走査しないフォルダ（振り分け先・削除フォルダ）
    pub exclude: Vec<PathBuf>,
    pub order: SortOrder,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanResult {
    /// 表示・仕分けの対象になる画像（`order` の順）
    pub images: Vec<PathBuf>,
    /// 画像だがデコーダが用意されていないため読み込まなかったもの
    pub unsupported: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("仕分け元フォルダが見つかりません: {0}")]
    NotFound(PathBuf),
    #[error("フォルダを読み込めませんでした: {0}")]
    Io(#[from] io::Error),
}

/// フォルダを走査する。`is_supported` はその形式を表示できるかを返す。
pub fn scan(opts: &ScanOptions, is_supported: impl Fn(Format) -> bool) -> Result<ScanResult, ScanError> {
    let root = dunce::canonicalize(&opts.root).map_err(|_| ScanError::NotFound(opts.root.clone()))?;
    if !root.is_dir() {
        return Err(ScanError::NotFound(opts.root.clone()));
    }
    // 存在しない除外先は無視する（まだ作られていない振り分け先など）
    // 仕分け元そのものを含む除外先（仕分け元が振り分け先の中にある場合など）は除外に使わない。
    // 仕分け元の中にある振り分け先・削除フォルダだけを走査から外す
    let exclude: Vec<PathBuf> =
        opts.exclude.iter().filter_map(|p| dunce::canonicalize(p).ok()).filter(|ex| !root.starts_with(ex)).collect();

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
    sort_paths(&mut result.images, opts.order);
    result.unsupported.sort_by(|a, b| natural_cmp_path(a, b));
    Ok(result)
}

/// 並べ替えに使う値。読めなかった値は `None` で、昇順・降順どちらでも後ろに回す
#[derive(Debug)]
enum SortKey {
    Name,
    Modified(Option<SystemTime>),
    /// 撮影日時（年, 月, 日, 時, 分, 秒）。ない画像は更新日時で並べる
    Taken(Option<(u16, u8, u8, u8, u8, u8)>, Option<SystemTime>),
    Size(Option<u64>),
}

fn sort_key(path: &Path, order: SortOrder) -> SortKey {
    let modified = || std::fs::metadata(path).and_then(|m| m.modified()).ok();
    match order {
        SortOrder::NameAsc | SortOrder::NameDesc => SortKey::Name,
        SortOrder::ModifiedAsc | SortOrder::ModifiedDesc => SortKey::Modified(modified()),
        SortOrder::TakenAsc | SortOrder::TakenDesc => match crate::decoder::taken_datetime(path) {
            Some(d) => SortKey::Taken(Some((d.year, d.month, d.day, d.hour, d.minute, d.second)), None),
            None => SortKey::Taken(None, modified()),
        },
        SortOrder::SizeAsc | SortOrder::SizeDesc => SortKey::Size(std::fs::metadata(path).map(|m| m.len()).ok()),
    }
}

/// 値を読めなかったものを後ろにしつつ、`desc` なら逆順で比べる
fn cmp_option<T: Ord>(a: &Option<T>, b: &Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) if desc => b.cmp(a),
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn cmp_key(a: &SortKey, b: &SortKey, desc: bool) -> Ordering {
    match (a, b) {
        (SortKey::Modified(a), SortKey::Modified(b)) => cmp_option(a, b, desc),
        (SortKey::Taken(ta, ma), SortKey::Taken(tb, mb)) => {
            cmp_option(ta, tb, desc).then_with(|| cmp_option(ma, mb, desc))
        }
        (SortKey::Size(a), SortKey::Size(b)) => cmp_option(a, b, desc),
        _ => Ordering::Equal,
    }
}

/// 画像を `order` の順に並べる。値が同じものはファイル名の自然順にする
pub fn sort_paths(paths: &mut Vec<PathBuf>, order: SortOrder) {
    match order {
        SortOrder::NameAsc => return paths.sort_by(|a, b| natural_cmp_path(a, b)),
        SortOrder::NameDesc => return paths.sort_by(|a, b| natural_cmp_path(b, a)),
        _ => {}
    }
    let desc = matches!(order, SortOrder::ModifiedDesc | SortOrder::TakenDesc | SortOrder::SizeDesc);
    let keys = sort_keys(paths, order);
    let mut keyed: Vec<(SortKey, PathBuf)> = keys.into_iter().zip(paths.drain(..)).collect();
    keyed.sort_by(|(ka, pa), (kb, pb)| cmp_key(ka, kb, desc).then_with(|| natural_cmp_path(pa, pb)));
    paths.extend(keyed.into_iter().map(|(_, p)| p));
}

/// ファイルの情報を読むのは SMB 上だと遅いので、いくつかのスレッドで分けて読む
fn sort_keys(paths: &[PathBuf], order: SortOrder) -> Vec<SortKey> {
    let workers = std::thread::available_parallelism().map(|n| n.get().clamp(1, 8)).unwrap_or(4);
    let chunk = paths.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().map(|p| sort_key(p, order)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("並べ替えの値の読み込みで panic しない")).collect()
    })
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
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(".lumiwake-tmp"))
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
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)).then_with(|| na.len().cmp(&nb.len()));
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
        paths.iter().map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    fn all_supported(_: Format) -> bool {
        true
    }

    #[test]
    fn lists_images_in_natural_order_without_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["IMG_10.jpg", "IMG_2.JPG", "IMG_1.png", "memo.txt", "raw.cr2"] {
            touch(&dir.path().join(n));
        }
        touch(&dir.path().join("sub/inner.jpg"));
        let r = scan(
            &ScanOptions {
                root: dir.path().into(),
                include_subdirs: false,
                exclude: vec![],
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, dir.path()), ["IMG_1.png", "IMG_2.JPG", "IMG_10.jpg"]);
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
                exclude: vec![root.join("風景"), root.join("trash"), root.join("not-yet-created")],
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, root), ["a.jpg", "sub/b.jpg", "sub/deep/c.jpg"]);
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
                order: SortOrder::NameAsc,
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
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, dir.path()), ["a.jpg"]);
    }

    #[test]
    fn root_inside_a_target_folder_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("dest/inner");
        touch(&root.join("a.jpg"));
        touch(&root.join("sub/b.jpg"));
        touch(&root.join("other-target/c.jpg"));
        let r = scan(
            &ScanOptions {
                root: root.clone(),
                include_subdirs: true,
                // dest は仕分け元を含むので除外に使わない。仕分け元の中の other-target は除外する
                exclude: vec![dir.path().join("dest"), root.join("other-target")],
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, &root), ["a.jpg", "sub/b.jpg"]);
    }

    #[test]
    fn root_equal_to_a_target_folder_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.jpg"));
        let r = scan(
            &ScanOptions {
                root: dir.path().into(),
                include_subdirs: false,
                exclude: vec![dir.path().into()],
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap();
        assert_eq!(names(&r.images, dir.path()), ["a.jpg"]);
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = scan(
            &ScanOptions {
                root: dir.path().join("nope"),
                include_subdirs: false,
                exclude: vec![],
                order: SortOrder::NameAsc,
            },
            all_supported,
        )
        .unwrap_err();
        assert!(matches!(err, ScanError::NotFound(_)));
    }

    /// 更新日時を `secs` 秒（UNIX エポックから）にしたダミーファイルを作る
    fn touch_at(path: &Path, bytes: usize, secs: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; bytes]).unwrap();
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    /// EXIF の撮影日時だけを持つ JPEG（画像データはない）
    fn jpeg_taken_at(path: &Path, datetime: &str, modified: u64) {
        let field = exif::Field {
            tag: exif::Tag::DateTimeOriginal,
            ifd_num: exif::In::PRIMARY,
            value: exif::Value::Ascii(vec![datetime.as_bytes().to_vec()]),
        };
        let mut writer = exif::experimental::Writer::new();
        writer.push_field(&field);
        let mut tiff = std::io::Cursor::new(Vec::new());
        writer.write(&mut tiff, false).unwrap();
        let tiff = tiff.into_inner();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        fs::write(path, jpeg).unwrap();
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified);
        fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    fn sorted(dir: &Path, order: SortOrder) -> Vec<String> {
        let r = scan(&ScanOptions { root: dir.into(), include_subdirs: false, exclude: vec![], order }, all_supported)
            .unwrap();
        names(&r.images, dir)
    }

    #[test]
    fn sorts_by_name_descending() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["IMG_10.jpg", "IMG_2.jpg", "IMG_1.jpg"] {
            touch(&dir.path().join(n));
        }
        assert_eq!(sorted(dir.path(), SortOrder::NameDesc), ["IMG_10.jpg", "IMG_2.jpg", "IMG_1.jpg"]);
    }

    #[test]
    fn sorts_by_modified_time() {
        let dir = tempfile::tempdir().unwrap();
        touch_at(&dir.path().join("a.jpg"), 1, 3_000);
        touch_at(&dir.path().join("b.jpg"), 1, 1_000);
        touch_at(&dir.path().join("c.jpg"), 1, 2_000);
        // 同じ日時はファイル名順
        touch_at(&dir.path().join("d.jpg"), 1, 2_000);
        assert_eq!(sorted(dir.path(), SortOrder::ModifiedAsc), ["b.jpg", "c.jpg", "d.jpg", "a.jpg"]);
        assert_eq!(sorted(dir.path(), SortOrder::ModifiedDesc), ["a.jpg", "c.jpg", "d.jpg", "b.jpg"]);
    }

    #[test]
    fn sorts_by_size() {
        let dir = tempfile::tempdir().unwrap();
        touch_at(&dir.path().join("big.jpg"), 300, 0);
        touch_at(&dir.path().join("small.jpg"), 10, 0);
        touch_at(&dir.path().join("mid.jpg"), 100, 0);
        assert_eq!(sorted(dir.path(), SortOrder::SizeAsc), ["small.jpg", "mid.jpg", "big.jpg"]);
        assert_eq!(sorted(dir.path(), SortOrder::SizeDesc), ["big.jpg", "mid.jpg", "small.jpg"]);
    }

    #[test]
    fn sorts_by_taken_time_and_puts_images_without_exif_last() {
        let dir = tempfile::tempdir().unwrap();
        // 撮影日時と更新日時の順番をわざと逆にしておく
        jpeg_taken_at(&dir.path().join("a.jpg"), "2024:05:01 10:00:00", 1_000);
        jpeg_taken_at(&dir.path().join("b.jpg"), "2023:12:31 23:59:59", 3_000);
        jpeg_taken_at(&dir.path().join("c.jpg"), "2024:05:01 10:00:01", 2_000);
        touch_at(&dir.path().join("no-exif-old.jpg"), 1, 500);
        touch_at(&dir.path().join("no-exif-new.jpg"), 1, 5_000);
        assert_eq!(
            sorted(dir.path(), SortOrder::TakenAsc),
            ["b.jpg", "a.jpg", "c.jpg", "no-exif-old.jpg", "no-exif-new.jpg"]
        );
        assert_eq!(
            sorted(dir.path(), SortOrder::TakenDesc),
            ["c.jpg", "a.jpg", "b.jpg", "no-exif-new.jpg", "no-exif-old.jpg"]
        );
    }

    #[test]
    fn unreadable_files_go_last() {
        let dir = tempfile::tempdir().unwrap();
        touch_at(&dir.path().join("a.jpg"), 5, 0);
        touch_at(&dir.path().join("b.jpg"), 1, 0);
        let mut paths = vec![dir.path().join("gone.jpg"), dir.path().join("a.jpg"), dir.path().join("b.jpg")];
        sort_paths(&mut paths, SortOrder::SizeDesc);
        assert_eq!(paths, [dir.path().join("a.jpg"), dir.path().join("b.jpg"), dir.path().join("gone.jpg")]);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["b10", "b2", "a", "B1", "b02"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a", "B1", "b2", "b02", "b10"]);
    }
}
