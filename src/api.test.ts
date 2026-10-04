import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { api, errorText, lumiUrl } from "./api";
import { makeConfig } from "./test/fixtures";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
});

describe("api", () => {
  // コマンド名と引数名は Rust 側（commands.rs）の関数名・引数名と一致していないと呼べない
  it.each([
    ["getConfig", [], "get_config", undefined],
    ["validateConfig", [makeConfig().config], "validate_config", { config: makeConfig().config }],
    ["saveConfig", [makeConfig().config], "save_config", { config: makeConfig().config }],
    ["reloadConfig", [], "reload_config", undefined],
    ["setViewMode", ["focus"], "set_view_mode", { mode: "focus" }],
    ["setShowPaths", [false], "set_show_paths", { show: false }],
    ["openConfigFile", [], "open_config_file", undefined],
    ["openLocation", ["/a"], "open_location", { path: "/a" }],
    ["listSubfolders", ["/a"], "list_subfolders", { path: "/a" }],
    ["startSession", ["/src", true], "start_session", { source: "/src", includeSubdirs: true }],
    ["getSession", [], "get_session", undefined],
    ["perform", [{ kind: "move", target: 2 }], "perform", { action: { kind: "move", target: 2 } }],
    ["resolveConflict", ["keepBoth"], "resolve_conflict", { choice: "keepBoth" }],
    ["cancelConflict", [], "cancel_conflict", undefined],
    ["undo", [], "undo", undefined],
    ["navigate", [false], "navigate", { forward: false }],
    ["jumpTo", [3], "jump_to", { index: 3 }],
    ["imageInfo", [7, 1], "image_info", { generation: 7, index: 1 }],
    ["pendingDeletions", [], "pending_deletions", undefined],
    ["finalizeAndExit", [true], "finalize_and_exit", { delete: true }],
    ["exitApp", [], "exit_app", undefined],
  ] as const)("%s → %s", async (method, args, command, payload) => {
    await (api[method] as (...a: unknown[]) => Promise<unknown>)(...args);
    expect(invoke).toHaveBeenCalledTimes(1);
    if (payload === undefined) expect(invoke).toHaveBeenCalledWith(command);
    else expect(invoke).toHaveBeenCalledWith(command, payload);
  });
});

describe("errorText", () => {
  it("文字列・Error・その他を表示用の文字列にする", () => {
    expect(errorText("移動できません")).toBe("移動できません");
    expect(errorText(new Error("boom"))).toBe("boom");
    expect(errorText(404)).toBe("404");
  });
});

describe("lumiUrl", () => {
  it("Linux（テスト環境）では lumi:// を使う", () => {
    expect(lumiUrl("preview/1/2")).toBe("lumi://localhost/preview/1/2");
  });
});
