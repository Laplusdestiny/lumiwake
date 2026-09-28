// Rust コア（src-tauri/src/commands.rs）とのやり取り。画面はここ以外から invoke しない。
import { invoke } from "@tauri-apps/api/core";

export type OnExit = "confirm" | "delete";
export type ViewMode = "sidebar" | "focus";
export type Accent = "amber" | "blue" | "green";

export interface Target {
  key: string;
  name: string;
  path: string;
}

/** TOML の設定そのもの（キー名は TOML と同じ snake_case） */
export interface Config {
  general: {
    source_dir?: string;
    include_subdirs: boolean;
    delete_folder?: string;
    on_exit: OnExit;
    view_mode: ViewMode;
    accent: Accent;
    prefetch: number;
    check_updates: boolean;
  };
  keys: {
    skip: string;
    delete: string;
    undo: string;
    prev: string;
    next: string;
    toggle_view: string;
  };
  targets: Target[];
}

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
  imageInfo: (generation: number, index: number) => invoke<ImageInfo>("image_info", { generation, index }),
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
