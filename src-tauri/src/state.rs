//! アプリ全体の状態。Tauri の State として共有する。

use crate::config::{self, Config};
use crate::decoder::{make_preview, DecodeLimit, PrefetchJob, Prefetcher, PreviewCache, Registry, ThumbCache};
use crate::fileops::{Session, Status};
use crate::similar::scan::SimilarScan;
use crate::similar::store::{self, PrintStore};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// 先読み済みの画像をいくつまで保持するか
const CACHE_CAPACITY: usize = 24;
/// 一覧用サムネイルをいくつまで保持するか（1 枚 30〜50 KB で 40 MB 程度）
const GRID_CACHE_CAPACITY: usize = 1000;
/// 一覧用サムネイルを同時にいくつまでデコードするか
const GRID_DECODE_LIMIT: usize = 3;

pub struct ConfigState {
    pub config: Config,
    /// 起動時に設定ファイルを読めなかった場合の理由（既定値で動いている）
    pub load_error: Option<String>,
}

pub struct SessionState {
    pub session: Session,
    /// 仕分け元を読み込むたびに変わる番号。古い画面からの要求を見分ける
    pub generation: u64,
    pub source: PathBuf,
    pub unsupported: Vec<PathBuf>,
    /// 読み込み時に読み飛ばした振り分け先フォルダ
    pub excluded_targets: Vec<PathBuf>,
}

pub struct AppState {
    pub config_path: PathBuf,
    config: Mutex<ConfigState>,
    session: Mutex<Option<SessionState>>,
    /// 仕分け元を切り替える前のセッション。削除予定の記録を終了時まで引き継ぐ
    retired: Mutex<Vec<Session>>,
    pub registry: Arc<Registry>,
    pub cache: Arc<PreviewCache>,
    /// 一覧用サムネイルのキャッシュ（先読みのキャッシュとは別）
    pub grid_cache: Arc<ThumbCache>,
    pub grid_limit: DecodeLimit,
    /// 類似画像の判定用の情報（バックグラウンドで集める）
    pub similar: Arc<SimilarScan>,
    /// 保存した類似判定の結果（アプリを終了しても残る）
    pub prints: Arc<PrintStore>,
    prefetcher: Prefetcher,
    /// true のときだけウィンドウを閉じてよい（削除予定の確認を済ませた）
    pub allow_exit: AtomicBool,
    next_generation: AtomicU64,
}

impl AppState {
    /// `cache_path` は類似判定の結果を保存するファイル（ここではまだ読まない）
    pub fn new(config_path: PathBuf, cache_path: PathBuf) -> Self {
        let (config, load_error) = match config::load_or_create(&config_path) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(e.to_string())),
        };
        let registry = Arc::new(Registry::builtin());
        let cache = Arc::new(PreviewCache::new(CACHE_CAPACITY));
        let loader_registry = registry.clone();
        let workers = std::thread::available_parallelism().map(|n| n.get().clamp(1, 3)).unwrap_or(2);
        let prefetcher =
            Prefetcher::start(workers, cache.clone(), Arc::new(move |p: &Path| make_preview(&loader_registry, p)));
        let prints = Arc::new(PrintStore::new(cache_path));
        AppState {
            config_path,
            config: Mutex::new(ConfigState { config, load_error }),
            session: Mutex::new(None),
            retired: Mutex::new(Vec::new()),
            registry,
            cache,
            grid_cache: Arc::new(ThumbCache::new(GRID_CACHE_CAPACITY)),
            grid_limit: DecodeLimit::new(GRID_DECODE_LIMIT),
            similar: Arc::new(SimilarScan::with_on_idle(Arc::new({
                let prints = prints.clone();
                // 判定が終わったら書き出す。失敗しても判定の結果は使える
                move || {
                    if let Err(e) = prints.save(store::now_secs()) {
                        eprintln!("類似判定の記録を保存できませんでした: {e}");
                    }
                }
            }))),
            prints,
            prefetcher,
            allow_exit: AtomicBool::new(false),
            next_generation: AtomicU64::new(1),
        }
    }

    pub fn config(&self) -> MutexGuard<'_, ConfigState> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn session(&self) -> MutexGuard<'_, Option<SessionState>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn retired(&self) -> MutexGuard<'_, Vec<Session>> {
        self.retired.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 類似判定の記録を書き出す。Lumiwake が移動した画像は、移動先のパスでも記録を使えるようにしてから書く。
    /// 失敗しても終了などは止めない
    pub fn save_prints(&self) {
        let now = store::now_secs();
        let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
        {
            // ロックは常に session → retired の順で取る
            let current = self.session();
            let retired = self.retired();
            for s in retired.iter().chain(current.as_ref().map(|c| &c.session)) {
                for item in s.items() {
                    if let Status::Moved { to } | Status::Trashed { to, .. } = &item.status {
                        moves.push((item.path.clone(), to.clone()));
                    }
                }
            }
        }
        for (from, to) in moves {
            // 次に読み込むときのパス（スキャナは実体のパスにそろえる）と合わせる
            let to = dunce::canonicalize(&to).unwrap_or(to);
            self.prints.copy_record(&from, &to, now);
        }
        if let Err(e) = self.prints.save(now) {
            eprintln!("類似判定の記録を保存できませんでした: {e}");
        }
    }

    pub fn next_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::SeqCst)
    }

    /// 現在の画像と次の数枚を先読みする
    pub fn schedule_prefetch(&self, state: &SessionState, count: usize) {
        let s = &state.session;
        let Some(current) = s.current() else {
            self.prefetcher.schedule(Vec::new());
            return;
        };
        let jobs = std::iter::once(current)
            .chain(s.upcoming(count))
            .map(|i| {
                let item = &s.items()[i];
                PrefetchJob { key: item.path.clone(), path: location(&item.path, &item.status).to_path_buf() }
            })
            .collect();
        self.prefetcher.schedule(jobs);
    }
}

/// 画像が今ある場所
pub fn location<'a>(original: &'a Path, status: &'a Status) -> &'a Path {
    match status {
        Status::Moved { to } | Status::Trashed { to, .. } => to,
        _ => original,
    }
}
