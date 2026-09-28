//! 仕分けセッション: 画像の並び・現在位置・操作履歴（Undo）・削除予定を管理する。

use super::fs::{safe_move, unique_path, Fs};
use serde::Serialize;
use std::collections::HashSet;
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
    MoveTo {
        dir: PathBuf,
        label: String,
    },
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

/// 上書きのために退避した既存ファイル
#[derive(Debug, Clone)]
struct Displaced {
    original: PathBuf,
    now: PathBuf,
    in_trash: bool,
}

/// 操作履歴 1 件。取り消しに必要な情報をすべて持つ
#[derive(Debug, Clone)]
struct Entry {
    item: usize,
    prev_status: Status,
    label: String,
    /// 画像を動かした先（取り消し時にここから元の場所へ戻す）
    moved_to: Option<PathBuf>,
    /// 上書きで退避した既存ファイル（取り消し時に元へ戻す）
    displaced: Option<Displaced>,
}

pub struct Session {
    fs: Arc<dyn Fs>,
    items: Vec<Item>,
    /// 現在の画像の番号。常に未処理の画像を指すか、`items.len()`（完了）
    cursor: usize,
    trash_dir: Option<PathBuf>,
    conflict: Option<ConflictInfo>,
    /// 操作履歴（メモリ上のみ。上限なし）
    history: Vec<Entry>,
    /// 終了時の処理で完全削除したファイル
    finalized: HashSet<PathBuf>,
}

impl Session {
    pub fn new(fs: Arc<dyn Fs>, images: Vec<PathBuf>, trash_dir: Option<PathBuf>) -> Self {
        let items = images.into_iter().map(|path| Item { path, status: Status::Pending }).collect();
        Session { fs, items, cursor: 0, trash_dir, conflict: None, history: Vec::new(), finalized: HashSet::new() }
    }

    pub fn set_trash_dir(&mut self, dir: Option<PathBuf>) {
        self.trash_dir = dir;
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// 現在表示中の画像の番号。すべて処理し終えたら `None`
    pub fn current(&self) -> Option<usize> {
        self.items.get(self.cursor).filter(|i| i.status.is_open()).map(|_| self.cursor)
    }

    fn current_for_action(&self) -> Result<usize, FileOpError> {
        if self.conflict.is_some() {
            return Err(FileOpError::ConflictPending);
        }
        self.current().ok_or(FileOpError::NoCurrentItem)
    }

    pub fn apply(&mut self, action: Action) -> Result<Outcome, FileOpError> {
        let idx = self.current_for_action()?;
        match action {
            Action::MoveTo { dir, label } => {
                if !dir.is_dir() {
                    return Err(FileOpError::TargetMissing(dir));
                }
                let dest = dir.join(file_name(&self.items[idx].path));
                if self.fs.exists(&dest) {
                    let info =
                        ConflictInfo { item: idx, incoming: self.items[idx].path.clone(), existing: dest, dir, label };
                    self.conflict = Some(info.clone());
                    return Ok(Outcome::Conflict(info));
                }
                self.move_item(idx, dest, format!("→ {label}"), None)?;
            }
            Action::Skip => self.push(idx, Status::Skipped, "スキップ".into(), None, None),
            Action::Delete => self.delete_item(idx, DeletionReason::DeleteKey)?,
        }
        self.advance_from(idx);
        Ok(Outcome::Done)
    }

    pub fn pending_conflict(&self) -> Option<&ConflictInfo> {
        self.conflict.as_ref()
    }

    pub fn resolve_conflict(&mut self, choice: ConflictChoice) -> Result<(), FileOpError> {
        let info = self.conflict.clone().ok_or(FileOpError::NoPendingConflict)?;
        let idx = info.item;
        match choice {
            ConflictChoice::KeepExisting => self.delete_item(idx, DeletionReason::KeepExisting)?,
            ConflictChoice::Overwrite => {
                let displaced = self.displace(&info.existing)?;
                let label = format!("→ {}（上書き）", info.label);
                if let Err(e) = self.move_item(idx, info.existing.clone(), label, Some(displaced.clone())) {
                    // 退避した既存ファイルを元に戻してから失敗を返す
                    let _ = safe_move(self.fs.as_ref(), &displaced.now, &displaced.original);
                    return Err(e);
                }
            }
            ConflictChoice::KeepBoth => {
                let dest = unique_path(self.fs.as_ref(), &info.dir, &file_name(&info.incoming));
                self.move_item(idx, dest, format!("→ {}（リネーム）", info.label), None)?;
            }
            ConflictChoice::Skip => self.push(idx, Status::Skipped, "スキップ".into(), None, None),
        }
        self.conflict = None;
        self.advance_from(idx);
        Ok(())
    }

    pub fn cancel_conflict(&mut self) {
        self.conflict = None;
    }

    /// 直前の操作を取り消し、その説明を返す
    pub fn undo(&mut self) -> Result<String, FileOpError> {
        self.conflict = None;
        let entry = self.history.last().cloned().ok_or(FileOpError::NothingToUndo)?;
        let original = self.items[entry.item].path.clone();

        let touched = [
            entry.moved_to.as_ref(),
            entry.displaced.as_ref().map(|d| &d.now),
            matches!(self.items[entry.item].status, Status::Marked { .. }).then_some(&original),
        ];
        if let Some(gone) = touched.into_iter().flatten().find(|p| self.finalized.contains(*p)) {
            return Err(io_err(
                "完全削除済みのため取り消せません",
                gone,
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ));
        }

        if let Some(moved_to) = &entry.moved_to {
            safe_move(self.fs.as_ref(), moved_to, &original)
                .map_err(|e| io_err("取り消せませんでした（元の場所へ戻せません）", &original, e))?;
        }
        if let Some(d) = &entry.displaced {
            if let Err(e) = safe_move(self.fs.as_ref(), &d.now, &d.original) {
                // 既存ファイルを戻せないなら、画像も移動後の状態に戻して整合性を保つ
                if let Some(moved_to) = &entry.moved_to {
                    let _ = safe_move(self.fs.as_ref(), &original, moved_to);
                }
                return Err(io_err("取り消せませんでした（上書き前のファイルを戻せません）", &d.original, e));
            }
        }

        self.history.pop();
        self.items[entry.item].status = entry.prev_status;
        self.cursor = entry.item;
        Ok(format!("取り消しました: {}（{}）", file_name(&original), entry.label))
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// 新しい順に最大 `n` 件
    pub fn recent_history(&self, n: usize) -> Vec<HistoryEntry> {
        self.history.iter().rev().take(n).map(|e| HistoryEntry { item: e.item, label: e.label.clone() }).collect()
    }

    /// 前の未処理（保留を含む）画像へ
    pub fn go_prev(&mut self) -> bool {
        match (0..self.cursor.min(self.items.len())).rev().find(|&i| self.items[i].status.is_open()) {
            Some(i) => {
                self.conflict = None;
                self.cursor = i;
                true
            }
            None => false,
        }
    }

    /// 次の未処理（保留を含む）画像へ。最後の次は完了
    pub fn go_next(&mut self) -> bool {
        if self.cursor >= self.items.len() {
            return false;
        }
        self.conflict = None;
        self.advance_from(self.cursor);
        true
    }

    /// 現在の画像より後ろの未処理画像を最大 `n` 件
    pub fn upcoming(&self, n: usize) -> Vec<usize> {
        (self.cursor + 1..self.items.len()).filter(|&i| self.items[i].status.is_open()).take(n).collect()
    }

    pub fn pending_deletions(&self) -> Vec<PendingDeletion> {
        let items = self.items.iter().filter_map(|item| match &item.status {
            Status::Trashed { to, reason } => {
                Some(PendingDeletion { path: to.clone(), original: item.path.clone(), reason: *reason, in_trash: true })
            }
            Status::Marked { reason } => Some(PendingDeletion {
                path: item.path.clone(),
                original: item.path.clone(),
                reason: *reason,
                in_trash: false,
            }),
            _ => None,
        });
        let displaced = self.history.iter().filter_map(|e| e.displaced.as_ref()).map(|d| PendingDeletion {
            path: d.now.clone(),
            original: d.original.clone(),
            reason: DeletionReason::Overwritten,
            in_trash: d.in_trash,
        });
        items.chain(displaced).filter(|p| !self.finalized.contains(&p.path)).collect()
    }

    /// 削除予定のファイルを完全削除する（アプリ終了時の確認後にのみ呼ぶ）
    pub fn finalize_deletions(&mut self) -> FinalizeReport {
        let mut report = FinalizeReport::default();
        for pending in self.pending_deletions() {
            match self.fs.remove_file(&pending.path) {
                Ok(()) => {
                    self.finalized.insert(pending.path.clone());
                    report.deleted.push(pending.path);
                }
                Err(e) => report.failed.push((pending.path, e.to_string())),
            }
        }
        report
    }

    // ---- 内部処理 ----

    fn move_item(
        &mut self,
        idx: usize,
        dest: PathBuf,
        label: String,
        displaced: Option<Displaced>,
    ) -> Result<(), FileOpError> {
        let from = self.items[idx].path.clone();
        safe_move(self.fs.as_ref(), &from, &dest).map_err(|e| io_err("画像を移動できませんでした", &from, e))?;
        self.push(idx, Status::Moved { to: dest.clone() }, label, Some(dest), displaced);
        Ok(())
    }

    fn delete_item(&mut self, idx: usize, reason: DeletionReason) -> Result<(), FileOpError> {
        let label = match reason {
            DeletionReason::KeepExisting => "削除（既存を残す）",
            _ => "削除",
        }
        .to_string();
        match self.trash_dir.clone() {
            Some(trash) => {
                if !trash.is_dir() {
                    return Err(FileOpError::TargetMissing(trash));
                }
                let from = self.items[idx].path.clone();
                let dest = unique_path(self.fs.as_ref(), &trash, &file_name(&from));
                safe_move(self.fs.as_ref(), &from, &dest)
                    .map_err(|e| io_err("削除フォルダへ移動できませんでした", &from, e))?;
                self.push(idx, Status::Trashed { to: dest.clone(), reason }, label, Some(dest), None);
            }
            None => self.push(idx, Status::Marked { reason }, label, None, None),
        }
        Ok(())
    }

    /// 上書きされる既存ファイルを退避する。削除フォルダがあればそこへ、なければ同じフォルダでリネームする
    fn displace(&self, existing: &Path) -> Result<Displaced, FileOpError> {
        let fs = self.fs.as_ref();
        let (now, in_trash) = match &self.trash_dir {
            Some(trash) => {
                if !trash.is_dir() {
                    return Err(FileOpError::TargetMissing(trash.clone()));
                }
                (unique_path(fs, trash, &file_name(existing)), true)
            }
            None => {
                let stem = existing.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ext = existing.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                let dir = existing.parent().unwrap_or(Path::new("."));
                (unique_path(fs, dir, &format!("{stem}.lumiwake-old{ext}")), false)
            }
        };
        safe_move(fs, existing, &now)
            .map_err(|e| io_err("上書きする既存ファイルを退避できませんでした", existing, e))?;
        Ok(Displaced { original: existing.to_path_buf(), now, in_trash })
    }

    fn push(
        &mut self,
        idx: usize,
        status: Status,
        label: String,
        moved_to: Option<PathBuf>,
        displaced: Option<Displaced>,
    ) {
        let prev_status = std::mem::replace(&mut self.items[idx].status, status);
        self.history.push(Entry { item: idx, prev_status, label, moved_to, displaced });
    }

    /// `idx` より後ろの最初の未処理画像へ進む。なければ完了
    fn advance_from(&mut self, idx: usize) {
        self.cursor = (idx + 1..self.items.len()).find(|&i| self.items[i].status.is_open()).unwrap_or(self.items.len());
    }
}

fn io_err(context: &'static str, path: &Path, source: std::io::Error) -> FileOpError {
    FileOpError::Io { context, path: path.to_path_buf(), source }
}

pub(crate) fn file_name(path: &Path) -> String {
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
