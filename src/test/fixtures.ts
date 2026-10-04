// テスト用のデータ。Rust 側（commands.rs）が返す形をまねる
import type { ConfigPayload, SessionView } from "../api";

export function makeConfig(overrides: Partial<ConfigPayload["config"]["general"]> = {}): ConfigPayload {
  return {
    config: {
      version: 1,
      general: {
        include_subdirs: false,
        on_exit: "confirm",
        view_mode: "sidebar",
        accent: "amber",
        prefetch: 4,
        check_updates: true,
        show_paths: true,
        ...overrides,
      },
      keys: {
        skip: "Space",
        delete: "Delete",
        undo: "Ctrl+Z",
        prev: "Left",
        next: "Right",
        toggle_view: "Ctrl+Shift+F",
        toggle_paths: "Ctrl+Shift+P",
      },
      targets: [
        { key: "1", name: "", path: "/home/u/写真/風景" },
        { key: "2", name: "人物", path: "/home/u/写真/people" },
      ],
    },
    path: "/home/u/.config/lumiwake/config.toml",
    issues: [],
    loadError: null,
    formats: [
      { format: "jpeg", label: "JPEG", supported: true, decoder: "image" },
      { format: "heic", label: "HEIC", supported: false, decoder: null },
    ],
  };
}

export function makeSession(overrides: Partial<SessionView> = {}): SessionView {
  return {
    generation: 1,
    source: "/home/u/inbox",
    total: 5,
    current: { index: 2, name: "c.jpg", path: "/home/u/inbox/c.jpg" },
    remaining: 3,
    skipped: 0,
    upcoming: [3, 4],
    history: [],
    nextSkipped: null,
    canUndo: false,
    pendingDeletions: 0,
    movedCounts: [0, 0],
    unsupported: [],
    conflict: null,
    ...overrides,
  };
}
