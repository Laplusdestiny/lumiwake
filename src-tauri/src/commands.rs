//! 画面（WebView）から呼ばれる Tauri コマンド。
//!
//! 画面はキー入力をここへ渡すだけで、ファイル操作の整合性は `fileops` が持つ。

use crate::config::{self, Config, Issue};
use crate::decoder::{self, FormatSupport};
use crate::fileops::{
    self, Action, ConflictChoice, DeletionReason, FinalizeReport, HistoryKind, PendingDeletion, Session, Status,
};
use crate::scanner::{self, ScanOptions};
use crate::similar::group::Candidate;
use crate::similar::scan::{ScanJob, ScanStatus};
use crate::similar::{self, MAX_DISTANCE_LIMIT};
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
            // 類似判定の結果はパスで持っているので、番号が変わっても使い続ける
            state.similar.renumber(s.generation);
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
pub fn start_session(state: State<'_, AppState>, source: String, include_subdirs: bool) -> CmdResult<SessionView> {
    let source = PathBuf::from(source);
    let (exclude, trash, order) = {
        let c = state.config();
        let mut exclude: Vec<PathBuf> = c.config.targets.iter().map(|t| t.path.clone()).collect();
        exclude.extend(c.config.general.delete_folder.clone());
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
    });

    state.cache.clear();
    state.grid_cache.clear();
    let generation = state.next_generation();
    state.similar.reset(generation);
    let session = Session::new(Arc::new(fileops::RealFs), result.images, trash);
    let old = state.session().replace(SessionState { session, generation, source, unsupported: result.unsupported });
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

// ---- 類似画像の確認 ----

const STALE_SIMILAR: &str = "画像の並びが変わりました。類似画像を開き直してください";
const INVALID_SELECTION: &str = "選んだ画像の状態が変わりました。もう一度選んでください";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarMember {
    index: usize,
    name: String,
    width: u32,
    height: u32,
    size: u64,
    /// グループ内で最も近い画像との類似度（%）
    similarity: u8,
    largest_resolution: bool,
    largest_size: bool,
    /// 仕分け画面で見ている画像
    current: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarGroup {
    members: Vec<SimilarMember>,
    exact: bool,
    same_size: bool,
    same_resolution: bool,
    similar_names: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarGroups {
    generation: u64,
    groups: Vec<SimilarGroup>,
}

/// まだ振り分けていない画像の類似判定を、バックグラウンドで始める（判定済みの画像は読み直さない）
#[tauri::command]
pub fn start_similar_scan(state: State<'_, AppState>) -> CmdResult<ScanStatus> {
    let (generation, jobs) = {
        let guard = state.session();
        let s = guard.as_ref().ok_or("仕分け元フォルダが読み込まれていません")?;
        let jobs = s
            .session
            .items()
            .iter()
            .filter(|item| item.status.is_open())
            .map(|item| ScanJob { key: item.path.clone(), path: location(&item.path, &item.status).to_path_buf() })
            .collect();
        (s.generation, jobs)
    };
    let workers = similar::scan_workers(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2));
    let registry = state.registry.clone();
    Ok(state.similar.start(generation, jobs, workers, Arc::new(move |p: &Path| similar::fingerprint(&registry, p))))
}

#[tauri::command]
pub fn similar_status(state: State<'_, AppState>) -> ScanStatus {
    state.similar.status()
}

/// 判定を終えた画像のうち、まだ振り分けていないものを類似グループにまとめる（判定の途中でも呼べる）。
/// 完全一致の確認でファイルを読むことがあるので、画面を止めないよう非同期のコマンドにする
#[tauri::command]
pub async fn similar_groups(state: State<'_, AppState>, max_distance: u32) -> CmdResult<SimilarGroups> {
    // ファイルを読み比べる間（完全一致の確認）にセッションを止めないよう、必要な情報を写してから判定する
    let (generation, current, candidates) = {
        let guard = state.session();
        let s = guard.as_ref().ok_or("仕分け元フォルダが読み込まれていません")?;
        let candidates: Vec<Candidate> = s
            .session
            .items()
            .iter()
            .enumerate()
            .filter(|(_, item)| item.status.is_open())
            .filter_map(|(index, item)| {
                let print = state.similar.get(&item.path)?;
                Some(Candidate { index, path: item.path.clone(), print })
            })
            .collect();
        (s.generation, s.session.current(), candidates)
    };
    let by_index = |index: usize| candidates.iter().find(|c| c.index == index);
    // 一度読み比べた組は覚えておく（閾値を動かすたび・操作のたびに読み直さない）
    let same_bytes = |a: &Path, b: &Path| state.similar.exact(a, b, || similar::group::same_bytes(a, b));
    let groups = similar::group::group(&candidates, max_distance.min(MAX_DISTANCE_LIMIT), same_bytes)
        .into_iter()
        .map(|g| SimilarGroup {
            members: g
                .members
                .iter()
                .filter_map(|m| {
                    let c = by_index(m.index)?;
                    Some(SimilarMember {
                        index: m.index,
                        name: fileops::file_name(&c.path),
                        width: c.print.width,
                        height: c.print.height,
                        size: c.print.size,
                        similarity: m.similarity,
                        largest_resolution: m.largest_resolution,
                        largest_size: m.largest_size,
                        current: current == Some(m.index),
                    })
                })
                .collect(),
            exact: g.exact,
            same_size: g.same_size,
            same_resolution: g.same_resolution,
            similar_names: g.similar_names,
        })
        .collect();
    Ok(SimilarGroups { generation, groups })
}

/// 画面が持っている画像の番号が今のものか（並び順を変えると番号が変わる）
fn check_generation(s: &SessionState, generation: u64) -> Result<(), String> {
    if s.generation == generation {
        Ok(())
    } else {
        Err(STALE_SIMILAR.into())
    }
}

/// 類似画像の確認画面から、番号で指定した 1 枚を削除予定にする
#[tauri::command]
pub fn delete_similar(state: State<'_, AppState>, generation: u64, index: usize) -> CmdResult<ActionResult> {
    let (_, view) = with_session(&state, |s| {
        check_generation(s, generation)?;
        s.session.delete_at(index, DeletionReason::Similar).map_err(|e| e.to_string())
    })?;
    Ok(ActionResult { view, message: None })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepOnlyResult {
    view: SessionView,
    /// 削除予定にした枚数
    deleted: usize,
    /// 途中で失敗したときの理由（そこまでの分は削除予定のまま。取り消しで戻せる）
    error: Option<String>,
}

/// `keep` を残し、同じグループの `others` を削除予定にする。
/// 対象が 1 枚でもおかしければ何もしない。途中で失敗したらそこで止める
#[tauri::command]
pub fn keep_only(
    state: State<'_, AppState>,
    generation: u64,
    keep: usize,
    others: Vec<usize>,
) -> CmdResult<KeepOnlyResult> {
    let ((deleted, error), view) = with_session(&state, |s| {
        check_generation(s, generation)?;
        let open = |i: &usize| s.session.items().get(*i).is_some_and(|item| item.status.is_open());
        let distinct = others.iter().enumerate().all(|(n, i)| *i != keep && !others[..n].contains(i));
        if others.is_empty() || !distinct || !open(&keep) || !others.iter().all(open) {
            return Err(INVALID_SELECTION.into());
        }
        let mut deleted = 0;
        for &index in &others {
            if let Err(e) = s.session.delete_at(index, DeletionReason::Similar) {
                return Ok((deleted, Some(e.to_string())));
            }
            deleted += 1;
        }
        Ok((deleted, None))
    })?;
    Ok(KeepOnlyResult { view, deleted, error })
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
            start_session(self.state(), lossy(&self.path("src")), false).unwrap()
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
        assert!(start_session(f.state(), lossy(&f.path("nowhere")), false).is_err());
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
        let v = start_session(f.state(), lossy(&f.path("other")), false).unwrap();
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

    // ---- 類似画像 ----

    use crate::fileops::DeletionReason;
    use crate::similar::hash::tests::scene;
    use crate::similar::scan::ScanStatus;
    use crate::similar::MAX_DISTANCE_LIMIT;
    use std::time::{Duration, Instant};

    fn groups_at(f: &Fixture, max_distance: u32) -> CmdResult<SimilarGroups> {
        tauri::async_runtime::block_on(similar_groups(f.state(), max_distance))
    }

    impl Fixture {
        /// 既定の 3 枚（中身がまったく同じ 1.png〜3.png）に、同じ絵の大小 2 枚を足して読み込む。
        /// 並びは 1.png, 2.png, 3.png, dup-a.png, dup-b.png（番号 0〜4）
        fn start_with_duplicates(&self) -> SessionView {
            let big = scene(7, 160, 120);
            big.save(self.path("src/dup-a.png")).unwrap();
            image::imageops::resize(&big, 80, 60, image::imageops::FilterType::Triangle)
                .save(self.path("src/dup-b.png"))
                .unwrap();
            self.start()
        }

        /// 判定を始めて、終わるまで待つ
        fn scan(&self) -> ScanStatus {
            start_similar_scan(self.state()).unwrap();
            let start = Instant::now();
            loop {
                let st = similar_status(self.state());
                if !st.running {
                    return st;
                }
                assert!(start.elapsed() < Duration::from_secs(20), "判定が終わらない: {st:?}");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn groups(&self) -> Vec<Vec<usize>> {
            let list = groups_at(self, crate::similar::DEFAULT_MAX_DISTANCE).unwrap();
            list.groups.iter().map(|g| g.members.iter().map(|m| m.index).collect()).collect()
        }

        fn pending(&self) -> Vec<PendingDeletion> {
            pending_deletions(self.state()).items
        }

        /// 読み込んだ画像のパス。スキャナは仕分け元を実体のパスにそろえるので、同じようにそろえて比べる
        /// （Windows の一時フォルダは `RUNNER~1` のような短い名前で渡されることがある）
        fn scanned(&self, rel: &str) -> PathBuf {
            dunce::canonicalize(self.path(rel)).unwrap()
        }
    }

    #[test]
    fn similar_commands_fail_before_loading_a_folder() {
        let f = Fixture::new();
        assert!(start_similar_scan(f.state()).is_err());
        assert!(groups_at(&f, 6).is_err());
        assert!(delete_similar(f.state(), 1, 0).is_err());
        assert!(keep_only(f.state(), 1, 0, vec![1]).is_err());
        let st = similar_status(f.state());
        assert_eq!((st.done, st.total, st.running), (0, 0, false), "状態は読み込み前でも返す");
    }

    #[test]
    fn scan_then_groups_report_members_and_evidence() {
        let f = Fixture::new();
        let v = f.start_with_duplicates();
        let st = f.scan();
        assert_eq!((st.generation, st.done, st.total, st.failed), (v.generation, 5, 5, 0));

        let list = groups_at(&f, crate::similar::DEFAULT_MAX_DISTANCE).unwrap();
        assert_eq!(list.generation, v.generation);
        assert_eq!(f.groups(), [vec![0, 1, 2], vec![3, 4]]);

        // 中身がまったく同じ連番の 3 枚
        let same = &list.groups[0];
        assert!(same.exact && same.same_size && same.same_resolution && same.similar_names);
        let m = &same.members[0];
        assert_eq!((m.name.as_str(), m.width, m.height, m.similarity), ("1.png", 8, 6, 100));
        assert_eq!(m.size, std::fs::metadata(f.path("src/1.png")).unwrap().len());
        assert!(m.current && !same.members[1].current, "見ている画像に印を付ける");
        assert!(same.members.iter().all(|m| !m.largest_resolution && !m.largest_size));

        // 同じ絵を縮めたもの
        let resized = &list.groups[1];
        assert!(!resized.exact && !resized.same_size && !resized.same_resolution && !resized.similar_names);
        assert_eq!(resized.members[0].name, "dup-a.png");
        assert!(resized.members[0].largest_resolution && resized.members[0].largest_size);
        assert!(!resized.members[1].largest_resolution);
        assert!(resized.members[0].similarity >= 91);
    }

    #[test]
    fn groups_use_only_what_has_been_scanned_and_skip_unreadable_images() {
        let f = Fixture::new();
        std::fs::write(f.path("src/broken.png"), "画像ではない").unwrap();
        f.start();
        assert!(f.groups().is_empty(), "判定の前はグループなし");
        let st = f.scan();
        assert_eq!((st.done, st.total, st.failed), (4, 4, 1), "読めない画像は失敗に数える");
        assert_eq!(f.groups(), [vec![0, 1, 2]], "読めた画像だけでグループにする");
    }

    #[test]
    fn groups_leave_out_processed_images() {
        let f = Fixture::new();
        let v = f.start_with_duplicates();
        f.scan();
        f.act(ActionArg::Move { target: 0 }); // 1.png を振り分ける
        assert_eq!(f.groups(), [vec![1, 2], vec![3, 4]]);
        delete_similar(f.state(), v.generation, 1).unwrap();
        assert_eq!(f.groups(), [vec![3, 4]], "1 枚になったグループは出さない");
        // 移動した画像は、もう一度判定を始めても対象にしない
        assert_eq!(f.scan().total, 5);
        assert_eq!(f.groups(), [vec![3, 4]]);
    }

    #[test]
    fn max_distance_is_clamped_to_the_limit() {
        let f = Fixture::new();
        f.start_with_duplicates();
        f.scan();
        let count = |d: u32| groups_at(&f, d).unwrap().groups.len();
        assert_eq!(count(999), count(MAX_DISTANCE_LIMIT));
        assert!(count(0) >= 1, "距離 0 でも中身が同じ 3 枚はグループになる");
    }

    #[test]
    fn delete_similar_marks_the_image_and_can_be_undone() {
        let f = Fixture::new();
        let v = f.start_with_duplicates();
        f.scan();
        let r = delete_similar(f.state(), v.generation, 2).unwrap();
        assert_eq!(r.view.pending_deletions, 1);
        assert_eq!(current_name(&r.view), Some("1.png"), "見ている画像は変えない");
        assert!(r.view.can_undo);
        let pending = f.pending();
        assert_eq!((pending[0].path.clone(), pending[0].reason), (f.scanned("src/3.png"), DeletionReason::Similar));
        assert!(f.path("src/3.png").exists(), "ファイルは消さない");

        undo(f.state()).unwrap();
        assert!(f.pending().is_empty());
        assert_eq!(f.groups()[0], [0, 1, 2]);

        // 処理済みの画像・範囲外は断る（取り消した 3.png を見ているので、それを振り分ける）
        let moved = f.act(ActionArg::Move { target: 0 });
        assert!(f.path("a/3.png").exists() && moved.pending_deletions == 0);
        assert!(delete_similar(f.state(), v.generation, 2).err().unwrap().contains("振り分け済み"));
        assert!(delete_similar(f.state(), v.generation, 99).is_err());
    }

    #[test]
    fn delete_similar_moves_to_the_delete_folder() {
        let f = Fixture::new();
        f.set_delete_folder("trash");
        let v = f.start_with_duplicates();
        delete_similar(f.state(), v.generation, 4).unwrap();
        assert!(f.path("trash/dup-b.png").exists());
        assert!(!f.path("src/dup-b.png").exists());
        undo(f.state()).unwrap();
        assert!(f.path("src/dup-b.png").exists());
    }

    #[test]
    fn keep_only_deletes_the_rest_of_the_group() {
        let f = Fixture::new();
        let v = f.start_with_duplicates();
        f.scan();
        let r = keep_only(f.state(), v.generation, 1, vec![0, 2]).unwrap();
        assert_eq!((r.deleted, r.error.as_deref()), (2, None));
        assert_eq!(r.view.pending_deletions, 2);
        assert_eq!(current_name(&r.view), Some("2.png"), "見ていた画像を消したので、残した画像へ進む");
        let pending = f.pending();
        assert_eq!(
            pending.iter().map(|p| p.path.clone()).collect::<Vec<_>>(),
            [f.scanned("src/1.png"), f.scanned("src/3.png")]
        );
        assert!(pending.iter().all(|p| p.reason == DeletionReason::Similar));
        assert_eq!(f.groups(), [vec![3, 4]]);

        // 取り消しは 1 枚ずつ
        undo(f.state()).unwrap();
        assert_eq!(f.pending().len(), 1);
        undo(f.state()).unwrap();
        assert!(f.pending().is_empty());
    }

    #[test]
    fn keep_only_validates_before_deleting_anything() {
        let f = Fixture::new();
        let v = f.start_with_duplicates();
        let g = v.generation;
        f.act(ActionArg::Skip); // 1.png は保留（対象にできる）
        f.act(ActionArg::Move { target: 0 }); // 2.png は処理済み
        let rejected = [
            (0, vec![2, 0]),  // 残す画像が削除する側にも入っている
            (0, vec![2, 2]),  // 重複
            (0, vec![2, 1]),  // 処理済みを含む
            (1, vec![0, 2]),  // 残す画像が処理済み
            (0, vec![2, 99]), // 範囲外
            (0, vec![]),      // 削除する画像がない
        ];
        for (keep, others) in rejected {
            let err = keep_only(f.state(), g, keep, others.clone()).err();
            assert!(err.is_some_and(|e| e.contains("もう一度選んでください")), "{keep} {others:?}");
        }
        assert!(f.pending().is_empty(), "1 枚も削除予定にしない");
        // 保留の画像は対象にできる
        assert_eq!(keep_only(f.state(), g, 2, vec![0]).unwrap().deleted, 1);
    }

    #[test]
    fn keep_only_stops_at_the_first_failure_and_reports_it() {
        let f = Fixture::new();
        f.set_delete_folder("trash");
        let v = f.start_with_duplicates();
        std::fs::remove_dir(f.path("trash")).unwrap();
        let r = keep_only(f.state(), v.generation, 0, vec![1, 2]).unwrap();
        assert_eq!(r.deleted, 0);
        assert!(r.error.as_deref().is_some_and(|e| e.contains("見つかりません")), "{:?}", r.error);
        assert!(f.pending().is_empty());
        assert!(f.path("src/2.png").exists() && f.path("src/3.png").exists());
    }

    #[test]
    fn stale_generation_is_rejected_after_reordering() {
        let f = Fixture::new();
        let old = f.start_with_duplicates();
        f.scan();
        // 並び順を変えると画像の番号が変わる（番号 0 は dup-b.png になる）
        let mut config = f.config().config;
        config.general.sort_order = config::SortOrder::NameDesc;
        save_config(f.state(), config).unwrap();
        let now = get_session(f.state()).unwrap();
        assert_ne!(now.generation, old.generation);
        assert_eq!(current_name(&now), Some("dup-b.png"));

        let err = delete_similar(f.state(), old.generation, 0).err().unwrap();
        assert!(err.contains("開き直してください"), "{err}");
        assert!(keep_only(f.state(), old.generation, 0, vec![1]).is_err());
        assert!(f.pending().is_empty(), "古い番号で別の画像を削除予定にしない");
        assert!(delete_similar(f.state(), now.generation, 0).is_ok());
    }

    #[test]
    fn rescanning_after_reorder_reuses_fingerprints() {
        let f = Fixture::new();
        f.start_with_duplicates();
        f.scan();
        let mut config = f.config().config;
        config.general.sort_order = config::SortOrder::NameDesc;
        save_config(f.state(), config).unwrap();
        let generation = get_session(f.state()).unwrap().generation;

        let st = start_similar_scan(f.state()).unwrap();
        assert_eq!(st, ScanStatus { generation, done: 5, total: 5, running: false, failed: 0 }, "読み直さない");
        assert_eq!(f.groups(), [vec![0, 1], vec![2, 3, 4]], "新しい番号でグループを返す");
    }

    #[test]
    fn loading_a_folder_again_discards_the_scan() {
        let f = Fixture::new();
        f.start_with_duplicates();
        assert_eq!(f.scan().done, 5);
        let v = f.start();
        let st = similar_status(f.state());
        assert_eq!((st.generation, st.done, st.total), (v.generation, 0, 0));
        assert!(f.groups().is_empty());
    }
}
