// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const updater = vi.hoisted(() => ({ check: vi.fn() }));
const dialog = vi.hoisted(() => ({ ask: vi.fn() }));
const proc = vi.hoisted(() => ({ relaunch: vi.fn() }));
const api = vi.hoisted(() => ({ pendingDeletions: vi.fn(), saveSimilarCache: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-updater", () => updater);
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("@tauri-apps/plugin-process", () => proc);
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api }));
vi.mock("./toast", () => ({ toast }));

import { checkForUpdates } from "./updater";

const banner = () => document.querySelector<HTMLElement>(".update-banner");
const press = (action: string) => banner()!.querySelector<HTMLElement>(`[data-update="${action}"]`)!.click();

function update(downloadAndInstall = vi.fn().mockResolvedValue(undefined)) {
  return { version: "0.2.0<script>", downloadAndInstall };
}

beforeEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = "";
  api.pendingDeletions.mockResolvedValue({ items: [], trashDir: null, onExit: "confirm" });
  api.saveSimilarCache.mockResolvedValue(undefined);
});

describe("checkForUpdates", () => {
  it("確認できなくても何も表示しない", async () => {
    vi.spyOn(console, "info").mockImplementation(() => {});
    updater.check.mockRejectedValue(new Error("offline"));
    await checkForUpdates();
    updater.check.mockResolvedValue(null);
    await checkForUpdates();
    expect(banner()).toBeNull();
  });

  it("新しいバージョンがあればバナーを出し、閉じられる", async () => {
    updater.check.mockResolvedValue(update());
    await checkForUpdates();
    expect(banner()!.textContent).toContain("新しいバージョン 0.2.0<script> があります");
    expect(banner()!.querySelector("script")).toBeNull();
    press("dismiss");
    expect(banner()).toBeNull();
  });

  it("更新して再起動する", async () => {
    const u = update();
    updater.check.mockResolvedValue(u);
    await checkForUpdates();
    press("install");
    await vi.waitFor(() => expect(proc.relaunch).toHaveBeenCalled());
    expect(u.downloadAndInstall).toHaveBeenCalled();
    expect(dialog.ask).not.toHaveBeenCalled();
  });

  it("更新を入れる前に類似判定の記録を保存する（Windows では更新を入れるとアプリが終了するため）", async () => {
    const order: string[] = [];
    api.saveSimilarCache.mockImplementation(async () => void order.push("save"));
    proc.relaunch.mockImplementation(async () => void order.push("relaunch"));
    updater.check.mockResolvedValue(update(vi.fn(async () => void order.push("install"))));
    await checkForUpdates();
    press("install");
    await vi.waitFor(() => expect(order).toEqual(["save", "install", "relaunch"]));
  });

  it("類似判定の記録を保存できなくても再起動する", async () => {
    api.saveSimilarCache.mockRejectedValue("保存できません");
    updater.check.mockResolvedValue(update());
    await checkForUpdates();
    press("install");
    await vi.waitFor(() => expect(proc.relaunch).toHaveBeenCalled());
    expect(toast).not.toHaveBeenCalledWith(expect.stringContaining("更新できませんでした"), "error");
  });

  it("削除予定があれば再起動の前に確認する", async () => {
    const u = update();
    updater.check.mockResolvedValue(u);
    api.pendingDeletions.mockResolvedValue({ items: [{}, {}], trashDir: null, onExit: "confirm" });
    dialog.ask.mockResolvedValue(false);
    await checkForUpdates();
    press("install");
    await vi.waitFor(() => expect(dialog.ask).toHaveBeenCalledWith(expect.stringContaining("2 件"), expect.anything()));
    expect(u.downloadAndInstall).not.toHaveBeenCalled();
  });

  it("更新に失敗したらエラーを表示する", async () => {
    updater.check.mockResolvedValue(update(vi.fn().mockRejectedValue("署名が一致しません")));
    await checkForUpdates();
    press("install");
    await vi.waitFor(() => expect(toast).toHaveBeenLastCalledWith("更新できませんでした: 署名が一致しません", "error"));
    expect(proc.relaunch).not.toHaveBeenCalled();
  });
});
