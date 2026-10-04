// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  validateConfig: vi.fn(),
  saveConfig: vi.fn(),
  reloadConfig: vi.fn(),
  listSubfolders: vi.fn(),
  openConfigFile: vi.fn(),
}));
const dialog = vi.hoisted(() => ({ ask: vi.fn(), open: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), api }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("../toast", () => ({ toast }));

import type { ConfigPayload } from "../api";
import { makeConfig, makeSession } from "../test/fixtures";

type Settings = typeof import("./settings");
type Store = typeof import("../store")["store"];

let root: HTMLElement;
let s: Settings;
let store: Store;

/** 設定画面は編集中の下書きをモジュール内に持つので、テストごとに読み込み直す */
async function open(payload: ConfigPayload = makeConfig()): Promise<void> {
  vi.resetModules();
  const storeMod = await import("../store");
  store = storeMod.store;
  s = await import("./settings");
  store.config = payload;
  store.home = "/home/u";
  storeMod.onChange(() => store.screen === "settings" && s.renderSettings(root));
  s.openSettings("keys");
}

const click = (action: string, extra: Record<string, string> = {}) => {
  const el = document.createElement("button");
  Object.assign(el.dataset, extra);
  return s.handleSettingsAction(action, el);
};
const input = (field: string, value: string, checked = false, extra: Record<string, string> = {}) => {
  const el = document.createElement("input");
  el.dataset.field = field;
  el.value = value;
  el.checked = checked;
  Object.assign(el.dataset, extra);
  s.handleSettingsInput(el);
};
const saveButton = () => root.querySelector<HTMLButtonElement>('[data-action="save-config"]')!;
const keys = () => [...root.querySelectorAll<HTMLElement>('[data-action="capture-target"]')].map((b) => b.dataset.key);

beforeEach(async () => {
  vi.clearAllMocks();
  vi.useRealTimers();
  document.body.innerHTML = `<div id="app"></div>`;
  root = document.querySelector("#app")!;
  api.validateConfig.mockResolvedValue([]);
  await open();
});

describe("表示", () => {
  it("キー割り当て: 振り分け先と操作キーを並べ、変更がなければ保存できない", () => {
    expect(store.screen).toBe("settings");
    expect(keys()).toEqual(["1", "2"]);
    expect(root.querySelectorAll(".action-row")).toHaveLength(7);
    expect(root.querySelector<HTMLInputElement>('[data-field="target-name"]')!.placeholder).toBe("風景");
    expect(saveButton().disabled).toBe(true);
    expect(root.querySelector('[data-slot="issues"]')!.textContent).toBe("問題はありません");
  });

  it("各セクションに切り替えられる", async () => {
    await click("section", { section: "general" });
    expect(root.querySelector("h1")!.textContent).toBe("一般");
    expect(root.querySelector('[data-action="clear-delete-folder"]')).toBeNull();
    await click("section", { section: "formats" });
    expect(root.querySelector(".format-list")!.textContent).toContain("このビルドでは未対応");
    await click("section", { section: "file" });
    expect(root.querySelector(".path-box")!.textContent).toBe("~/.config/lumiwake/config.toml");
  });

  it("設定の問題をキーの欄と一覧に出し、エラーがあれば保存できない", async () => {
    const payload = makeConfig();
    payload.issues = [
      { severity: "warning", message: "F5 は予約されています", key: "2" },
      { severity: "error", message: "キー 1 が重複しています", key: "1" },
    ];
    await open(payload);
    const btn = root.querySelector<HTMLElement>('[data-key="1"]')!;
    expect(btn.classList.contains("has-error")).toBe(true);
    expect(btn.title).toBe("キー 1 が重複しています");
    expect(root.querySelector('[data-key="2"]')!.classList.contains("has-warning")).toBe(true);
    expect(root.querySelector(".issues")!.textContent).toBe("注意: F5 は予約されていますエラー: キー 1 が重複しています");
    expect(saveButton().disabled).toBe(true);
  });
});

describe("キーの登録", () => {
  it("欄をクリックしてキーを押すと割り当て、検証してから保存できるようにする", async () => {
    await click("capture-target", { index: "1" });
    expect(s.isCapturing()).toBe(true);
    expect(root.querySelector(".capturing")!.textContent).toContain("キーを押してください");
    vi.useFakeTimers();
    s.handleCaptureKey("Ctrl+5");
    expect(s.isCapturing()).toBe(false);
    expect(keys()).toEqual(["1", "Ctrl+5"]);
    await vi.advanceTimersByTimeAsync(150);
    expect(api.validateConfig).toHaveBeenCalledWith(expect.objectContaining({ targets: expect.any(Array) }));
    expect(saveButton().disabled).toBe(false);
  });

  it("操作キーも変更でき、Esc で登録をやめる", async () => {
    await click("capture-action", { name: "skip" });
    s.handleCaptureKey("Escape");
    expect(s.isCapturing()).toBe(false);
    await click("capture-action", { name: "skip" });
    s.handleCaptureKey("Enter");
    expect(root.querySelector('[data-name="skip"]')!.getAttribute("data-key")).toBe("Enter");
  });

  it("検証に失敗したらエラーを表示する", async () => {
    vi.useFakeTimers();
    api.validateConfig.mockRejectedValue("検証できません");
    input("target-name", "海", false, { index: "0" });
    await vi.advanceTimersByTimeAsync(150);
    expect(toast).toHaveBeenCalledWith("検証できません", "error");
  });
});

describe("振り分け先の編集", () => {
  it("追加すると空いているキーを割り当てる", async () => {
    dialog.open.mockResolvedValueOnce("/home/u/写真/旅行").mockResolvedValueOnce(null);
    await click("target-add");
    expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "/home/u/写真/people" }));
    expect(keys()).toEqual(["1", "2", "3"]);
    await click("target-add"); // キャンセル
    expect(keys()).toHaveLength(3);
  });

  it("サブフォルダをまとめて追加する（登録済み・削除フォルダは除く）", async () => {
    await open(makeConfig({ delete_folder: "/p/ごみ" }));
    dialog.open.mockResolvedValue("/p");
    api.listSubfolders.mockResolvedValue(["/home/u/写真/風景", "/p/ごみ", "/p/a", "/p/b"]);
    await click("target-add-sub");
    expect(api.listSubfolders).toHaveBeenCalledWith("/p");
    expect(keys()).toEqual(["1", "2", "3", "4"]);
    expect(toast).toHaveBeenCalledWith("2 個のフォルダを追加しました");

    api.listSubfolders.mockResolvedValue(["/p/a"]);
    await click("target-add-sub");
    expect(toast).toHaveBeenLastCalledWith("追加できるフォルダがありませんでした");

    api.listSubfolders.mockRejectedValue("読み込めません");
    await click("target-add-sub");
    expect(toast).toHaveBeenLastCalledWith("読み込めません", "error");
  });

  it("フォルダの変更・並べ替え・削除", async () => {
    dialog.open.mockResolvedValue("/x/新しい");
    await click("target-path", { index: "0" });
    expect(root.querySelector(".path-cell")!.textContent!.trim()).toBe("/x/新しい");
    await click("target-up", { index: "1" });
    expect(keys()).toEqual(["2", "1"]);
    await click("target-up", { index: "0" });
    expect(keys()).toEqual(["2", "1"]);
    await click("target-remove", { index: "0" });
    expect(keys()).toEqual(["1"]);
    await click("target-remove", { index: "0" });
    expect(root.querySelector(".key-table")!.textContent).toContain("振り分け先はまだありません");
  });

  it("使えるキーがなくなるまで順に割り当てる", async () => {
    const payload = makeConfig();
    payload.config.targets = [];
    await open(payload);
    dialog.open.mockResolvedValue("/d");
    for (let i = 0; i < 12; i++) await click("target-add");
    expect(keys().slice(8)).toEqual(["9", "0", "Ctrl+1", "Ctrl+2"]);
  });
});

describe("一般", () => {
  it("削除フォルダの指定と解除", async () => {
    await click("section", { section: "general" });
    dialog.open.mockResolvedValue("/home/u/ごみ");
    await click("pick-delete-folder");
    expect(root.querySelector(".path-box")!.textContent).toBe("~/ごみ");
    await click("clear-delete-folder");
    expect(root.querySelector(".path-box")!.textContent).toContain("未指定");
  });

  it("入力欄の変更を下書きに反映する", async () => {
    input("on_exit", "delete");
    input("view_mode", "focus");
    input("accent", "green");
    input("include_subdirs", "", true);
    input("check_updates", "", false);
    input("show_paths", "", false);
    input("prefetch", "99");
    input("target-name", "海", false, { index: "0" });
    expect(document.documentElement.dataset.accent).toBe("green");
    api.saveConfig.mockImplementation(async (c) => ({ ...makeConfig(), config: c }));
    await click("save-config");
    const saved = api.saveConfig.mock.calls[0][0];
    expect(saved.general).toMatchObject({
      on_exit: "delete",
      view_mode: "focus",
      accent: "green",
      include_subdirs: true,
      check_updates: false,
      show_paths: false,
      prefetch: 16,
    });
    expect(saved.targets[0].name).toBe("海");
    input("prefetch", "abc");
    input("unknown", "x");
  });
});

describe("保存・破棄・再読み込み", () => {
  it("保存すると設定を差し替え、保存ボタンを戻す", async () => {
    const saved = makeConfig({ prefetch: 2 });
    api.saveConfig.mockResolvedValue(saved);
    input("prefetch", "2");
    await click("save-config");
    expect(store.config).toBe(saved);
    expect(toast).toHaveBeenCalledWith("設定を保存しました");
    expect(saveButton().disabled).toBe(true);

    api.saveConfig.mockRejectedValue("キーが重複しています");
    input("prefetch", "3");
    await click("save-config");
    expect(toast).toHaveBeenLastCalledWith("キーが重複しています", "error");
  });

  it("元に戻すと下書きを捨てる", async () => {
    await click("target-remove", { index: "0" });
    await click("revert-config");
    expect(keys()).toEqual(["1", "2"]);
  });

  it("変更があれば閉じる前に確認する", async () => {
    store.session = makeSession();
    input("prefetch", "2");
    dialog.ask.mockResolvedValueOnce(false);
    await click("close-settings");
    expect(store.screen).toBe("settings");
    dialog.ask.mockResolvedValueOnce(true);
    await click("close-settings");
    expect(store.screen).toBe("sort");
    expect(await click("section", { section: "keys" })).toBe(false);
  });

  it("変更がなければ確認せずに閉じる", async () => {
    await s.closeSettings();
    expect(dialog.ask).not.toHaveBeenCalled();
    expect(store.screen).toBe("start");
  });

  it("ファイルから再読み込み（編集中なら確認する）", async () => {
    const loaded = makeConfig({ prefetch: 9 });
    api.reloadConfig.mockResolvedValue(loaded);
    await click("reload-config");
    expect(store.config).toBe(loaded);
    expect(toast).toHaveBeenCalledWith("設定ファイルを読み込み直しました");

    input("prefetch", "2");
    dialog.ask.mockResolvedValueOnce(false);
    await click("reload-config");
    expect(api.reloadConfig).toHaveBeenCalledTimes(1);

    api.reloadConfig.mockRejectedValue("TOML の書式が正しくありません");
    dialog.ask.mockResolvedValueOnce(true);
    await click("reload-config");
    expect(toast).toHaveBeenLastCalledWith("TOML の書式が正しくありません", "error");
  });

  it("設定ファイルを開く（失敗したらエラー表示）", async () => {
    api.openConfigFile.mockRejectedValue("開けません");
    await click("open-config-file");
    expect(toast).toHaveBeenCalledWith("開けません", "error");
    expect(await click("no-such-action")).toBe(false);
  });
});
