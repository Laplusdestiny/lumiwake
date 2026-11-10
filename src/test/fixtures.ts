// テスト用のデータ。Rust 側（commands.rs）が返す形をまねる
import type { ConfigPayload, GridList, SessionView, SimilarGroups, SimilarMember, SimilarStatus } from "../api";

export function makeConfig(overrides: Partial<ConfigPayload["config"]["general"]> = {}): ConfigPayload {
  return {
    config: {
      version: 1,
      general: {
        include_subdirs: false,
        include_targets: false,
        sort_order: "name_asc",
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
        toggle_grid: "Ctrl+Shift+L",
        toggle_similar: "Ctrl+Shift+D",
        keep_only: "Ctrl+Shift+K",
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
    excludedTargets: [],
    conflict: null,
    ...overrides,
  };
}

/** 一覧表示用の 6 枚（番号 2 が現在の画像） */
export function makeGridList(overrides: Partial<GridList> = {}): GridList {
  return {
    generation: 1,
    current: 2,
    items: [
      { index: 0, name: "a.jpg", state: "moved", label: "風景" },
      { index: 1, name: "b.jpg", state: "deleted", label: "削除予定" },
      { index: 2, name: "c.jpg", state: "pending", label: null },
      { index: 3, name: "cat.jpg", state: "skipped", label: null },
      { index: 4, name: "d.jpg", state: "pending", label: null },
      { index: 5, name: "Cat2.png", state: "pending", label: null },
    ],
    ...overrides,
  };
}

function member(index: number, name: string, over: Partial<SimilarMember> = {}): SimilarMember {
  return {
    index,
    name,
    width: 400,
    height: 300,
    size: 1000,
    similarity: 97,
    largestResolution: false,
    largestSize: false,
    current: false,
    ...over,
  };
}

/** 類似画像の 2 グループ（3 枚と 2 枚。番号 2 が現在の画像） */
export function makeSimilarGroups(overrides: Partial<SimilarGroups> = {}): SimilarGroups {
  return {
    generation: 1,
    groups: [
      {
        members: [
          member(1, "a.jpg", { width: 800, height: 600, size: 2048, largestResolution: true, largestSize: true }),
          member(2, "a (1).jpg", { current: true }),
          member(4, "a_copy.jpg", { size: 900, similarity: 95 }),
        ],
        exact: false,
        sameSize: false,
        sameResolution: false,
        similarNames: true,
      },
      {
        members: [member(5, "x.png", { similarity: 100 }), member(7, "y.png", { similarity: 100 })],
        exact: true,
        sameSize: true,
        sameResolution: true,
        similarNames: false,
      },
    ],
    ...overrides,
  };
}

export function makeSimilarStatus(overrides: Partial<SimilarStatus> = {}): SimilarStatus {
  return { generation: 1, done: 8, total: 8, running: false, failed: 0, ...overrides };
}
