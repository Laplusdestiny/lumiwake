// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const actions = vi.hoisted(() => ({
  busy: vi.fn(() => false),
  undo: vi.fn(),
  navigate: vi.fn(),
  perform: vi.fn(),
}));
const api = vi.hoisted(() => ({ setShowPaths: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
vi.mock("../actions", () => actions);
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), api }));
vi.mock("../toast", () => ({ toast }));

import { store } from "../store";
import { makeConfig, makeSession } from "../test/fixtures";
import { handleSorterKey, initSidebarResize, onModeChange, renderSorter, togglePaths } from "./sorter";

let root: HTMLElement;
const key = (combo: string, repeat = false) => handleSorterKey(combo, { repeat } as KeyboardEvent);
const text = (sel: string) => root.querySelector(sel)?.textContent?.replace(/\s+/g, " ").trim();

beforeEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = `<div id="app"></div>`;
  root = document.querySelector("#app")!;
  store.config = makeConfig();
  store.session = makeSession();
  store.info = null;
  store.home = "/home/u";
});

describe("renderSorter", () => {
  it("サイドバー型: 進み具合・振り分け先・削除の行を表示する", () => {
    store.session = makeSession({ movedCounts: [3, 0], pendingDeletions: 2, canUndo: true });
    renderSorter(root);
    expect(root.dataset.view).toBe("sort-sidebar");
    expect(text('[data-slot="progress"]')).toBe("3 / 5 枚");
    expect(text('[data-slot="source"]')).toBe("仕分け元：~/inbox");
    const rows = [...root.querySelectorAll(".target-row")].map((r) => r.textContent!.replace(/\s+/g, " ").trim());
    expect(rows).toEqual(["1 風景 ~/写真/風景 3", "2 人物 ~/写真/people", "Delete 削除 未指定（その場で削除予定に） 2"]);
    expect(root.querySelector<HTMLButtonElement>('[data-action="undo"]')!.disabled).toBe(false);
    expect(text('[data-slot="hints"]')).toContain("1〜2 で移動");
  });

  it("画像の詳細（大きさ・撮影日時）を表示する", () => {
    store.info = {
      generation: 1,
      index: 2,
      info: { name: "c.jpg", path: "/c.jpg", size: 2048, modified: null, width: 640, height: 480, taken: "2026:01:02 03:04:05" },
    };
    renderSorter(root);
    expect(text('[data-slot="meta"]')).toBe("c.jpg 640×480 · 2.0 KB · 2026:01:02 03:04:05 撮影");
  });

  it("パスを隠しても、同じ名前の振り分け先はパスを出す", () => {
    const cfg = makeConfig({ show_paths: false, delete_folder: "/home/u/ごみ" });
    cfg.config.targets = [
      { key: "1", name: "", path: "/a/2026" },
      { key: "Ctrl+2", name: "", path: "/b/2026" },
      { key: "3", name: "", path: "/c/その他" },
    ];
    store.config = cfg;
    renderSorter(root);
    const paths = [...root.querySelectorAll(".target-path")].map((e) => e.textContent);
    expect(paths).toEqual(["/a/2026", "/b/2026"]);
    expect(root.querySelector('[data-slot="targets"]')!.classList.contains("compact")).toBe(true);
    expect(root.querySelector(".delete-row")!.getAttribute("title")).toContain("/home/u/ごみ");
    // 組み合わせキーがあるときは範囲で書かない
    expect(text('[data-slot="hints"]')).toContain("振り分けキーで移動");
  });

  it("フィルムストリップ: 保留中の画像はクリックで戻れる", () => {
    store.session = makeSession({
      skipped: 1,
      nextSkipped: 0,
      history: [
        { item: 1, label: "風景", kind: "move", open: false },
        { item: 0, label: "保留", kind: "skip", open: true },
      ],
    });
    renderSorter(root);
    const film = root.querySelector('[data-slot="filmstrip"]')!;
    expect(film.querySelector('.tile-open[data-action="jump"]')!.getAttribute("data-index")).toBe("0");
    expect(film.querySelector(".kind-move")).not.toBeNull();
    expect(film.querySelectorAll(".film-next .tile")).toHaveLength(2);
    expect(root.querySelector('.skip-chip[data-action="jump"]')!.getAttribute("data-index")).toBe("0");
  });

  it("すべて処理したら完了画面を出し、振り分けボタンを無効にする", () => {
    store.session = makeSession({ current: null, remaining: 1, skipped: 1 });
    renderSorter(root);
    expect(text(".finished-title")).toBe("すべての画像を処理しました");
    expect(text(".finished")).toContain("保留した画像が 1 枚あります");
    expect(root.querySelector<HTMLButtonElement>(".target-row")!.disabled).toBe(true);
    expect(text('[data-slot="meta"]')).toBe("完了");
  });

  it("画像がないフォルダ・振り分け先が未登録のとき", () => {
    store.session = makeSession({ total: 0, current: null, remaining: 0 });
    store.config!.config.targets = [];
    renderSorter(root);
    expect(text(".finished-title")).toBe("画像がありません");
    expect(root.querySelector('.empty-targets [data-action="settings"]')).not.toBeNull();
  });

  it("全画面集中型: 振り分け先をチップで表示する", () => {
    store.config = makeConfig({ view_mode: "focus" });
    renderSorter(root);
    expect(root.dataset.view).toBe("sort-focus");
    expect([...root.querySelectorAll(".chip")].map((c) => c.textContent)).toEqual(["1風景", "2人物"]);
    expect(root.querySelector('[data-slot="filmstrip"]')).toBeNull();
  });

  it("セッションや設定がなければ何もしない", () => {
    store.session = null;
    renderSorter(root);
    expect(root.innerHTML).toBe("");
  });
});

describe("handleSorterKey", () => {
  it("設定したキーを操作に割り当てる", () => {
    expect(key("1")).toBe(true);
    expect(actions.perform).toHaveBeenLastCalledWith({ kind: "move", target: 0 });
    expect(key("2")).toBe(true);
    expect(actions.perform).toHaveBeenLastCalledWith({ kind: "move", target: 1 });
    key("Space");
    expect(actions.perform).toHaveBeenLastCalledWith({ kind: "skip" });
    key("Delete");
    expect(actions.perform).toHaveBeenLastCalledWith({ kind: "delete" });
    key("Ctrl+Z");
    expect(actions.undo).toHaveBeenCalled();
    key("Left");
    key("Right");
    expect(actions.navigate.mock.calls).toEqual([[false], [true]]);
    expect(key("9")).toBe(false);
  });

  it("キーリピートや処理待ちが溜まっているときは前後移動だけ受け付ける", () => {
    expect(key("1", true)).toBe(true);
    expect(actions.perform).not.toHaveBeenCalled();
    key("Right", true);
    expect(actions.navigate).toHaveBeenCalledWith(true);
    actions.busy.mockReturnValue(true);
    expect(key("Space")).toBe(true);
    expect(actions.perform).not.toHaveBeenCalled();
    actions.busy.mockReturnValue(false);
  });

  it("すべて処理した後は振り分けキーを受け付けない", () => {
    store.session = makeSession({ current: null });
    expect(key("1")).toBe(false);
    expect(key("Space")).toBe(false);
    expect(actions.perform).not.toHaveBeenCalled();
  });

  it("表示モードの切り替えと Esc での全画面終了", () => {
    const listener = vi.fn();
    onModeChange(listener);
    key("Ctrl+Shift+F");
    expect(listener).toHaveBeenLastCalledWith("focus");
    expect(key("Escape")).toBe(false);
    store.config = makeConfig({ view_mode: "focus" });
    key("Ctrl+Shift+F");
    expect(listener).toHaveBeenLastCalledWith("sidebar");
    expect(key("Escape")).toBe(true);
  });

  it("セッションがなければ処理しない", () => {
    store.session = null;
    expect(key("1")).toBe(false);
  });
});

describe("togglePaths", () => {
  it("その場で切り替えてから設定に保存する", async () => {
    const saved = makeConfig({ show_paths: false });
    api.setShowPaths.mockResolvedValue(saved);
    expect(key("Ctrl+Shift+O")).toBe(true);
    expect(store.config!.config.general.show_paths).toBe(false);
    await vi.waitFor(() => expect(store.config).toBe(saved));
    expect(api.setShowPaths).toHaveBeenCalledWith(false);
  });

  it("保存に失敗したらエラーを表示する", async () => {
    api.setShowPaths.mockRejectedValue("書き込めません");
    await togglePaths();
    expect(toast).toHaveBeenCalledWith("書き込めません", "error");
  });
});

describe("サイドバーの幅", () => {
  it("ドラッグで幅を変えて保存し、ダブルクリックで元に戻す", () => {
    localStorage.setItem("lumiwake.sidebarWidth", "300");
    window.innerWidth = 1200;
    initSidebarResize();
    const width = () => document.documentElement.style.getPropertyValue("--sidebar-w");
    expect(width()).toBe("300px");

    renderSorter(root);
    const handle = root.querySelector<HTMLElement>("[data-resize]")!;
    handle.setPointerCapture = vi.fn();
    handle.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerId: 1 }));
    expect(document.body.classList.contains("resizing")).toBe(true);
    handle.dispatchEvent(new PointerEvent("pointermove", { clientX: 700 }));
    expect(width()).toBe("500px");
    // プレビューの最低幅を残す
    handle.dispatchEvent(new PointerEvent("pointermove", { clientX: 100 }));
    expect(width()).toBe("780px");
    handle.dispatchEvent(new PointerEvent("pointerup"));
    expect(document.body.classList.contains("resizing")).toBe(false);
    expect(localStorage.getItem("lumiwake.sidebarWidth")).toBe("780");

    handle.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(width()).toBe("360px");
    expect(localStorage.getItem("lumiwake.sidebarWidth")).toBe("360");
  });
});
