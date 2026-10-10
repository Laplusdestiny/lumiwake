// @vitest-environment happy-dom
// 画面全体の結合テスト。Rust 側（invoke）だけを差し替え、キー入力・クリックから呼ばれるコマンドを確かめる
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { makeConfig, makeSession } from "./test/fixtures";

const invoke = vi.hoisted(() => vi.fn());
const listen = vi.hoisted(() => vi.fn());
const checkForUpdates = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/path", () => ({ homeDir: async () => "/home/u/" }));
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
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("start_session", { source: "/home/u/inbox", includeSubdirs: false }));
  });

  it("開始画面: ↑↓ で履歴から仕分け元を選ぶ", async () => {
    store.screen = "start";
    store.config = makeConfig({ source_dir: "/home/u/a", recent_sources: ["/home/u/a", "/home/u/b"] });
    expect(press("ArrowDown")).toBe(true);
    press("Enter");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("start_session", { source: "/home/u/b", includeSubdirs: false }));
  });

  it("Ctrl+Q で終了する", async () => {
    press("KeyQ", { ctrlKey: true });
    await vi.waitFor(() => expect(commands()).toContain("exit_app"));
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
