// Rust コア（src-tauri/src/commands.rs）とのやり取り。画面はここ以外から invoke しない。
import { invoke } from "@tauri-apps/api/core";

export type OnExit = "confirm" | "delete";
export type ViewMode = "sidebar" | "focus";
export type Accent = "amber" | "blue" | "green";

export interface Target {
  key: string;
  name: string;
  path: string;
  /** AI 候補用の説明文 */
  description?: string;
  /** true なら外部 API への選択肢に含めない */
  exclude_external?: boolean;
}

/** TOML の設定そのもの（キー名は TOML と同じ snake_case） */
export interface Config {
  /** 設定ファイルの形式のバージョン */
  version: number;
  general: {
    source_dir?: string;
    include_subdirs: boolean;
    delete_folder?: string;
    on_exit: OnExit;
    view_mode: ViewMode;
    accent: Accent;
    prefetch: number;
    check_updates: boolean;
    show_paths: boolean;
  };
  keys: {
    skip: string;
    delete: string;
    undo: string;
    prev: string;
    next: string;
    toggle_view: string;
    toggle_paths: string;
    rediagnose: string;
  };
  targets: Target[];
  /** AI 振り分け候補。設定画面が対応するまでは、読んだ値をそのまま保存し直す */
  ai: Ai;
}

export type AiBackend = "local" | "systemone" | "off";

export interface Ai {
  backend: AiBackend;
  top_k: number;
  prefetch: number;
  local: { model: string; strategy: "zeroshot" | "knn" | "hybrid"; high: number; low: number };
  systemone: {
    endpoint: string;
    /** 空ならエンドポイントの末尾から判別 */
    model: string;
    api_key_env: string;
    max_image_kb: number;
    high: number;
    low: number;
    external_consent: boolean;
  };
}

export type SuggestionLevel = "high" | "mid" | "low";

/** 候補カード 1 枚。`target` は設定の targets の番号（キーでの振り分けはこの番号を使う） */
export interface SuggestionCard {
  target: number;
  key: string;
  name: string;
  path: string;
  score: number;
  level: SuggestionLevel;
}

/**
 * 1 枚の画像の AI 候補。`state` が "ready" のときだけ cards などが意味を持つ。
 * kind が "match" のスコアは確率ではなく「一致度」と表記する。
 */
export type Suggestions = {
  backend: AiBackend;
  kind: "probability" | "match" | null;
  /** 画像を外部へ送っているか（「外部送信中」表示） */
  sendsImages: boolean;
  cards: SuggestionCard[];
  /** 「該当なし」の確率（Space＝スキップに対応）。確率を返すバックエンドのみ */
  noneOfAbove: number | null;
  /** 診断のあとに追加された、未評価の振り分け先の名前 */
  unevaluated: string[];
  fromCache: boolean;
} & ({ state: "ready" | "off" } | { state: "unavailable" | "failed"; message: string });

export interface Issue {
  severity: "error" | "warning";
  message: string;
  key?: string;
}

export interface FormatSupport {
  format: string;
  label: string;
  supported: boolean;
  decoder: string | null;
}

export interface ConfigPayload {
  config: Config;
  path: string;
  issues: Issue[];
  loadError: string | null;
  formats: FormatSupport[];
}

export interface FileView {
  name: string;
  path: string;
  size: number | null;
  modified: number | null;
}

export interface ImageInfo extends FileView {
  width: number;
  height: number;
  taken: string | null;
}

export interface ConflictView {
  item: number;
  label: string;
  dir: string;
  incoming: FileView;
  existing: FileView;
}

export interface HistoryEntry {
  item: number;
  label: string;
  kind: "move" | "skip" | "delete";
  /** 保留中で、クリックして戻れる */
  open: boolean;
}

export interface SessionView {
  generation: number;
  source: string;
  total: number;
  current: { index: number; name: string; path: string } | null;
  remaining: number;
  skipped: number;
  upcoming: number[];
  history: HistoryEntry[];
  /** 次に見る保留中の画像（現在の画像を除く） */
  nextSkipped: number | null;
  canUndo: boolean;
  pendingDeletions: number;
  movedCounts: number[];
  unsupported: string[];
  conflict: ConflictView | null;
}

export interface ActionResult {
  view: SessionView;
  message: string | null;
}

export type ConflictChoice = "keepExisting" | "overwrite" | "keepBoth" | "skip";

export type DeletionReason = "deleteKey" | "keepExisting" | "overwritten";

export interface PendingDeletion {
  path: string;
  original: string;
  reason: DeletionReason;
  inTrash: boolean;
}

export interface DeletionSummary {
  items: PendingDeletion[];
  trashDir: string | null;
  onExit: OnExit;
}

export interface FinalizeReport {
  deleted: string[];
  failed: [string, string][];
}

export type SortAction = { kind: "move"; target: number } | { kind: "skip" } | { kind: "delete" };

export const api = {
  getConfig: () => invoke<ConfigPayload>("get_config"),
  validateConfig: (config: Config) => invoke<Issue[]>("validate_config", { config }),
  saveConfig: (config: Config) => invoke<ConfigPayload>("save_config", { config }),
  reloadConfig: () => invoke<ConfigPayload>("reload_config"),
  setViewMode: (mode: ViewMode) => invoke<ConfigPayload>("set_view_mode", { mode }),
  setShowPaths: (show: boolean) => invoke<ConfigPayload>("set_show_paths", { show }),
  openConfigFile: () => invoke<void>("open_config_file"),
  openLocation: (path: string) => invoke<void>("open_location", { path }),
  listSubfolders: (path: string) => invoke<string[]>("list_subfolders", { path }),
  startSession: (source: string, includeSubdirs: boolean) =>
    invoke<SessionView>("start_session", { source, includeSubdirs }),
  getSession: () => invoke<SessionView | null>("get_session"),
  perform: (action: SortAction) => invoke<ActionResult>("perform", { action }),
  resolveConflict: (choice: ConflictChoice) => invoke<ActionResult>("resolve_conflict", { choice }),
  cancelConflict: () => invoke<SessionView>("cancel_conflict"),
  undo: () => invoke<ActionResult>("undo"),
  navigate: (forward: boolean) => invoke<SessionView>("navigate", { forward }),
  jumpTo: (index: number) => invoke<SessionView>("jump_to", { index }),
  imageInfo: (generation: number, index: number) => invoke<ImageInfo>("image_info", { generation, index }),
  /** 画像の AI 候補。force=true で診断し直す（キャッシュにあればリクエストは出ない） */
  getSuggestions: (generation: number, index: number, force = false) =>
    invoke<Suggestions>("get_suggestions", { generation, index, force }),
  /** System One の API キーが環境変数にあるか（キーの値は返らない） */
  aiKeyStatus: () => invoke<{ envName: string; detected: boolean }>("ai_key_status"),
  /** System One への接続テスト。合成した小さな画像を 1 回だけ送り、成功したらモデル名を返す */
  testSystemone: () => invoke<string>("test_systemone"),
  /** 画像を外部へ送ることへの同意を保存する（false で取り消し） */
  setExternalConsent: (consent: boolean) => invoke<ConfigPayload>("set_external_consent", { consent }),
  pendingDeletions: () => invoke<DeletionSummary>("pending_deletions"),
  finalizeAndExit: (del: boolean) => invoke<FinalizeReport>("finalize_and_exit", { delete: del }),
  exitApp: () => invoke<void>("exit_app"),
};

const isWindows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");

/** Rust 側のカスタムプロトコル（lumi://）の URL。Windows の WebView2 では http://lumi.localhost になる */
export function lumiUrl(path: string): string {
  return isWindows ? `http://lumi.localhost/${path}` : `lumi://localhost/${path}`;
}

/** invoke のエラーを表示用の文字列にする */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
