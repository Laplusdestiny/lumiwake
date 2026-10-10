// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const actions = vi.hoisted(() => ({ startSession: vi.fn() }));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
const api = vi.hoisted(() => ({ forgetSource: vi.fn() }));
vi.mock("../api", () => ({ api, errorText: String }));
vi.mock("../actions", () => actions);
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("../toast", () => ({ toast }));

import { store } from "../store";
import { makeConfig, makeSession } from "../test/fixtures";
import { chooseSource, forgetSource, moveRecentSource, pickSource, renderStart, startFromForm } from "./start";

let root: HTMLElement;
const startBtn = () => root.querySelector<HTMLButtonElement>('[data-action="start"]')!;

beforeEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = `<div id="app"></div>`;
  root = document.querySelector("#app")!;
  store.config = makeConfig({ source_dir: "/home/u/inbox", include_subdirs: true });
  store.session = null;
  store.home = "/home/u";
});

describe("renderStart", () => {
  it("前回の仕分け元とサブフォルダの設定を表示する", () => {
    renderStart(root);
    expect(root.querySelector(".path-box")!.textContent).toBe("~/inbox");
    expect(root.querySelector<HTMLInputElement>("#include-subdirs")!.checked).toBe(true);
    expect(startBtn().disabled).toBe(false);
    expect(root.querySelector('[data-action="back-to-sort"]')).toBeNull();
  });

  it("仕分け元が未選択・振り分け先なし・設定の読み込み失敗を知らせる", () => {
    const cfg = makeConfig();
    cfg.config.targets = [];
    cfg.loadError = "TOML の書式が正しくありません";
    store.config = cfg;
    store.session = makeSession();
    renderStart(root);
    expect(root.querySelector(".path-box")!.textContent).toBe("フォルダを選んでください");
    expect(startBtn().disabled).toBe(true);
    expect(root.querySelector(".notice-error")!.textContent).toContain("TOML の書式が正しくありません");
    expect(root.textContent).toContain("振り分け先がまだ登録されていません");
    expect(root.querySelector('[data-action="back-to-sort"]')).not.toBeNull();
  });

  it("設定がなければ何もしない", () => {
    store.config = null;
    renderStart(root);
    expect(root.innerHTML).toBe("");
  });
});

describe("開始", () => {
  it("選んだフォルダとチェックボックスの状態で読み込む", async () => {
    dialog.open.mockResolvedValue("/mnt/nas/写真");
    renderStart(root);
    await pickSource();
    expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ directory: true, defaultPath: "/home/u/inbox" }));
    expect(root.querySelector(".path-box")!.textContent).toBe("/mnt/nas/写真");
    root.querySelector<HTMLInputElement>("#include-subdirs")!.checked = false;
    await startFromForm();
    expect(actions.startSession).toHaveBeenCalledWith("/mnt/nas/写真", false);
  });

  it("フォルダ選択をキャンセルしたら何も変えない", async () => {
    dialog.open.mockResolvedValue(null);
    renderStart(root);
    await pickSource();
    expect(root.querySelector(".path-box")!.textContent).toBe("~/inbox");
  });

  it("仕分け元が未選択ならフォルダ選択を開く", async () => {
    store.config = makeConfig();
    dialog.open.mockResolvedValue(null);
    await startFromForm();
    expect(dialog.open).toHaveBeenCalled();
    expect(actions.startSession).not.toHaveBeenCalled();
  });

  it("読み込みに失敗したらエラーを表示する", async () => {
    actions.startSession.mockRejectedValue("フォルダを読み込めません");
    renderStart(root);
    await startFromForm();
    expect(toast).toHaveBeenCalledWith("フォルダを読み込めません", "error");
  });
});

describe("最近使ったフォルダ", () => {
  const recent = ["/home/u/inbox", "/mnt/nas/写真", "/home/u/old/写真"];
  const selected = () => root.querySelector(".recent-item.selected .recent-path")?.textContent;

  beforeEach(() => {
    store.config = makeConfig({ source_dir: "/home/u/inbox", recent_sources: recent });
    // 前のテストで選んだフォルダを残さない
    chooseSource("/home/u/inbox");
  });

  it("同名フォルダも区別できるよう、名前とパスを並べて前回の仕分け元を選択中にする", () => {
    renderStart(root);
    const items = [...root.querySelectorAll(".recent-item")];
    expect(items.map((i) => i.querySelector(".recent-name")!.textContent)).toEqual(["inbox", "写真", "写真"]);
    expect(items.map((i) => i.querySelector(".recent-path")!.textContent)).toEqual(["~/inbox", "/mnt/nas/写真", "~/old/写真"]);
    expect(selected()).toBe("~/inbox");
    expect(root.textContent).toContain("↑↓ で履歴から選択");
  });

  it("履歴がなければ一覧を出さない", () => {
    store.config = makeConfig({ source_dir: "/home/u/inbox" });
    renderStart(root);
    expect(root.querySelector(".recent")).toBeNull();
    expect(root.textContent).not.toContain("↑↓");
  });

  it("選んだフォルダで読み込む", async () => {
    chooseSource("/mnt/nas/写真");
    expect(root.querySelector(".path-box")!.textContent).toBe("/mnt/nas/写真");
    expect(selected()).toBe("/mnt/nas/写真");
    await startFromForm();
    expect(actions.startSession).toHaveBeenCalledWith("/mnt/nas/写真", false);
  });

  it("↑↓ で履歴の中を移り、端で止まる", () => {
    moveRecentSource(1);
    moveRecentSource(1);
    expect(selected()).toBe("~/old/写真");
    moveRecentSource(1);
    expect(selected()).toBe("~/old/写真");
    moveRecentSource(-1);
    moveRecentSource(-1);
    moveRecentSource(-1);
    expect(selected()).toBe("~/inbox");
  });

  it("ダイアログで履歴にないフォルダを選んでいれば、↓ で履歴の先頭へ", async () => {
    dialog.open.mockResolvedValue("/tmp/new");
    await pickSource();
    expect(selected()).toBeUndefined();
    moveRecentSource(1);
    expect(selected()).toBe("~/inbox");
  });

  it("履歴から外したら、選んでいたフォルダの選択も外す", async () => {
    chooseSource("/mnt/nas/写真");
    const after = makeConfig({ source_dir: "/home/u/inbox", recent_sources: ["/home/u/inbox", "/home/u/old/写真"] });
    api.forgetSource.mockResolvedValue(after);
    await forgetSource("/mnt/nas/写真");
    expect(api.forgetSource).toHaveBeenCalledWith("/mnt/nas/写真");
    expect(store.config).toBe(after);
    renderStart(root);
    expect(root.querySelector(".path-box")!.textContent).toBe("~/inbox");
    expect(root.querySelectorAll(".recent-item")).toHaveLength(2);
  });
});
