//! 仕分けセッション: 画像の並び・現在位置・操作履歴（Undo）・削除予定を管理する。

use super::fs::Fs;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 画像 1 枚ごとの状態
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    /// 未処理
    Pending,
    /// 保留（ファイルは動かしていない）
    Skipped,
    /// 振り分け先へ移動済み
    Moved { to: PathBuf },
    /// 削除フォルダへ移動済み（終了時に完全削除の対象）
    Trashed { to: PathBuf, reason: DeletionReason },
    /// 元の場所のまま削除予定として記録（終了時に完全削除の対象）
    Marked { reason: DeletionReason },
}

impl Status {
    /// 表示・仕分けの対象として残っているか
    pub fn is_open(&self) -> bool {
        matches!(self, Status::Pending | Status::Skipped)
    }
}

/// 削除予定になった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeletionReason {
    /// 削除キー
    DeleteKey,
    /// 同名ファイルの衝突で「既存を残す」を選んだため、移動しようとしたファイルが不要になった
    KeepExisting,
    /// 同名ファイルの衝突で「上書き」を選んだため、置き換えられた既存ファイル
    Overwritten,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// 仕分け元での元の場所
    pub path: PathBuf,
    pub status: Status,
}

/// ユーザーのキー操作
#[derive(Debug, Clone)]
pub enum Action {
    /// 振り分け先フォルダへ移動する。`label` は履歴表示用の名前
    MoveTo { dir: PathBuf, label: String },
    Skip,
    Delete,
}

/// 同名ファイルがあったときの選択肢
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictChoice {
    /// 移動先の既存ファイルを残す（移動しようとしたファイルは削除予定にする）
    KeepExisting,
    /// 既存ファイルを置き換える（既存ファイルは削除予定にする）
    Overwrite,
    /// 移動するファイルをリネームして両方残す
    KeepBoth,
    /// 今回は移動しない（保留）
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub item: usize,
    /// 移動しようとしたファイル
    pub incoming: PathBuf,
    /// 移動先にある同名ファイル
    pub existing: PathBuf,
    pub dir: PathBuf,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// 同名ファイルがあるため、ユーザーの選択を待っている（まだ何も動かしていない）
    Conflict(ConflictInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDeletion {
    /// 現在ファイルがある場所
    pub path: PathBuf,
    /// 元の場所（削除フォルダへ移動したもの・リネームしたものは path と異なる）
    pub original: PathBuf,
    pub reason: DeletionReason,
    /// 削除フォルダ内にあるか
    pub in_trash: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeReport {
    pub deleted: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

/// フィルムストリップ用の履歴の見え方
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub item: usize,
    pub label: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FileOpError {
    #[error("処理する画像がありません")]
    NoCurrentItem,
    #[error("同名ファイルの確認待ちではありません")]
    NoPendingConflict,
    #[error("同名ファイルの処理を先に選んでください")]
    ConflictPending,
    #[error("振り分け先フォルダが見つかりません: {0}")]
    TargetMissing(PathBuf),
    #[error("取り消せる操作がありません")]
    NothingToUndo,
    #[error("{context}: {path}（{source}）")]
    Io {
        context: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct Session {
    fs: Arc<dyn Fs>,
    items: Vec<Item>,
    cursor: usize,
    trash_dir: Option<PathBuf>,
    conflict: Option<ConflictInfo>,
}

impl Session {
    pub fn new(_fs: Arc<dyn Fs>, _images: Vec<PathBuf>, _trash_dir: Option<PathBuf>) -> Self {
        todo!()
    }
    pub fn set_trash_dir(&mut self, _dir: Option<PathBuf>) {
        todo!()
    }
    pub fn items(&self) -> &[Item] {
        todo!()
    }
    /// 現在表示中の画像の番号。すべて処理し終えたら `None`
    pub fn current(&self) -> Option<usize> {
        todo!()
    }
    pub fn apply(&mut self, _action: Action) -> Result<Outcome, FileOpError> {
        todo!()
    }
    pub fn pending_conflict(&self) -> Option<&ConflictInfo> {
        todo!()
    }
    pub fn resolve_conflict(&mut self, _choice: ConflictChoice) -> Result<(), FileOpError> {
        todo!()
    }
    pub fn cancel_conflict(&mut self) {
        todo!()
    }
    /// 直前の操作を取り消し、その説明を返す
    pub fn undo(&mut self) -> Result<String, FileOpError> {
        todo!()
    }
    pub fn can_undo(&self) -> bool {
        todo!()
    }
    /// 新しい順に最大 `n` 件
    pub fn recent_history(&self, _n: usize) -> Vec<HistoryEntry> {
        todo!()
    }
    /// 前の未処理（保留を含む）画像へ
    pub fn go_prev(&mut self) -> bool {
        todo!()
    }
    /// 次の未処理（保留を含む）画像へ
    pub fn go_next(&mut self) -> bool {
        todo!()
    }
    /// 現在の画像より後ろの未処理画像を最大 `n` 件
    pub fn upcoming(&self, _n: usize) -> Vec<usize> {
        todo!()
    }
    pub fn pending_deletions(&self) -> Vec<PendingDeletion> {
        todo!()
    }
    /// 削除予定のファイルを完全削除する（アプリ終了時の確認後にのみ呼ぶ）
    pub fn finalize_deletions(&mut self) -> FinalizeReport {
        todo!()
    }
}

#[allow(dead_code)]
fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::fs::tests::{listing, FaultyFs};
    use super::super::fs::RealFs;
    use super::*;
    use std::fs;

    struct Env {
        _tmp: tempfile::TempDir,
        src: PathBuf,
        a: PathBuf,
        b: PathBuf,
        trash: PathBuf,
    }

    /// 仕分け元に img1〜img{n}.jpg、振り分け先 a / b、削除フォルダ trash を用意する
    fn env(n: usize) -> (Env, Vec<PathBuf>) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let (src, a, b, trash) = (root.join("src"), root.join("a"), root.join("b"), root.join("trash"));
        for d in [&src, &a, &b, &trash] {
            fs::create_dir_all(d).unwrap();
        }
        let images: Vec<PathBuf> = (1..=n)
            .map(|i| {
                let p = src.join(format!("img{i}.jpg"));
                fs::write(&p, format!("image {i}")).unwrap();
                p
            })
            .collect();
        (Env { _tmp: tmp, src, a, b, trash }, images)
    }

    fn session(images: Vec<PathBuf>, trash: Option<&Path>) -> Session {
        Session::new(Arc::new(RealFs), images, trash.map(Path::to_path_buf))
    }

    fn move_to(dir: &Path) -> Action {
        Action::MoveTo { dir: dir.to_path_buf(), label: file_name(dir) }
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    #[test]
    fn move_advances_to_next_image() {
        let (e, imgs) = env(3);
        let mut s = session(imgs.clone(), None);
        assert_eq!(s.current(), Some(0));
        assert_eq!(s.apply(move_to(&e.a)).unwrap(), Outcome::Done);
        assert_eq!(read(&e.a.join("img1.jpg")), "image 1");
        assert!(!imgs[0].exists());
        assert_eq!(s.items()[0].status, Status::Moved { to: e.a.join("img1.jpg") });
        assert_eq!(s.current(), Some(1));
    }

    #[test]
    fn skip_keeps_the_file_in_place() {
        let (_e, imgs) = env(2);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Skip).unwrap();
        assert!(imgs[0].exists());
        assert_eq!(s.items()[0].status, Status::Skipped);
        assert_eq!(s.current(), Some(1));
    }

    #[test]
    fn delete_without_trash_folder_only_marks() {
        let (e, imgs) = env(2);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Delete).unwrap();
        assert_eq!(read(&imgs[0]), "image 1", "削除キーではファイルを消さない");
        assert_eq!(s.items()[0].status, Status::Marked { reason: DeletionReason::DeleteKey });
        assert_eq!(s.current(), Some(1));
        assert_eq!(
            s.pending_deletions(),
            vec![PendingDeletion {
                path: imgs[0].clone(),
                original: imgs[0].clone(),
                reason: DeletionReason::DeleteKey,
                in_trash: false
            }]
        );
        assert_eq!(listing(&e.src), ["img1.jpg", "img2.jpg"]);
    }

    #[test]
    fn delete_with_trash_folder_moves_there() {
        let (e, imgs) = env(1);
        let mut s = session(imgs.clone(), Some(&e.trash));
        s.apply(Action::Delete).unwrap();
        assert!(!imgs[0].exists());
        assert_eq!(read(&e.trash.join("img1.jpg")), "image 1");
        let pending = s.pending_deletions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].path, e.trash.join("img1.jpg"));
        assert!(pending[0].in_trash);
        assert_eq!(s.current(), None, "最後の 1 枚を処理したら完了");
    }

    #[test]
    fn delete_into_trash_renames_on_name_collision() {
        let (e, imgs) = env(1);
        fs::write(e.trash.join("img1.jpg"), "older trash").unwrap();
        let mut s = session(imgs, Some(&e.trash));
        s.apply(Action::Delete).unwrap();
        assert_eq!(read(&e.trash.join("img1.jpg")), "older trash");
        assert_eq!(read(&e.trash.join("img1 (2).jpg")), "image 1");
    }

    #[test]
    fn multi_level_undo_restores_everything() {
        let (e, imgs) = env(5);
        let mut s = session(imgs.clone(), Some(&e.trash));
        s.apply(move_to(&e.a)).unwrap();
        s.apply(Action::Delete).unwrap();
        s.apply(Action::Skip).unwrap();
        s.apply(move_to(&e.b)).unwrap();
        assert_eq!(s.current(), Some(4));
        assert_eq!(s.recent_history(10).iter().map(|h| h.item).collect::<Vec<_>>(), [3, 2, 1, 0]);

        for expected_cursor in [3, 2, 1, 0] {
            s.undo().unwrap();
            assert_eq!(s.current(), Some(expected_cursor));
        }
        assert!(!s.can_undo());
        assert!(matches!(s.undo(), Err(FileOpError::NothingToUndo)));
        for (i, p) in imgs.iter().enumerate() {
            assert_eq!(read(p), format!("image {}", i + 1));
        }
        assert!(listing(&e.a).is_empty());
        assert!(listing(&e.b).is_empty());
        assert!(listing(&e.trash).is_empty());
        assert!(s.items().iter().all(|i| i.status == Status::Pending));
        assert!(s.pending_deletions().is_empty());
    }

    #[test]
    fn undo_of_marked_delete_unmarks() {
        let (_e, imgs) = env(1);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Delete).unwrap();
        s.undo().unwrap();
        assert_eq!(s.items()[0].status, Status::Pending);
        assert!(s.pending_deletions().is_empty());
        assert_eq!(s.current(), Some(0));
    }

    #[test]
    fn undo_restores_previous_skipped_status() {
        let (e, imgs) = env(2);
        let mut s = session(imgs, None);
        s.apply(Action::Skip).unwrap();
        assert!(s.go_prev());
        s.apply(move_to(&e.a)).unwrap();
        s.undo().unwrap();
        assert_eq!(s.items()[0].status, Status::Skipped);
    }

    #[test]
    fn undo_refuses_to_overwrite_a_file_that_reappeared() {
        let (e, imgs) = env(1);
        let mut s = session(imgs.clone(), None);
        s.apply(move_to(&e.a)).unwrap();
        fs::write(&imgs[0], "someone else's file").unwrap();
        assert!(s.undo().is_err());
        assert_eq!(read(&imgs[0]), "someone else's file");
        assert_eq!(read(&e.a.join("img1.jpg")), "image 1");
        assert!(s.can_undo(), "失敗した取り消しは履歴に残す");
    }

    #[test]
    fn missing_target_folder_is_an_error_and_nothing_moves() {
        let (e, imgs) = env(1);
        let mut s = session(imgs.clone(), None);
        let err = s.apply(move_to(&e.a.join("nope"))).unwrap_err();
        assert!(matches!(err, FileOpError::TargetMissing(_)));
        assert!(imgs[0].exists());
        assert_eq!(s.current(), Some(0));
    }

    #[test]
    fn cross_device_moves_and_undo_work() {
        let (e, imgs) = env(2);
        let fs_ = Arc::new(FaultyFs { cross_device: true, ..Default::default() });
        let mut s = Session::new(fs_, imgs.clone(), Some(e.trash.clone()));
        s.apply(move_to(&e.a)).unwrap();
        s.apply(Action::Delete).unwrap();
        assert_eq!(listing(&e.a), ["img1.jpg"]);
        assert_eq!(listing(&e.trash), ["img2.jpg"]);
        s.undo().unwrap();
        s.undo().unwrap();
        assert_eq!(listing(&e.src), ["img1.jpg", "img2.jpg"]);
        assert!(listing(&e.a).is_empty());
        assert!(listing(&e.trash).is_empty());
    }

    // ---- 同名ファイルの衝突 ----

    fn conflict_env(trash: bool) -> (Env, Vec<PathBuf>, Session) {
        let (e, imgs) = env(2);
        fs::write(e.a.join("img1.jpg"), "existing in a").unwrap();
        let s = session(imgs.clone(), trash.then_some(e.trash.as_path()));
        (e, imgs, s)
    }

    #[test]
    fn conflict_is_reported_before_anything_moves() {
        let (e, imgs, mut s) = conflict_env(false);
        let outcome = s.apply(move_to(&e.a)).unwrap();
        let Outcome::Conflict(info) = outcome else { panic!("衝突になるはず") };
        assert_eq!(info.incoming, imgs[0]);
        assert_eq!(info.existing, e.a.join("img1.jpg"));
        assert_eq!(read(&imgs[0]), "image 1");
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert_eq!(s.current(), Some(0));
        assert!(s.pending_conflict().is_some());
        assert!(matches!(s.apply(Action::Skip), Err(FileOpError::ConflictPending)));
        s.cancel_conflict();
        assert!(s.pending_conflict().is_none());
        assert_eq!(s.current(), Some(0));
    }

    #[test]
    fn keep_existing_marks_incoming_for_deletion() {
        let (e, imgs, mut s) = conflict_env(false);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::KeepExisting).unwrap();
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert_eq!(read(&imgs[0]), "image 1", "消さずに削除予定にするだけ");
        assert_eq!(s.items()[0].status, Status::Marked { reason: DeletionReason::KeepExisting });
        assert_eq!(s.current(), Some(1));
        s.undo().unwrap();
        assert_eq!(s.items()[0].status, Status::Pending);
    }

    #[test]
    fn keep_existing_with_trash_moves_incoming_to_trash() {
        let (e, imgs, mut s) = conflict_env(true);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::KeepExisting).unwrap();
        assert!(!imgs[0].exists());
        assert_eq!(read(&e.trash.join("img1.jpg")), "image 1");
        s.undo().unwrap();
        assert_eq!(read(&imgs[0]), "image 1");
        assert!(listing(&e.trash).is_empty());
    }

    #[test]
    fn overwrite_without_trash_keeps_old_file_as_pending_deletion() {
        let (e, imgs, mut s) = conflict_env(false);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::Overwrite).unwrap();
        assert_eq!(read(&e.a.join("img1.jpg")), "image 1");
        assert!(!imgs[0].exists());
        let pending = s.pending_deletions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].reason, DeletionReason::Overwritten);
        assert_eq!(pending[0].original, e.a.join("img1.jpg"));
        assert_eq!(read(&pending[0].path), "existing in a", "置き換えられたファイルはまだ消えていない");

        s.undo().unwrap();
        assert_eq!(read(&imgs[0]), "image 1");
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert_eq!(listing(&e.a), ["img1.jpg"]);
        assert!(s.pending_deletions().is_empty());
    }

    #[test]
    fn overwrite_with_trash_moves_old_file_to_trash() {
        let (e, _imgs, mut s) = conflict_env(true);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::Overwrite).unwrap();
        assert_eq!(read(&e.a.join("img1.jpg")), "image 1");
        assert_eq!(read(&e.trash.join("img1.jpg")), "existing in a");
        assert!(s.pending_deletions()[0].in_trash);
        s.undo().unwrap();
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert!(listing(&e.trash).is_empty());
    }

    #[test]
    fn keep_both_renames_incoming() {
        let (e, imgs, mut s) = conflict_env(false);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::KeepBoth).unwrap();
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert_eq!(read(&e.a.join("img1 (2).jpg")), "image 1");
        s.undo().unwrap();
        assert_eq!(read(&imgs[0]), "image 1");
        assert_eq!(listing(&e.a), ["img1.jpg"]);
    }

    #[test]
    fn skip_choice_leaves_both_files() {
        let (e, imgs, mut s) = conflict_env(false);
        s.apply(move_to(&e.a)).unwrap();
        s.resolve_conflict(ConflictChoice::Skip).unwrap();
        assert_eq!(read(&imgs[0]), "image 1");
        assert_eq!(read(&e.a.join("img1.jpg")), "existing in a");
        assert_eq!(s.items()[0].status, Status::Skipped);
        assert_eq!(s.current(), Some(1));
    }

    #[test]
    fn resolve_without_conflict_is_an_error() {
        let (_e, imgs) = env(1);
        let mut s = session(imgs, None);
        assert!(matches!(s.resolve_conflict(ConflictChoice::Overwrite), Err(FileOpError::NoPendingConflict)));
    }

    // ---- 並び・移動 ----

    #[test]
    fn navigation_visits_only_open_items() {
        let (e, imgs) = env(4);
        let mut s = session(imgs, None);
        s.apply(Action::Skip).unwrap(); // 0: 保留
        s.apply(move_to(&e.a)).unwrap(); // 1: 移動
        assert_eq!(s.current(), Some(2));
        assert_eq!(s.upcoming(5), vec![3]);
        assert!(s.go_prev());
        assert_eq!(s.current(), Some(0), "移動済みの 1 は飛ばす");
        assert!(!s.go_prev());
        assert!(s.go_next());
        assert_eq!(s.current(), Some(2));
        assert!(s.go_next());
        assert_eq!(s.current(), Some(3));
        assert!(s.go_next());
        assert_eq!(s.current(), None, "最後の次は完了画面");
        assert!(s.go_prev());
        assert_eq!(s.current(), Some(3));
    }

    #[test]
    fn finishing_with_skipped_items_allows_going_back() {
        let (_e, imgs) = env(2);
        let mut s = session(imgs, None);
        s.apply(Action::Skip).unwrap();
        s.apply(Action::Skip).unwrap();
        assert_eq!(s.current(), None);
        assert!(s.go_prev());
        assert_eq!(s.current(), Some(1));
    }

    // ---- 終了時の完全削除 ----

    #[test]
    fn finalize_deletes_only_pending_files() {
        let (e, imgs) = env(4);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Delete).unwrap(); // img1: 削除予定
        s.apply(move_to(&e.a)).unwrap(); // img2: 移動
        s.apply(Action::Delete).unwrap(); // img3: 削除予定 → 取り消す
        s.undo().unwrap();
        s.apply(Action::Skip).unwrap(); // img3: 保留

        let report = s.finalize_deletions();
        assert_eq!(report.deleted, vec![imgs[0].clone()]);
        assert!(report.failed.is_empty());
        assert_eq!(listing(&e.src), ["img3.jpg", "img4.jpg"]);
        assert_eq!(listing(&e.a), ["img2.jpg"]);
    }

    #[test]
    fn finalize_empties_only_files_moved_to_trash_in_this_session() {
        let (e, imgs) = env(1);
        fs::write(e.trash.join("from-before.jpg"), "not ours").unwrap();
        let mut s = session(imgs, Some(&e.trash));
        s.apply(Action::Delete).unwrap();
        let report = s.finalize_deletions();
        assert_eq!(report.deleted, vec![e.trash.join("img1.jpg")]);
        assert_eq!(listing(&e.trash), ["from-before.jpg"]);
    }

    #[test]
    fn finalize_reports_failures_and_continues() {
        let (_e, imgs) = env(2);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Delete).unwrap();
        s.apply(Action::Delete).unwrap();
        fs::remove_file(&imgs[0]).unwrap(); // 外部で先に消された
        let report = s.finalize_deletions();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.deleted, vec![imgs[1].clone()]);
    }

    #[test]
    fn trash_dir_can_change_during_session() {
        let (e, imgs) = env(2);
        let mut s = session(imgs.clone(), None);
        s.apply(Action::Delete).unwrap();
        s.set_trash_dir(Some(e.trash.clone()));
        s.apply(Action::Delete).unwrap();
        assert!(imgs[0].exists());
        assert_eq!(listing(&e.trash), ["img2.jpg"]);
        assert_eq!(s.pending_deletions().len(), 2);
    }
}
