//! 設定の振り分け先から、診断に渡す選択肢を作る。

use super::Choice;
use crate::config::ai::AiBackend;
use crate::config::Target;
use std::path::{Path, PathBuf};

/// 振り分け先の識別子。キャッシュ上のキーになるので、表記ゆれ（末尾の区切りなど）をそろえたフルパスにする。
/// 実在すれば正規化したパス、無ければ（SMB が切れているときなど）与えられたパスをそのまま使う。
/// フォルダを改名・移動した場合は別の振り分け先として扱う。
pub fn target_id(path: &Path) -> String {
    let p = dunce::canonicalize(path).unwrap_or_else(|_| path.components().collect::<PathBuf>());
    p.to_string_lossy().into_owned()
}

/// 画像を外部へ送るバックエンドか
pub fn sends_images(backend: AiBackend) -> bool {
    backend == AiBackend::Systemone
}

/// 診断に渡す選択肢。外部送信するバックエンドには、送信除外にしたフォルダを含めない
/// （フォルダ名・説明文・画像のいずれも外へ出さない）。
pub fn build_choices(targets: &[Target], backend: AiBackend) -> Vec<Choice> {
    targets
        .iter()
        .filter(|t| !(sends_images(backend) && t.exclude_external))
        .map(|t| Choice { id: target_id(&t.path), label: t.display_name(), description: t.description.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(key: &str, name: &str, path: &Path, desc: &str, exclude: bool) -> Target {
        Target {
            key: key.into(),
            name: name.into(),
            path: path.to_path_buf(),
            description: desc.into(),
            exclude_external: exclude,
        }
    }

    #[test]
    fn id_ignores_trailing_separators_and_dot_segments() {
        let dir = tempfile::tempdir().unwrap();
        let a = target_id(dir.path());
        assert_eq!(a, target_id(&dir.path().join(".")));
        assert_eq!(a, target_id(&PathBuf::from(format!("{}/", dir.path().display()))));
    }

    #[test]
    fn id_of_a_missing_folder_is_the_given_path_normalized() {
        assert_eq!(target_id(Path::new("/nonexistent-lumiwake/a/")), target_id(Path::new("/nonexistent-lumiwake/a")));
        assert_ne!(target_id(Path::new("/nonexistent-lumiwake/a")), target_id(Path::new("/nonexistent-lumiwake/b")));
    }

    #[test]
    fn choices_carry_label_and_description() {
        let dir = tempfile::tempdir().unwrap();
        let t = [target("1", "", &dir.path().join("風景"), "山や海の写真", false)];
        let c = build_choices(&t, AiBackend::Local);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].label.as_str(), c[0].description.as_str()), ("風景", "山や海の写真"));
    }

    #[test]
    fn excluded_folders_never_reach_an_external_backend() {
        let dir = tempfile::tempdir().unwrap();
        let t = [
            target("1", "風景", &dir.path().join("a"), "", false),
            target("2", "書類", &dir.path().join("b"), "個人情報を含む", true),
        ];
        let ext = build_choices(&t, AiBackend::Systemone);
        assert_eq!(ext.len(), 1);
        assert_eq!(ext[0].label, "風景");
        let all = serde_json::to_string(&ext).unwrap();
        assert!(!all.contains("書類") && !all.contains("個人情報"), "名前も説明文も出ない");

        assert_eq!(build_choices(&t, AiBackend::Local).len(), 2, "端末内のバックエンドでは除外しない");
    }
}
