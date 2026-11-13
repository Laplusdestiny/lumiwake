// @vitest-environment happy-dom
// 画面全体の結合テスト。Rust 側（invoke）だけを差し替え、キー入力・クリックから呼ばれるコマンドを確かめる
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { makeConfig, makeGridList, makeSession, makeSimilarGroups, makeSimilarStatus } from "./test/fixtures";

const invoke = vi.hoisted(() => vi.fn());
const listen = vi.hoisted(() => vi.fn());
const checkForUpdates = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/path", () => ({ homeDir: async () => "/home/u/" }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "0.2.0" }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("./updater", () => ({ checkForUpdates }));

import { store } from "./store";

const root = () => document.querySelector<HTMLElement>("#app")!;
const commands = () => invoke.mock.calls.map((c) => c[0]);

function respond(command: string, args?: Record<string, unknown>): unknown {
  switch (command) {
    case "get_config":
      return makeConfig();
    case "get_session":
      return makeSession();
    case "perform":
    case "undo":
      return { view: makeSession({ canUndo: true }), message: null };
    case "set_view_mode":
      return makeConfig({ view_mode: args!.mode as "focus" });
    case "pending_deletions":
      return { items: [], trashDir: null, onExit: "confirm" };
    case "list_items":
      return makeGridList();
    case "start_similar_scan":
    case "similar_status":
      return makeSimilarStatus();
    case "similar_groups":
      return makeSimilarGroups();
    case "delete_similar":
      return { view: makeSession({ pendingDeletions: 1 }), message: null };
    case "keep_only":
      return { view: makeSession({ pendingDeletions: 2 }), deleted: 2, error: null };
    case "image_info":
      return { name: "c.jpg", path: "/c.jpg", size: 1, modified: null, width: 1, height: 1, taken: null };
    default:
      return makeSession();
  }
}

/** キーを押して、既定動作（再読み込みなど）を止めたかを返す */
function press(code: string, mods: Partial<KeyboardEventInit> = {}, target: EventTarget = window): boolean {
  const e = new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true, ...mods });
  target.dispatchEvent(e);
  return e.defaultPrevented;
}

let listened: unknown[] = [];
let checkedUpdates = false;

beforeAll(async () => {
  document.body.innerHTML = `<div id="app"></div>`;
  invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => respond(cmd, args));
  await import("./main");
  await vi.waitFor(() => expect(listen).toHaveBeenCalled());
  listened = listen.mock.calls[0];
  checkedUpdates = checkForUpdates.mock.calls.length > 0;
});

beforeEach(() => {
  invoke.mockClear();
  store.screen = "sort";
  store.session = makeSession();
  store.config = makeConfig();
});

describe("起動", () => {
  it("前回のセッションがあれば仕分け画面から始め、更新を確認する", () => {
    expect(store.home).toBe("/home/u");
    expect(root().dataset.view).toBe("sort-sidebar");
    expect(listened[0]).toBe("lumiwake://close-requested");
    expect(checkedUpdates).toBe(true);
  });

  it("ウィンドウを閉じようとしたら削除予定を確認する", async () => {
    const onClose = listened[1] as () => void;
    onClose();
    await vi.waitFor(() => expect(commands()).toEqual(["pending_deletions", "exit_app"]));
  });
});

describe("キー入力", () => {
  it("振り分けキーで移動し、WebView の既定動作は止める", async () => {
    expect(press("Digit1")).toBe(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("perform", { action: { kind: "move", target: 0 } }));
  });

  it("割り当てのない予約キー（再読み込み・印刷）も止める", () => {
    expect(press("F5")).toBe(true);
    expect(press("KeyR", { ctrlKey: true })).toBe(true);
    expect(press("KeyP", { ctrlKey: true })).toBe(true);
    expect(press("KeyX")).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("日本語入力の変換中や修飾キー単体は無視する", () => {
    expect(press("Digit1", { isComposing: true })).toBe(false);
    expect(press("ShiftLeft", { shiftKey: true })).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("入力欄では Esc 以外を横取りしない", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    expect(press("Digit1", {}, input)).toBe(false);
    expect(press("Escape", {}, input)).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    input.remove();
  });

  it("同名ファイルの確認中は選択肢のキーだけを受け付ける", async () => {
    store.session = makeSession({
      conflict: {
        item: 2,
        label: "風景",
        dir: "/d",
        incoming: { name: "c.jpg", path: "/a/c.jpg", size: null, modified: null },
        existing: { name: "c.jpg", path: "/d/c.jpg", size: null, modified: null },
      },
    });
    press("Digit3");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("resolve_conflict", { choice: "keepBoth" }));
    expect(commands()).not.toContain("perform");
  });

  it("Ctrl+, で設定を開き、Esc で戻る", async () => {
    expect(press("Comma", { ctrlKey: true })).toBe(true);
    expect(store.screen).toBe("settings");
    expect(root().dataset.view).toBe("settings");
    press("Escape");
    await vi.waitFor(() => expect(store.screen).toBe("sort"));
  });

  it("開始画面: Enter で読み込み、Esc で仕分けに戻る", async () => {
    store.screen = "start";
    press("Escape");
    expect(store.screen).toBe("sort");
    store.screen = "start";
    store.config = makeConfig({ source_dir: "/home/u/inbox" });
    press("Enter");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("start_session", { source: "/home/u/inbox", includeSubdirs: false, includeTargets: false }));
  });

  it("開始画面に起動時に取得したバージョンを表示する", async () => {
    expect(store.version).toBe("0.2.0");
    store.screen = "start";
    (await import("./store")).notify();
    expect(root().querySelector(".start-version")!.textContent).toBe("v0.2.0");
  });

  it("開始画面: ↑↓ で履歴から仕分け元を選ぶ", async () => {
    store.screen = "start";
    store.config = makeConfig({ source_dir: "/home/u/a", recent_sources: ["/home/u/a", "/home/u/b"] });
    expect(press("ArrowDown")).toBe(true);
    press("Enter");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("start_session", { source: "/home/u/b", includeSubdirs: false, includeTargets: false }));
  });

  it("Ctrl+Q で終了する", async () => {
    press("KeyQ", { ctrlKey: true });
    await vi.waitFor(() => expect(commands()).toContain("exit_app"));
  });
});

describe("類似画像", () => {
  const openByKey = async () => {
    (await import("./store")).notify();
    expect(press("KeyD", { ctrlKey: true, shiftKey: true })).toBe(true);
    await vi.waitFor(() => expect(root().dataset.view).toBe("similar"));
  };

  it("Ctrl+Shift+D で開き、削除キーで選択中の画像を削除予定にする", async () => {
    await openByKey();
    expect(commands()).toEqual(["start_similar_scan", "similar_groups"]);
    expect(press("Delete")).toBe(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("delete_similar", { generation: 1, index: 2 }));
    expect(commands()).not.toContain("perform");
    press("Escape");
    expect(store.screen).toBe("sort");
  });

  it("Ctrl+Shift+K でこれだけ残し、振り分けキー・スキップではファイルを動かさない", async () => {
    await openByKey();
    press("Digit1");
    press("Space");
    expect(press("KeyK", { ctrlKey: true, shiftKey: true })).toBe(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("keep_only", { generation: 1, keep: 2, others: [1, 4] }));
    expect(commands()).not.toContain("perform");
    press("Escape");
  });

  it("ボタンで開き、タイルのクリックで選択、ダブルクリックで 1 枚表示を開く", async () => {
    root().dataset.view = "";
    (await import("./store")).notify();
    root().querySelector<HTMLElement>('[data-action="similar"]')!.click();
    await vi.waitFor(() => expect(root().dataset.view).toBe("similar"));
    const tile = root().querySelector<HTMLElement>('.similar-tile[data-index="5"]')!;
    tile.click();
    await vi.waitFor(() => expect(tile.classList.contains("is-selected")).toBe(true));
    tile.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("jump_to", { index: 5 }));
    expect(store.screen).toBe("sort");
  });

  it("閾値のスライダーと絞り込みの入力を画面に渡す", async () => {
    await openByKey();
    const range = root().querySelector<HTMLInputElement>('[data-similar="threshold"]')!;
    range.value = "10";
    range.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("similar_groups", { maxDistance: 10 }));
    const select = root().querySelector<HTMLSelectElement>('[data-similar="filter"]')!;
    select.value = "exact";
    select.dispatchEvent(new Event("input", { bubbles: true }));
    expect(root().querySelectorAll(".similar-group")).toHaveLength(1);
    range.value = "6";
    range.dispatchEvent(new Event("input", { bubbles: true }));
    press("Escape");
  });

  it("Space で比較表示にし、ホイールで拡大する（既定のスクロールは止める）。一覧ではホイールを止めない", async () => {
    await openByKey();
    const wheel = () => {
      const e = new WheelEvent("wheel", { deltaY: -100, bubbles: true, cancelable: true });
      window.dispatchEvent(e);
      return e.defaultPrevented;
    };
    expect(wheel()).toBe(false);
    expect(press("Space")).toBe(true);
    expect(root().querySelector<HTMLElement>('[data-slot="similar-compare"]')!.hidden).toBe(false);
    expect(wheel()).toBe(true);
    expect(root().querySelector<HTMLElement>(".compare-pane img")!.style.transform).toContain("scale(1.5)");
    expect(press("Tab")).toBe(true);
    expect(root().querySelectorAll(".compare-pane")).toHaveLength(1);
    press("Escape");
    expect(store.screen).toBe("similar");
    press("Escape");
    expect(store.screen).toBe("sort");
    expect(commands()).not.toContain("perform");
  });

  it("確認画面の写しがなければ仕分け画面を出す", async () => {
    store.screen = "similar";
    store.similar = null;
    (await import("./store")).notify();
    expect(store.screen).toBe("sort");
    expect(root().dataset.view).toBe("sort-sidebar");
  });
});

describe("一覧", () => {
  const openByKey = async () => {
    (await import("./store")).notify();
    expect(press("KeyL", { ctrlKey: true, shiftKey: true })).toBe(true);
    await vi.waitFor(() => expect(root().dataset.view).toBe("grid"));
  };
  const tileSize = () => document.documentElement.style.getPropertyValue("--grid-tile");

  it("Ctrl+Shift+L で一覧を開き、Enter で選んだ画像へ移動して戻る", async () => {
    await openByKey();
    press("ArrowRight");
    press("Enter");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("jump_to", { index: 3 }));
    expect(store.screen).toBe("sort");
  });

  it("一覧では振り分け・削除・スキップのキーでファイルを動かさない", async () => {
    await openByKey();
    press("Digit1");
    press("Delete");
    press("Space");
    await new Promise((r) => setTimeout(r, 0));
    expect(commands()).toEqual(["list_items"]);
    press("Escape");
    expect(store.screen).toBe("sort");
  });

  it("Ctrl+= でタイルを大きくし、WebView の拡大にはしない", async () => {
    await openByKey();
    expect(press("Equal", { ctrlKey: true })).toBe(true);
    expect(tileSize()).toBe("240px");
    press("Digit0", { ctrlKey: true });
    expect(tileSize()).toBe("160px");
    press("Escape");
  });

  it("Ctrl+ホイールで大きさを変え、既定の拡大は止める", async () => {
    await openByKey();
    const e = new WheelEvent("wheel", { deltaY: -100, bubbles: true, cancelable: true });
    Object.defineProperty(e, "ctrlKey", { value: true });
    window.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
    expect(tileSize()).toBe("240px");
    press("Digit0", { ctrlKey: true });
    press("Escape");
  });

  it("一覧以外の画面では Ctrl+ホイールも入力欄も一覧に渡さない", async () => {
    (await import("./store")).notify();
    const e = new WheelEvent("wheel", { deltaY: -100, bubbles: true, cancelable: true });
    Object.defineProperty(e, "ctrlKey", { value: true });
    window.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    const input = document.createElement("input");
    input.dataset.grid = "size";
    input.value = "300";
    root().appendChild(input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.remove();
    await openByKey();
    expect(tileSize()).toBe("160px");
    press("Escape");
  });

  it("一覧の写しがなければ一覧の画面にせず仕分け画面を出す", async () => {
    store.screen = "grid";
    store.grid = null;
    (await import("./store")).notify();
    expect(store.screen).toBe("sort");
    expect(root().dataset.view).toBe("sort-sidebar");
  });

  it("絞り込み欄に入力すると絞り込み、Enter で一覧に戻って開ける", async () => {
    await openByKey();
    const input = root().querySelector<HTMLInputElement>('[data-grid="filter"]')!;
    input.focus();
    input.value = "cat";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    expect(root().querySelectorAll(".grid-tile")).toHaveLength(2);
    expect(press("Enter", {}, input)).toBe(true);
    press("Enter");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("jump_to", { index: 3 }));
  });

  it("一覧ボタンで開き、タイルのクリックで開く", async () => {
    root().dataset.view = "";
    (await import("./store")).notify();
    root().querySelector<HTMLElement>('[data-action="grid"]')!.click();
    await vi.waitFor(() => expect(root().dataset.view).toBe("grid"));
    root().querySelector<HTMLElement>('.grid-tile[data-index="4"]')!.click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("jump_to", { index: 4 }));
    expect(store.screen).toBe("sort");
  });

  it("振り分けの処理中に一覧キーを押しても、同名ファイルの確認になったら一覧を開かない", async () => {
    const conflict = {
      item: 2,
      label: "風景",
      dir: "/d",
      incoming: { name: "c.jpg", path: "/a/c.jpg", size: null, modified: null },
      existing: { name: "c.jpg", path: "/d/c.jpg", size: null, modified: null },
    };
    let finish!: () => void;
    invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) =>
      cmd === "perform"
        ? new Promise((r) => (finish = () => r({ view: makeSession({ conflict }), message: null })))
        : respond(cmd, args),
    );
    (await import("./store")).notify();
    press("Digit1");
    press("KeyL", { ctrlKey: true, shiftKey: true });
    await new Promise((r) => setTimeout(r, 0));
    finish();
    await vi.waitFor(() => expect(store.session?.conflict).not.toBeNull());
    await new Promise((r) => setTimeout(r, 0));
    invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => respond(cmd, args));
    expect(commands()).not.toContain("list_items");
    expect(store.screen).toBe("sort");
  });

  it("同名ファイルの確認中は一覧ボタンを押しても開かない", async () => {
    store.session = makeSession({
      conflict: {
        item: 2,
        label: "風景",
        dir: "/d",
        incoming: { name: "c.jpg", path: "/a/c.jpg", size: null, modified: null },
        existing: { name: "c.jpg", path: "/d/c.jpg", size: null, modified: null },
      },
    });
    root().dataset.view = "";
    (await import("./store")).notify();
    root().querySelector<HTMLElement>('[data-action="grid"]')!.click();
    await new Promise((r) => setTimeout(r, 0));
    expect(commands()).not.toContain("list_items");
    expect(store.screen).toBe("sort");
  });
});

describe("クリック", () => {
  const clickAction = async (selector: string) => {
    root().querySelector<HTMLElement>(selector)!.click();
    await new Promise((r) => setTimeout(r, 0));
  };

  it("振り分け先・削除・取り消しのボタン", async () => {
    store.session = makeSession({ canUndo: true });
    store.config = makeConfig();
    root().dataset.view = "";
    store.screen = "sort";
    (await import("./store")).notify();
    await clickAction('[data-action="move"][data-target="1"]');
    await clickAction('[data-action="delete"]');
    await vi.waitFor(() => expect(commands()).toEqual(["perform", "perform"]));
    expect(invoke).toHaveBeenCalledWith("perform", { action: { kind: "move", target: 1 } });
    expect(invoke).toHaveBeenCalledWith("perform", { action: { kind: "delete" } });
    await clickAction('[data-action="undo"]');
    await vi.waitFor(() => expect(commands()).toContain("undo"));
  });

  it("表示モードを切り替えて保存する", async () => {
    (await import("./store")).notify();
    await clickAction('[data-action="mode"][data-mode="focus"]');
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("set_view_mode", { mode: "focus" }));
    expect(root().dataset.view).toBe("sort-focus");
  });

  it("仕分け元の変更で開始画面へ、戻るで仕分け画面へ", async () => {
    (await import("./store")).notify();
    await clickAction('[data-action="change-source"]');
    expect(store.screen).toBe("start");
    await clickAction('[data-action="back-to-sort"]');
    expect(store.screen).toBe("sort");
  });

  it("開始画面: 履歴から選ぶ・履歴から外す", async () => {
    store.screen = "start";
    store.config = makeConfig({ source_dir: "/home/u/a", recent_sources: ["/home/u/a", "/home/u/b"] });
    (await import("./store")).notify();
    await clickAction('[data-action="choose-source"][data-path="/home/u/b"]');
    expect(root().querySelector(".path-box")!.textContent).toBe("~/b");
    await clickAction('[data-action="forget-source"][data-path="/home/u/a"]');
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("forget_source", { path: "/home/u/a" }));
  });

  it("右クリックメニューを出さない", () => {
    const e = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    root().dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
  });
});
