//! アプリ全体の状態。Tauri の State として共有する。

use crate::config::{self, Config};
use crate::decoder::{make_preview, PrefetchJob, Prefetcher, PreviewCache, Registry};
use crate::fileops::{Session, Status};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// 先読み済みの画像をいくつまで保持するか
const CACHE_CAPACITY: usize = 24;

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
}

pub struct AppState {
    pub config_path: PathBuf,
    config: Mutex<ConfigState>,
    session: Mutex<Option<SessionState>>,
    /// 仕分け元を切り替える前のセッション。削除予定の記録を終了時まで引き継ぐ
    retired: Mutex<Vec<Session>>,
    pub registry: Arc<Registry>,
    pub cache: Arc<PreviewCache>,
    prefetcher: Prefetcher,
    /// true のときだけウィンドウを閉じてよい（削除予定の確認を済ませた）
    pub allow_exit: AtomicBool,
    next_generation: AtomicU64,
}

impl AppState {
    pub fn new(config_path: PathBuf) -> Self {
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
        AppState {
            config_path,
            config: Mutex::new(ConfigState { config, load_error }),
            session: Mutex::new(None),
            retired: Mutex::new(Vec::new()),
            registry,
            cache,
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
