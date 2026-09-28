//! ファイル操作モジュール: 移動・スキップ・削除予定・複数段 Undo・同名衝突の整合性を一手に持つ。
//!
//! 最優先は「ユーザーの画像ファイルを失わせない」こと。
//! - 削除キーではファイルを消さない（削除フォルダへ移動、または削除予定として記録するだけ）
//! - 完全削除は [`Session::finalize_deletions`]（アプリ終了時の確認後）でのみ行う
//! - 別ドライブ／SMB 間の移動は「一時ファイルへコピー → 検証 → 確定 → 元を削除」で、
//!   途中で失敗しても欠損や重複を残さない（[`safe_move`]）
//! - 操作履歴はメモリ上のみに持ち、永続化しない

mod fs;
mod session;

pub use fs::{safe_move, unique_path, Fs, RealFs};
pub use session::{
    Action, ConflictChoice, ConflictInfo, DeletionReason, FileOpError, FinalizeReport,
    HistoryEntry, Item, Outcome, PendingDeletion, Session, Status,
};
