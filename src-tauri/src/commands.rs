//! 画面（WebView）から呼ばれる Tauri コマンド。
//!
//! 画面はキー入力をここへ渡すだけで、ファイル操作の整合性は `fileops` が持つ。

use crate::config::{self, Config, Issue};
use crate::decoder::{self, FormatSupport};
use crate::fileops::{self, Action, ConflictChoice, FinalizeReport, HistoryKind, PendingDeletion, Session, Status};
use crate::scanner::{self, ScanOptions};
use crate::state::{location, AppState, SessionState};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

type CmdResult<T> = Result<T, String>;

fn lossy(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// ---- 設定 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPayload {
    config: Config,
    path: String,
    issues: Vec<Issue>,
    load_error: Option<String>,
    formats: Vec<FormatSupport>,
}

fn config_payload(state: &AppState) -> ConfigPayload {
    let c = state.config();
    ConfigPayload {
        config: c.config.clone(),
        path: lossy(&state.config_path),
        issues: c.config.validate(),
        load_error: c.load_error.clone(),
        formats: state.registry.format_support(),
    }
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> ConfigPayload {
    config_payload(&state)
}

#[tauri::command]
pub fn validate_config(config: Config) -> Vec<Issue> {
    let mut config = config;
    config.normalize();
    config.validate()
}

#[tauri::command]
pub fn save_config(state: State<'_, AppState>, config: Config) -> CmdResult<ConfigPayload> {
    let (saved, _) = config::save(&state.config_path, &config).map_err(|e| e.to_string())?;
    apply_config(&state, saved, None);
    Ok(config_payload(&state))
}

#[tauri::command]
pub fn reload_config(state: State<'_, AppState>) -> CmdResult<ConfigPayload> {
    let text = std::fs::read_to_string(&state.config_path).map_err(|e| e.to_string())?;
    let loaded = config::parse(&text, &state.config_path).map_err(|e| e.to_string())?;
    apply_config(&state, loaded, None);
    Ok(config_payload(&state))
}

fn apply_config(state: &AppState, config: Config, load_error: Option<String>) {
    let trash = config.general.delete_folder.clone();
    let order = config.general.sort_order;
    let (order_changed, prefetch) = {
        let mut c = state.config();
        let changed = c.config.general.sort_order != order;
        c.config = config;
        c.load_error = load_error;
        (changed, c.config.general.prefetch)
    };
    if let Some(s) = state.session().as_mut() {
        s.session.set_trash_dir(trash);
        if order_changed {
            // 仕分け中に並び順を変えたら、まだ振り分けていない画像をすぐ並べ直す
            s.session.reorder_open(|paths| scanner::sort_paths(paths, order));
            // 画像の番号が変わるので、画面が持っている古い番号の URL を無効にする
            s.generation = state.next_generation();
            state.schedule_prefetch(s, prefetch);
        }
    }
}

/// 設定の一部をファイルに書き戻し、メモリ上の設定も合わせる。
/// 保存できない場合（ファイルに誤りがある）はメモリ上だけ変更する。
fn remember(state: &AppState, patch: impl Fn(&mut Config)) {
    match config::update_file(&state.config_path, &patch) {
        Ok(updated) => apply_config(state, updated, None),
        Err(_) => patch(&mut state.config().config),
    }
}

/// 仕分け元フォルダの履歴から外す
#[tauri::command]
pub fn forget_source(state: State<'_, AppState>, path: String) -> ConfigPayload {
    let path = PathBuf::from(path);
    remember(&state, |c| c.forget_source(&path));
    config_payload(&state)
}

/// 表示モードの切り替え（次回の起動時も同じモードにする）
#[tauri::command]
pub fn set_view_mode(state: State<'_, AppState>, mode: config::ViewMode) -> ConfigPayload {
    remember(&state, |c| c.general.view_mode = mode);
    config_payload(&state)
}

/// 振り分け先リストのパス表示の切り替え（次回の起動時も同じ表示にする）
#[tauri::command]
pub fn set_show_paths(state: State<'_, AppState>, show: bool) -> ConfigPayload {
    remember(&state, |c| c.general.show_paths = show);
    config_payload(&state)
}

#[tauri::command]
pub fn open_config_file(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    app.opener().open_path(lossy(&state.config_path), None::<&str>).map_err(|e| e.to_string())
}

/// フォルダはそのまま開き、ファイルはファイルマネージャーで選択した状態で開く
#[tauri::command]
pub fn open_location(app: AppHandle, path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if p.is_dir() {
        app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
    } else {
        app.opener().reveal_item_in_dir(p).map_err(|e| e.to_string())
    }
}

/// フォルダ直下のサブフォルダ（振り分け先をまとめて登録するのに使う）
#[tauri::command]
pub fn list_subfolders(path: String) -> CmdResult<Vec<String>> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|e| format!("フォルダを読み込めません（{path}）: {e}"))?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| !fileops::file_name(p).starts_with('.'))
        .collect();
    dirs.sort_by(|a, b| scanner::natural_cmp_path(a, b));
    Ok(dirs.iter().map(|p| lossy(p)).collect())
}

// ---- 仕分けセッション ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    index: usize,
    name: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    path: String,
    size: Option<u64>,
    /// 更新日時（UNIX エポックからのミリ秒）
    modified: Option<u64>,
}

fn file_view(path: &Path) -> FileView {
    let meta = std::fs::metadata(path).ok();
    FileView {
        name: fileops::file_name(path),
        path: lossy(path),
        size: meta.as_ref().map(|m| m.len()),
        modified: meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    item: usize,
    label: String,
    dir: String,
    incoming: FileView,
    existing: FileView,
}

/// フィルムストリップに出す直近の操作
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryView {
    item: usize,
    label: String,
    kind: HistoryKind,
    /// まだ未処理（保留中）で、クリックして戻れるか
    open: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    generation: u64,
    source: String,
    total: usize,
    /// 現在の画像の番号（すべて処理し終えたら null）
    current: Option<ItemView>,
    /// 未処理（保留を含む）の枚数
    remaining: usize,
    skipped: usize,
    upcoming: Vec<usize>,
    history: Vec<HistoryView>,
    /// 次に見る保留中の画像（現在の画像を除く）
    next_skipped: Option<usize>,
    can_undo: bool,
    pending_deletions: usize,
    /// 振り分け先ごとの、このセッションで移動した枚数（設定の targets と同じ並び）
    moved_counts: Vec<usize>,
    unsupported: Vec<String>,
    /// 読み込み時に読み飛ばした振り分け先フォルダ
    excluded_targets: Vec<String>,
    conflict: Option<ConflictView>,
}

/// フィルムストリップに出す直近の操作の数
const HISTORY_TILES: usize = 4;

fn session_view(state: &AppState, s: &SessionState) -> SessionView {
    let session = &s.session;
    let items = session.items();
    let targets: Vec<PathBuf> = state.config().config.targets.iter().map(|t| t.path.clone()).collect();
    let mut moved_counts = vec![0; targets.len()];
    let (mut remaining, mut skipped) = (0, 0);
    for item in items {
        match &item.status {
            Status::Pending => remaining += 1,
            Status::Skipped => {
                remaining += 1;
                skipped += 1;
            }
            Status::Moved { to } => {
                if let Some(i) = targets.iter().position(|t| to.parent() == Some(t.as_path())) {
                    moved_counts[i] += 1;
                }
            }
            _ => {}
        }
    }
    SessionView {
        generation: s.generation,
        source: lossy(&s.source),
        total: items.len(),
        current: session.current().map(|i| ItemView {
            index: i,
            name: fileops::file_name(&items[i].path),
            path: lossy(&items[i].path),
        }),
        remaining,
        skipped,
        upcoming: session.upcoming(4),
        history: session
            .recent_history(HISTORY_TILES)
            .into_iter()
            .map(|h| HistoryView {
                open: h.kind == HistoryKind::Skip && items[h.item].status == Status::Skipped,
                item: h.item,
                label: h.label,
                kind: h.kind,
            })
            .collect(),
        next_skipped: session.next_skipped(),
        can_undo: session.can_undo(),
        pending_deletions: session.pending_deletions().len()
            + state.retired().iter().map(|r| r.pending_deletions().len()).sum::<usize>(),
        moved_counts,
        unsupported: s.unsupported.iter().map(|p| lossy(p)).collect(),
        excluded_targets: s.excluded_targets.iter().map(|p| lossy(p)).collect(),
        conflict: session.pending_conflict().map(|c| ConflictView {
            item: c.item,
            label: c.label.clone(),
            dir: lossy(&c.dir),
            incoming: file_view(&c.incoming),
            existing: file_view(&c.existing),
        }),
    }
}

/// セッションを操作して、画面に返す状態を作る（先読みの予定もここで更新する）
fn with_session<T>(
    state: &AppState,
    f: impl FnOnce(&mut SessionState) -> Result<T, String>,
) -> CmdResult<(T, SessionView)> {
    let prefetch = state.config().config.general.prefetch;
    let mut guard = state.session();
    let s = guard.as_mut().ok_or("仕分け元フォルダが読み込まれていません")?;
    let result = f(s);
    state.schedule_prefetch(s, prefetch);
    let view = session_view(state, s);
    Ok((result?, view))
}

#[tauri::command]
pub fn start_session(
    state: State<'_, AppState>,
    source: String,
    include_subdirs: bool,
    include_targets: bool,
) -> CmdResult<SessionView> {
    let source = PathBuf::from(source);
    let (exclude, trash, order) = {
        let c = state.config();
        // 削除フォルダは常に読み込まない。振り分け先は、含める指定がなければ読み込まない
        let mut exclude: Vec<PathBuf> = c.config.general.delete_folder.iter().cloned().collect();
        if !include_targets {
            exclude.extend(c.config.targets.iter().map(|t| t.path.clone()));
        }
        (exclude, c.config.general.delete_folder.clone(), c.config.general.sort_order)
    };
    let registry = state.registry.clone();
    let result =
        scanner::scan(&ScanOptions { root: source.clone(), include_subdirs, exclude, order }, |f| registry.supports(f))
            .map_err(|e| e.to_string())?;

    // 前回の仕分け元として覚えておく（設定ファイルに誤りがあって保存できなくても仕分けは続ける）
    remember(&state, |c| {
        c.remember_source(&source);
        c.general.include_subdirs = include_subdirs;
        c.general.include_targets = include_targets;
    });
    // 読み飛ばしたフォルダのうち、振り分け先だけを画面に伝える（削除フォルダを読まないのは当然なので伝えない）
    let trash_real = trash.as_deref().and_then(|t| dunce::canonicalize(t).ok());
    let excluded_targets: Vec<PathBuf> = result
        .excluded_dirs
        .into_iter()
        .filter(|d| !trash_real.as_ref().is_some_and(|t| dunce::canonicalize(d).is_ok_and(|d| d.starts_with(t))))
        .collect();

    state.cache.clear();
    state.grid_cache.clear();
    let generation = state.next_generation();
    let session = Session::new(Arc::new(fileops::RealFs), result.images, trash);
    let old = state.session().replace(SessionState {
        session,
        generation,
        source,
        unsupported: result.unsupported,
        excluded_targets,
    });
    if let Some(old) = old {
        // 前のフォルダで削除予定にしたものは、終了時の確認まで引き継ぐ
        if !old.session.pending_deletions().is_empty() {
            state.retired().push(old.session);
        }
    }
    with_session(&state, |_| Ok(())).map(|(_, v)| v)
}

#[tauri::command]
pub fn get_session(state: State<'_, AppState>) -> Option<SessionView> {
    with_session(&state, |_| Ok(())).ok().map(|(_, v)| v)
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActionArg {
    /// 設定の targets の番号
    Move {
        target: usize,
    },
    Skip,
    Delete,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    view: SessionView,
    message: Option<String>,
}

#[tauri::command]
pub fn perform(state: State<'_, AppState>, action: ActionArg) -> CmdResult<ActionResult> {
    let action = match action {
        ActionArg::Move { target } => {
            let c = state.config();
            let t = c.config.targets.get(target).ok_or("振り分け先が見つかりません")?;
            Action::MoveTo { dir: t.path.clone(), label: t.display_name() }
        }
        ActionArg::Skip => Action::Skip,
        ActionArg::Delete => Action::Delete,
    };
    let (_, view) = with_session(&state, |s| s.session.apply(action).map_err(|e| e.to_string()))?;
    Ok(ActionResult { view, message: None })
}

#[tauri::command]
pub fn resolve_conflict(state: State<'_, AppState>, choice: ConflictChoice) -> CmdResult<ActionResult> {
    let (_, view) = with_session(&state, |s| s.session.resolve_conflict(choice).map_err(|e| e.to_string()))?;
    Ok(ActionResult { view, message: None })
}

#[tauri::command]
pub fn cancel_conflict(state: State<'_, AppState>) -> CmdResult<SessionView> {
    with_session(&state, |s| {
        s.session.cancel_conflict();
        Ok(())
    })
    .map(|(_, v)| v)
}

#[tauri::command]
pub fn undo(state: State<'_, AppState>) -> CmdResult<ActionResult> {
    let (message, view) = with_session(&state, |s| s.session.undo().map_err(|e| e.to_string()))?;
    Ok(ActionResult { view, message: Some(message) })
}

#[tauri::command]
pub fn navigate(state: State<'_, AppState>, forward: bool) -> CmdResult<SessionView> {
    with_session(&state, |s| {
        if forward {
            s.session.go_next();
        } else {
            s.session.go_prev();
        }
        Ok(())
    })
    .map(|(_, v)| v)
}

/// 保留した画像などへ移動する（処理済みの画像には移動できない）
#[tauri::command]
pub fn jump_to(state: State<'_, AppState>, index: usize) -> CmdResult<SessionView> {
    with_session(&state, |s| {
        if s.session.go_to(index) {
            Ok(())
        } else {
            Err("この画像はすでに振り分け済みです（Ctrl+Z で取り消せます）".into())
        }
    })
    .map(|(_, v)| v)
}

/// 一覧に出す画像の状態
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GridState {
    Pending,
    Skipped,
    Moved,
    /// 削除フォルダへ移動済み、または元の場所で削除予定
    Deleted,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridItem {
    index: usize,
    name: String,
    state: GridState,
    /// 移動先の振り分け先の名前、または「削除予定」
    label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridList {
    generation: u64,
    current: Option<usize>,
    items: Vec<GridItem>,
}

/// 一覧表示用に、読み込んだすべての画像を並び順のまま返す
#[tauri::command]
pub fn list_items(state: State<'_, AppState>) -> CmdResult<GridList> {
    let targets = state.config().config.targets.clone();
    let guard = state.session();
    let s = guard.as_ref().ok_or("仕分け元フォルダが読み込まれていません")?;
    let items = s
        .session
        .items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let (state, label) = match &item.status {
                Status::Pending => (GridState::Pending, None),
                Status::Skipped => (GridState::Skipped, None),
                Status::Moved { to } => {
                    let dir = to.parent();
                    let label = match targets.iter().find(|t| dir == Some(t.path.as_path())) {
                        Some(t) => t.display_name(),
                        // 振り分け先から外したフォルダはフォルダ名で出す
                        None => dir.map(fileops::file_name).unwrap_or_default(),
                    };
                    (GridState::Moved, Some(label))
                }
                Status::Trashed { .. } | Status::Marked { .. } => (GridState::Deleted, Some("削除予定".into())),
            };
            GridItem { index, name: fileops::file_name(&item.path), state, label }
        })
        .collect();
    Ok(GridList { generation: s.generation, current: s.session.current(), items })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    #[serde(flatten)]
    file: FileView,
    width: u32,
    height: u32,
    /// EXIF の撮影日時
    taken: Option<String>,
}

/// 画像の大きさ・撮影日時など。デコードが必要なので表示とは別に取りに来る
#[tauri::command]
pub async fn image_info(state: State<'_, AppState>, generation: u64, index: usize) -> CmdResult<ImageInfo> {
    let (key, path) = {
        let guard = state.session();
        let s = guard.as_ref().filter(|s| s.generation == generation).ok_or("古い画面からの要求です")?;
        let item = s.session.items().get(index).ok_or("画像が見つかりません")?;
        (item.path.clone(), location(&item.path, &item.status).to_path_buf())
    };
    let registry = state.registry.clone();
    let cache = state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let preview = cache.get_or_load(&key, || decoder::make_preview(&registry, &path))?;
        Ok(ImageInfo {
            file: file_view(&path),
            width: preview.width,
            height: preview.height,
            taken: decoder::taken_at(&path),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- 終了時の削除確認 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionSummary {
    items: Vec<PendingDeletion>,
    trash_dir: Option<String>,
    on_exit: config::OnExit,
}

#[tauri::command]
pub fn pending_deletions(state: State<'_, AppState>) -> DeletionSummary {
    let (trash_dir, on_exit) = {
        let c = state.config();
        (c.config.general.delete_folder.as_deref().map(lossy), c.config.general.on_exit)
    };
    let current = state.session();
    let mut items: Vec<PendingDeletion> = state.retired().iter().flat_map(|s| s.pending_deletions()).collect();
    if let Some(s) = current.as_ref() {
        items.extend(s.session.pending_deletions());
    }
    DeletionSummary { items, trash_dir, on_exit }
}

/// 削除予定のファイルを完全削除する。すべて消せたらアプリを終了する
#[tauri::command]
pub fn finalize_and_exit(app: AppHandle, state: State<'_, AppState>, delete: bool) -> FinalizeReport {
    let report = if delete { finalize_all(&state) } else { FinalizeReport::default() };
    if report.failed.is_empty() {
        exit_app(app, state);
    }
    report
}

/// 切り替え前のセッションも含め、削除予定のファイルをすべて完全削除する
fn finalize_all(state: &AppState) -> FinalizeReport {
    let mut report = FinalizeReport::default();
    // ロックは常に session → retired の順で取る
    let mut current = state.session();
    let mut sessions = state.retired();
    for s in sessions.iter_mut().chain(current.as_mut().map(|c| &mut c.session)) {
        let r = s.finalize_deletions();
        report.deleted.extend(r.deleted);
        report.failed.extend(r.failed);
    }
    report
}

#[tauri::command]
pub fn exit_app(app: AppHandle, state: State<'_, AppState>) {
    state.allow_exit.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Target;
    use crate::decoder::tests::write_dummy;
    use tauri::test::{mock_app, MockRuntime};
    use tauri::{App, Manager};
    use tempfile::TempDir;

    /// 一時ディレクトリに仕分け元（ダミー画像 3 枚）と振り分け先 2 つを用意する
    struct Fixture {
        dir: TempDir,
        app: App<MockRuntime>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src");
            for d in ["src", "a", "b"] {
                std::fs::create_dir_all(dir.path().join(d)).unwrap();
            }
            for name in ["1.png", "2.png", "3.png"] {
                write_dummy(&src.join(name), 8, 6);
            }
            std::fs::write(src.join("memo.txt"), "画像ではない").unwrap();
            let app = mock_app();
            app.manage(AppState::new(dir.path().join("conf").join("config.toml")));
            let f = Fixture { dir, app };
            let mut config = f.config().config;
            config.targets = vec![f.target("1", "a"), f.target("2", "b")];
            save_config(f.state(), config).unwrap();
            f
        }

        fn state(&self) -> State<'_, AppState> {
            self.app.state::<AppState>()
        }

        fn config(&self) -> ConfigPayload {
            get_config(self.state())
        }

        /// `a/1.png` のように書いた相対パス。Windows でも区切りがそろうよう要素ごとにつなぐ
        fn path(&self, rel: &str) -> PathBuf {
            rel.split('/').fold(self.dir.path().to_path_buf(), |p, part| p.join(part))
        }

        fn target(&self, key: &str, rel: &str) -> Target {
            Target { key: key.into(), name: String::new(), path: self.path(rel) }
        }

        fn start(&self) -> SessionView {
            start_session(self.state(), lossy(&self.path("src")), false, false).unwrap()
        }

        fn act(&self, action: ActionArg) -> SessionView {
            perform(self.state(), action).unwrap().view
        }

        fn set_delete_folder(&self, rel: &str) {
            let mut config = self.config().config;
            config.general.delete_folder = Some(self.path(rel));
            std::fs::create_dir_all(self.path(rel)).unwrap();
            save_config(self.state(), config).unwrap();
        }
    }

    fn current_name(v: &SessionView) -> Option<&str> {
        v.current.as_ref().map(|c| c.name.as_str())
    }

    #[test]
    fn session_commands_fail_before_loading_a_folder() {
        let f = Fixture::new();
        assert!(get_session(f.state()).is_none());
        let err = perform(f.state(), ActionArg::Skip).err().unwrap();
        assert!(err.contains("読み込まれていません"), "{err}");
        assert!(undo(f.state()).is_err());
        assert!(navigate(f.state(), true).is_err());
        assert!(pending_deletions(f.state()).items.is_empty());
    }

    #[test]
    fn start_session_scans_images_and_remembers_the_source() {
        let f = Fixture::new();
        let v = f.start();
        assert_eq!((v.total, v.remaining, v.skipped), (3, 3, 0));
        assert_eq!(current_name(&v), Some("1.png"));
        assert_eq!(v.upcoming, vec![1, 2]);
        assert_eq!(v.moved_counts, vec![0, 0]);
        assert!(!v.can_undo);

        // 次回の起動で同じフォルダを開けるよう、設定ファイルに書き戻している
        let saved = config::load_or_create(&f.state().config_path).unwrap();
        assert_eq!(saved.general.source_dir, Some(f.path("src")));
        assert_eq!(saved.general.recent_sources, vec![f.path("src")], "履歴にも残す");
        assert_eq!(get_session(f.state()).unwrap().generation, v.generation);
    }

    #[test]
    fn start_session_reports_missing_folder() {
        let f = Fixture::new();
        assert!(start_session(f.state(), lossy(&f.path("nowhere")), false, false).is_err());
    }

    #[test]
    fn move_skip_delete_and_undo_round_trip() {
        let f = Fixture::new();
        f.start();

        let v = f.act(ActionArg::Move { target: 1 });
        assert!(f.path("b/1.png").exists() && !f.path("src/1.png").exists());
        assert_eq!(v.moved_counts, vec![0, 1]);
        assert_eq!(current_name(&v), Some("2.png"));

        let v = f.act(ActionArg::Skip);
        assert_eq!((v.remaining, v.skipped), (2, 1));
        assert_eq!(v.next_skipped, Some(1));

        // 削除フォルダ未指定: ファイルは動かさず削除予定として記録するだけ
        let v = f.act(ActionArg::Delete);
        assert!(f.path("src/3.png").exists());
        assert_eq!(v.pending_deletions, 1);
        assert_eq!(v.history.len(), 3);
        let summary = pending_deletions(f.state());
        assert_eq!(summary.items.len(), 1);
        assert!(summary.trash_dir.is_none());

        let r = undo(f.state()).unwrap();
        assert!(r.message.unwrap().contains("3.png"));
        assert_eq!(r.view.pending_deletions, 0);
        undo(f.state()).unwrap();
        let r = undo(f.state()).unwrap();
        assert!(f.path("src/1.png").exists() && !f.path("b/1.png").exists());
        assert!(!r.view.can_undo);
        assert_eq!(current_name(&r.view), Some("1.png"));
        assert!(undo(f.state()).is_err());
    }

    #[test]
    fn list_items_reports_every_image_with_its_state() {
        let f = Fixture::new();
        f.start();
        f.act(ActionArg::Move { target: 0 }); // 1.png → a
        f.act(ActionArg::Skip); // 2.png は保留
        let v = f.act(ActionArg::Delete); // 3.png は削除予定（削除フォルダ未指定）
        let list = list_items(f.state()).unwrap();
        let got: Vec<_> = list.items.iter().map(|i| (i.index, i.name.as_str(), i.state, i.label.as_deref())).collect();
        assert_eq!(
            got,
            [
                (0, "1.png", GridState::Moved, Some("a")),
                (1, "2.png", GridState::Skipped, None),
                (2, "3.png", GridState::Deleted, Some("削除予定")),
            ]
        );
        assert_eq!(list.generation, v.generation);
        assert_eq!(list.current, v.current.map(|c| c.index));
    }

    #[test]
    fn list_items_labels_moves_outside_the_targets_by_folder_name() {
        let f = Fixture::new();
        f.start();
        f.act(ActionArg::Move { target: 1 }); // 1.png → b
                                              // 振り分け先から外しても、移動先のフォルダ名で表示する
        let mut config = f.config().config;
        config.targets.truncate(1);
        save_config(f.state(), config).unwrap();
        let list = list_items(f.state()).unwrap();
        assert_eq!(list.items[0].label.as_deref(), Some("b"));
        assert_eq!(list.items[1].state, GridState::Pending);
    }

    #[test]
    fn list_items_fails_before_loading_a_folder() {
        assert!(list_items(Fixture::new().state()).is_err());
    }

    #[test]
    fn unknown_target_is_rejected_without_touching_files() {
        let f = Fixture::new();
        f.start();
        assert!(perform(f.state(), ActionArg::Move { target: 9 }).is_err());
        assert!(f.path("src/1.png").exists());
        assert!(!get_session(f.state()).unwrap().can_undo);
    }

    #[test]
    fn delete_moves_into_configured_delete_folder() {
        let f = Fixture::new();
        f.set_delete_folder("trash");
        f.start();
        f.act(ActionArg::Delete);
        assert!(f.path("trash/1.png").exists() && !f.path("src/1.png").exists());
        let summary = pending_deletions(f.state());
        assert_eq!(summary.trash_dir, Some(lossy(&f.path("trash"))));
        assert!(summary.items[0].in_trash);
    }

    #[test]
    fn delete_folder_change_applies_to_running_session() {
        let f = Fixture::new();
        f.start();
        f.set_delete_folder("trash");
        f.act(ActionArg::Delete);
        assert!(f.path("trash/1.png").exists());
    }

    #[test]
    fn conflict_is_shown_and_can_keep_both() {
        let f = Fixture::new();
        write_dummy(&f.path("a/1.png"), 4, 4);
        f.start();
        let v = f.act(ActionArg::Move { target: 0 });
        let c = v.conflict.expect("同名ファイルの確認になるはず");
        assert_eq!(c.incoming.name, "1.png");
        assert_eq!(c.existing.path, lossy(&f.path("a/1.png")));
        assert!(c.existing.size.unwrap() > 0 && c.existing.modified.is_some());

        let r = resolve_conflict(f.state(), ConflictChoice::KeepBoth).unwrap();
        assert!(r.view.conflict.is_none());
        assert!(!f.path("src/1.png").exists());
        let in_a = std::fs::read_dir(f.path("a")).unwrap().count();
        assert_eq!(in_a, 2, "既存と移動したファイルの両方が残る");
        assert_eq!(current_name(&r.view), Some("2.png"));
    }

    #[test]
    fn conflict_can_be_cancelled() {
        let f = Fixture::new();
        write_dummy(&f.path("a/1.png"), 4, 4);
        f.start();
        assert!(f.act(ActionArg::Move { target: 0 }).conflict.is_some());
        let v = cancel_conflict(f.state()).unwrap();
        assert!(v.conflict.is_none());
        assert_eq!(current_name(&v), Some("1.png"));
        assert!(f.path("src/1.png").exists());
    }

    #[test]
    fn navigate_and_jump_back_to_skipped_image() {
        let f = Fixture::new();
        f.start();
        f.act(ActionArg::Skip);
        f.act(ActionArg::Move { target: 0 });
        assert_eq!(current_name(&navigate(f.state(), false).unwrap()), Some("1.png"));
        assert_eq!(current_name(&navigate(f.state(), true).unwrap()), Some("3.png"));
        assert_eq!(current_name(&jump_to(f.state(), 0).unwrap()), Some("1.png"));
        let err = jump_to(f.state(), 1).err().unwrap();
        assert!(err.contains("振り分け済み"), "{err}");
    }

    #[test]
    fn pending_deletions_survive_switching_source_and_are_finalized() {
        let f = Fixture::new();
        f.start();
        f.act(ActionArg::Delete);
        // 別のフォルダに切り替えても、前のフォルダの削除予定は終了時まで残す
        std::fs::create_dir_all(f.path("other")).unwrap();
        write_dummy(&f.path("other/x.png"), 4, 4);
        let v = start_session(f.state(), lossy(&f.path("other")), false, false).unwrap();
        assert_eq!(v.total, 1);
        assert_eq!(v.pending_deletions, 1);
        f.act(ActionArg::Delete);
        assert_eq!(pending_deletions(f.state()).items.len(), 2);

        let report = finalize_all(&f.state());
        assert_eq!(report.deleted.len(), 2);
        assert!(report.failed.is_empty());
        assert!(!f.path("src/1.png").exists() && !f.path("other/x.png").exists());
        // 削除予定にしていないファイルは残る
        assert!(f.path("src/2.png").exists() && f.path("src/3.png").exists());
    }

    #[test]
    fn invalid_config_is_not_saved() {
        let f = Fixture::new();
        let mut config = f.config().config;
        config.targets.push(f.target("1", "b"));
        let err = save_config(f.state(), config.clone()).err().unwrap();
        assert!(err.contains("重複"), "{err}");
        assert_eq!(f.config().config.targets.len(), 2);
        assert!(validate_config(config).iter().any(|i| i.message.contains("重複")));
    }

    #[test]
    fn reload_picks_up_hand_edits() {
        let f = Fixture::new();
        let path = f.state().config_path.clone();
        let text = std::fs::read_to_string(&path).unwrap().replace("prefetch = 4", "prefetch = 7");
        assert!(text.contains("prefetch = 7"), "既定値が変わったらテストを直す");
        std::fs::write(&path, text).unwrap();
        assert_eq!(reload_config(f.state()).unwrap().config.general.prefetch, 7);

        std::fs::write(&path, "[general\n").unwrap();
        assert!(reload_config(f.state()).is_err());
        assert_eq!(f.config().config.general.prefetch, 7, "読めなければメモリ上の設定はそのまま");
    }

    #[test]
    fn view_toggles_are_written_back() {
        let f = Fixture::new();
        let shown = f.config().config.general.show_paths;
        assert_eq!(set_show_paths(f.state(), !shown).config.general.show_paths, !shown);
        let mode = config::ViewMode::Focus;
        assert_eq!(set_view_mode(f.state(), mode).config.general.view_mode, mode);
        let saved = config::load_or_create(&f.state().config_path).unwrap();
        assert_eq!((saved.general.show_paths, saved.general.view_mode), (!shown, mode));
    }

    #[test]
    fn forget_source_removes_it_from_the_history() {
        let f = Fixture::new();
        f.start();
        let payload = forget_source(f.state(), lossy(&f.path("src")));
        assert!(payload.config.general.recent_sources.is_empty());
        assert_eq!(payload.config.general.source_dir, None);
        let saved = config::load_or_create(&f.state().config_path).unwrap();
        assert!(saved.general.recent_sources.is_empty(), "設定ファイルにも書き戻す");
    }

    #[test]
    fn view_toggles_stay_in_memory_when_file_is_broken() {
        let f = Fixture::new();
        std::fs::write(&f.state().config_path, "[general\n").unwrap();
        let mode = config::ViewMode::Focus;
        assert_eq!(set_view_mode(f.state(), mode).config.general.view_mode, mode);
        assert_eq!(std::fs::read_to_string(&f.state().config_path).unwrap(), "[general\n");
    }

    #[test]
    fn broken_config_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "targets = 1\n").unwrap();
        let app = mock_app();
        app.manage(AppState::new(path));
        let payload = get_config(app.state::<AppState>());
        assert!(payload.load_error.is_some());
        assert!(payload.config.targets.is_empty());
        assert!(!payload.formats.is_empty());
    }

    #[test]
    fn list_subfolders_sorts_naturally_and_hides_dot_folders() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["10 夏", "2 春", ".hidden"] {
            std::fs::create_dir(dir.path().join(d)).unwrap();
        }
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let names: Vec<String> =
            list_subfolders(lossy(dir.path())).unwrap().iter().map(|p| fileops::file_name(Path::new(p))).collect();
        assert_eq!(names, ["2 春", "10 夏"]);
        assert!(list_subfolders(lossy(&dir.path().join("none"))).is_err());
    }

    #[test]
    fn image_info_reads_size_and_rejects_stale_requests() {
        let f = Fixture::new();
        let v = f.start();
        let info = tauri::async_runtime::block_on(image_info(f.state(), v.generation, 0)).unwrap();
        assert_eq!((info.width, info.height), (8, 6));
        assert_eq!(info.file.name, "1.png");
        assert!(info.taken.is_none());

        let stale = tauri::async_runtime::block_on(image_info(f.state(), v.generation + 100, 0));
        assert!(stale.err().unwrap().contains("古い画面"));
        assert!(tauri::async_runtime::block_on(image_info(f.state(), v.generation, 99)).is_err());
    }

    // ---- 振り分け先フォルダを含める／読み飛ばしたフォルダを伝える ----

    /// 振り分け先 a・b と削除フォルダ trash を中に持つ上位フォルダを、サブフォルダ込みで読み込む
    fn start_parent(f: &Fixture, include_targets: bool) -> SessionView {
        f.set_delete_folder("trash");
        write_dummy(&f.path("a/sorted.png"), 8, 6);
        std::fs::create_dir_all(f.path("b/deep")).unwrap();
        write_dummy(&f.path("b/deep/sorted2.png"), 8, 6);
        write_dummy(&f.path("trash/gone.png"), 8, 6);
        start_session(f.state(), lossy(f.dir.path()), true, include_targets).unwrap()
    }

    fn dir_names(dirs: &[String]) -> Vec<String> {
        dirs.iter().map(|d| fileops::file_name(Path::new(d))).collect()
    }

    #[test]
    fn target_folders_are_skipped_by_default_and_reported() {
        let f = Fixture::new();
        let v = start_parent(&f, false);
        assert_eq!(v.total, 3, "仕分け元の 3 枚だけ");
        assert_eq!(
            dir_names(&v.excluded_targets),
            ["a", "b"],
            "読み飛ばした振り分け先を伝える（削除フォルダは含めない）"
        );
        assert!(!f.config().config.general.include_targets);
    }

    #[test]
    fn target_folders_are_loaded_when_asked_but_never_the_delete_folder() {
        let f = Fixture::new();
        let v = start_parent(&f, true);
        assert_eq!(v.total, 5, "振り分け先の 2 枚も読み込む。削除フォルダの 1 枚は読み込まない");
        assert!(v.excluded_targets.is_empty());
        let names: Vec<_> =
            list_items(f.state()).unwrap().items.iter().map(|i| i.name.clone()).filter(|n| n != "conf").collect();
        assert!(names.contains(&"sorted.png".to_string()) && names.contains(&"sorted2.png".to_string()));
        assert!(!names.contains(&"gone.png".to_string()));
        // 次回のために設定に覚える
        let saved = config::load_or_create(&f.state().config_path).unwrap();
        assert!(saved.general.include_targets && saved.general.include_subdirs);
    }

    #[test]
    fn an_image_already_in_its_target_is_not_moved() {
        let f = Fixture::new();
        start_parent(&f, true);
        // a/sorted.png を見て、振り分け先 a のキーを押す
        let items = list_items(f.state()).unwrap().items;
        let sorted = items.iter().find(|i| i.name == "sorted.png").unwrap().index;
        jump_to(f.state(), sorted).unwrap();
        let v = f.act(ActionArg::Move { target: 0 });
        assert!(f.path("a/sorted.png").exists(), "同じフォルダへは動かさない");
        assert!(v.conflict.is_none(), "自分自身との同名衝突にしない");
        assert_eq!(v.moved_counts, vec![1, 0]);
    }

    #[test]
    fn nothing_is_reported_without_subfolders() {
        let f = Fixture::new();
        f.set_delete_folder("trash");
        let v = start_session(f.state(), lossy(f.dir.path()), false, false).unwrap();
        assert!(v.excluded_targets.is_empty());
    }
}
