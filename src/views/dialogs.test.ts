// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  pendingDeletions: vi.fn(),
  exitApp: vi.fn(),
  finalizeAndExit: vi.fn(),
  openLocation: vi.fn(),
}));
const actions = vi.hoisted(() => ({ resolveConflict: vi.fn(), cancelConflict: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), api }));
vi.mock("../actions", () => actions);
vi.mock("../toast", () => ({ toast }));

import type { DeletionSummary } from "../api";
import { store } from "../store";
import { makeConfig, makeSession } from "../test/fixtures";
import { handleConflictKey, handleExitAction, handleExitKey, renderOverlay, requestQuit } from "./dialogs";

const overlay = () => document.querySelector<HTMLElement>("#overlay")!;
const text = (sel: string) => overlay().querySelector(sel)?.textContent?.replace(/\s+/g, " ").trim();
const el = (path?: string) => {
  const b = document.createElement("button");
  if (path) b.dataset.path = path;
  return b;
};

const conflict = {
  item: 2,
  label: "風景",
  dir: "/home/u/写真/風景",
  incoming: { name: "c.jpg", path: "/home/u/inbox/c.jpg", size: 2048, modified: null },
  existing: { name: "c.jpg", path: "/home/u/写真/風景/c.jpg", size: null, modified: new Date(2026, 0, 2, 3, 4).getTime() },
};

function summary(over: Partial<DeletionSummary> = {}): DeletionSummary {
  return {
    items: [
      { path: "/home/u/ごみ/a.jpg", original: "/home/u/inbox/a.jpg", reason: "deleteKey", inTrash: true },
      { path: "/home/u/inbox/b.jpg", original: "/home/u/inbox/b.jpg", reason: "keepExisting", inTrash: false },
    ],
    trashDir: "/home/u/ごみ",
    onExit: "confirm",
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = "";
  store.config = makeConfig();
  store.session = makeSession();
  store.screen = "sort";
  store.home = "/home/u";
  store.modal = false;
});

afterEach(async () => {
  // 終了確認の状態はモジュール内に残るので、テストごとに閉じておく
  await handleExitAction("exit-cancel", el());
});

describe("同名ファイル", () => {
  it("両方の画像と 4 つの選択肢を表示する", () => {
    store.session = makeSession({ conflict });
    renderOverlay();
    expect(text("h2")).toBe("同じ名前のファイルがあります");
    expect(text("p")).toContain("振り分け先「風景」（~/写真/風景）に c.jpg がすでにあります");
    const imgs = [...overlay().querySelectorAll("img")].map((i) => i.getAttribute("src"));
    expect(imgs).toEqual(["lumi://localhost/conflict/1/incoming?item=2", "lumi://localhost/conflict/1/existing?item=2"]);
    expect(text(".compare-card:first-child .compare-meta .muted")).toBe("2.0 KB");
    expect(text(".compare-card:last-child .compare-meta .muted")).toBe("2026-01-02 03:04 更新");
    const choices = [...overlay().querySelectorAll<HTMLElement>("[data-choice]")].map((b) => b.dataset.choice);
    expect(choices).toEqual(["keepExisting", "overwrite", "keepBoth", "skip"]);
  });

  it("数字キー（テンキー含む）で選び、Esc で取り消す。他のキーは受け付けない", () => {
    store.session = makeSession({ conflict });
    expect(handleConflictKey("1")).toBe(true);
    expect(actions.resolveConflict).toHaveBeenLastCalledWith("keepExisting");
    expect(handleConflictKey("Num3")).toBe(true);
    expect(actions.resolveConflict).toHaveBeenLastCalledWith("keepBoth");
    expect(handleConflictKey("Escape")).toBe(true);
    expect(actions.cancelConflict).toHaveBeenCalled();
    expect(handleConflictKey("Space")).toBe(true);
    expect(actions.resolveConflict).toHaveBeenCalledTimes(2);
  });

  it("確認中でなければキーを受け取らず、何も表示しない", () => {
    expect(handleConflictKey("1")).toBe(false);
    renderOverlay();
    expect(overlay().innerHTML).toBe("");
  });
});

describe("終了時の削除確認", () => {
  it("削除予定がなければそのまま終了する", async () => {
    api.pendingDeletions.mockResolvedValue(summary({ items: [] }));
    await requestQuit();
    expect(api.exitApp).toHaveBeenCalled();
    expect(api.finalizeAndExit).not.toHaveBeenCalled();
  });

  it("「確認せずに削除」設定なら確認なしで完全削除する", async () => {
    api.pendingDeletions.mockResolvedValue(summary({ onExit: "delete" }));
    api.finalizeAndExit.mockResolvedValue({ deleted: ["a", "b"], failed: [] });
    await requestQuit();
    expect(api.finalizeAndExit).toHaveBeenCalledWith(true);
    expect(store.modal).toBe(false);
  });

  it("確認ダイアログに件数・置き場所・理由を出す", async () => {
    api.pendingDeletions.mockResolvedValue(summary());
    await requestQuit();
    expect(store.modal).toBe(true);
    renderOverlay();
    expect(text("h2")).toBe("削除予定のファイルが 2 件あります");
    expect(text("p")).toContain("削除フォルダ内の 1 件と、元の場所で削除予定にした 1 件を完全に削除しますか");
    expect(text(".trash-line")).toContain("~/ごみ");
    const items = [...overlay().querySelectorAll(".pending-main")].map((e) => e.textContent!.replace(/\s+/g, " ").trim());
    expect(items).toEqual(["a.jpg ~/ごみ/a.jpg 削除キー", "b.jpg ~/inbox/b.jpg 同名ファイルがあったため（既存を残す）"]);
    expect(document.activeElement?.getAttribute("data-action")).toBe("exit-cancel");
    // 表示中は仕分けのキーを受け付けない
    expect(handleExitKey("1")).toBe(true);
    expect(handleExitKey("Escape")).toBe(true);
    expect(store.modal).toBe(false);
    expect(handleExitKey("Escape")).toBe(false);
  });

  it("削除フォルダがなければ削除フォルダの行を出さない", async () => {
    api.pendingDeletions.mockResolvedValue(summary({ trashDir: null, items: [summary().items[1]] }));
    await requestQuit();
    renderOverlay();
    expect(overlay().querySelector(".trash-line")).toBeNull();
    expect(text("p")).toContain("元の場所で削除予定にした 1 件を");
  });

  it("「削除せずに終了」はファイルを消さずに終了する", async () => {
    api.pendingDeletions.mockResolvedValue(summary());
    api.finalizeAndExit.mockResolvedValue({ deleted: [], failed: [] });
    await requestQuit();
    await handleExitAction("exit-keep", el());
    expect(api.finalizeAndExit).toHaveBeenCalledWith(false);
  });

  it("削除に失敗したファイルを一覧にし、そのまま終了もできる", async () => {
    api.pendingDeletions.mockResolvedValue(summary());
    api.finalizeAndExit.mockResolvedValue({ deleted: ["/home/u/ごみ/a.jpg"], failed: [["/home/u/inbox/b.jpg", "使用中です"]] });
    await requestQuit();
    await handleExitAction("exit-delete", el());
    expect(api.finalizeAndExit).toHaveBeenCalledWith(true);
    renderOverlay();
    expect(text("h2")).toBe("一部のファイルを削除できませんでした");
    expect(text("p")).toBe("1 件を削除しました。次の 1 件は削除できなかったため、そのまま残っています。");
    expect(text(".pending-list")).toContain("使用中です");
    await handleExitAction("exit-force", el());
    expect(api.exitApp).toHaveBeenCalled();
  });

  it("確認中に再度終了しようとしても重ねて開かない", async () => {
    api.pendingDeletions.mockResolvedValue(summary());
    await requestQuit();
    await requestQuit();
    expect(api.pendingDeletions).toHaveBeenCalledTimes(1);
  });

  it("削除予定を取得できなければエラーを表示する", async () => {
    api.pendingDeletions.mockRejectedValue("取得できません");
    await requestQuit();
    expect(toast).toHaveBeenCalledWith("取得できません", "error");
    expect(api.exitApp).not.toHaveBeenCalled();
  });

  it("場所を開く（失敗したらエラー表示）", async () => {
    api.openLocation.mockResolvedValueOnce(undefined).mockRejectedValueOnce("開けません");
    expect(await handleExitAction("open-location", el("/home/u/ごみ"))).toBe(true);
    expect(api.openLocation).toHaveBeenCalledWith("/home/u/ごみ");
    await handleExitAction("open-location", el("/x"));
    expect(toast).toHaveBeenCalledWith("開けません", "error");
    expect(await handleExitAction("unknown", el())).toBe(false);
  });
});
