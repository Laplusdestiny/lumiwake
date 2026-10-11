// テスト用のデータ。Rust 側（commands.rs）が返す形をまねる
import type { ConfigPayload, SessionView, Suggestions } from "../api";

export function makeConfig(overrides: Partial<ConfigPayload["config"]["general"]> = {}): ConfigPayload {
  return {
    config: {
      version: 2,
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
        toggle_paths: "Ctrl+Shift+O",
        rediagnose: "Ctrl+Shift+D",
      },
      targets: [
        { key: "1", name: "", path: "/home/u/写真/風景" },
        { key: "2", name: "人物", path: "/home/u/写真/people" },
      ],
      ai: {
        backend: "local",
        top_k: 3,
        prefetch: 5,
        local: { model: "clip-vit-b32", strategy: "hybrid", high: 0.8, low: 0.4 },
        systemone: {
          endpoint: "https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/clef-flash",
          model: "",
          api_key_env: "LUMIWAKE_SYSTEMONE_KEY",
          max_image_kb: 190,
          high: 0.85,
          low: 0.5,
          external_consent: false,
        },
      },
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

/** 候補がまだない状態（ローカル推論で、まだ何も出していない） */
export function makeSuggestions(): Suggestions {
  return {
    backend: "local",
    kind: "match",
    sendsImages: false,
    cards: [],
    noneOfAbove: null,
    unevaluated: [],
    fromCache: false,
    state: "ready",
  };
}
