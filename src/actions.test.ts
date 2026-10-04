// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  perform: vi.fn(),
  resolveConflict: vi.fn(),
  cancelConflict: vi.fn(),
  undo: vi.fn(),
  navigate: vi.fn(),
  jumpTo: vi.fn(),
  imageInfo: vi.fn(),
  startSession: vi.fn(),
  getConfig: vi.fn(),
}));
const toast = vi.hoisted(() => vi.fn());
vi.mock("./api", async (orig) => ({ ...(await orig<typeof import("./api")>()), api }));
vi.mock("./toast", () => ({ toast }));

import * as actions from "./actions";
import { onChange, store } from "./store";
import { makeConfig, makeSession } from "./test/fixtures";

const info = { name: "c.jpg", path: "/c.jpg", size: 1, modified: null, width: 4, height: 3, taken: null };

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  store.session = null;
  store.info = null;
  store.screen = "start";
  api.imageInfo.mockResolvedValue(info);
});

describe("enqueue", () => {
  it("操作を 1 つずつ順番に処理し、溜まりすぎたら busy になる", async () => {
    const order: string[] = [];
    const first = deferred<void>();
    actions.enqueue(async () => {
      await first.promise;
      order.push("1");
    });
    actions.enqueue(async () => void order.push("2"));
    expect(actions.busy()).toBe(false);
    const last = actions.enqueue(async () => void order.push("3"));
    expect(actions.busy()).toBe(true);
    first.resolve();
    await last;
    expect(order).toEqual(["1", "2", "3"]);
    expect(actions.busy()).toBe(false);
  });

  it("失敗した操作はエラー表示し、後続の操作は続ける", async () => {
    actions.enqueue(() => Promise.reject("移動できません"));
    const ran = vi.fn();
    await actions.enqueue(async () => ran());
    expect(toast).toHaveBeenCalledWith("移動できません", "error");
    expect(ran).toHaveBeenCalled();
  });
});

describe("setSession", () => {
  it("現在の画像が変わったら詳細を取り直して通知する", async () => {
    const listener = vi.fn();
    onChange(listener);
    actions.setSession(makeSession());
    expect(store.info).toEqual({ generation: 1, index: 2, info: null });
    expect(api.imageInfo).toHaveBeenCalledWith(1, 2);
    await vi.waitFor(() => expect(store.info?.info).toEqual(info));
    expect(listener).toHaveBeenCalledTimes(2);

    // 同じ画像なら取り直さない
    actions.setSession(makeSession({ remaining: 2 }));
    expect(api.imageInfo).toHaveBeenCalledTimes(1);
  });

  it("古い画像の詳細やエラーは捨てる", async () => {
    const slow = deferred<typeof info>();
    api.imageInfo.mockReturnValueOnce(slow.promise).mockRejectedValueOnce("壊れた画像");
    actions.setSession(makeSession());
    actions.setSession(makeSession({ current: { index: 3, name: "d.jpg", path: "/d.jpg" } }));
    slow.resolve(info);
    await vi.waitFor(() => expect(toast).toHaveBeenCalledWith("画像を読み込めません: 壊れた画像", "error"));
    expect(store.info).toEqual({ generation: 1, index: 3, info: null });
  });

  it("すべて処理し終えたら詳細を消す", () => {
    store.info = { generation: 1, index: 2, info };
    actions.setSession(makeSession({ current: null }));
    expect(store.info).toBeNull();
    actions.setSession(null);
    expect(store.session).toBeNull();
  });
});

describe("操作", () => {
  const result = { view: makeSession({ canUndo: true }), message: null };

  it("振り分け・同名ファイルの解決・取り消しの結果を画面に反映する", async () => {
    api.perform.mockResolvedValue(result);
    await actions.perform({ kind: "move", target: 0 });
    expect(api.perform).toHaveBeenCalledWith({ kind: "move", target: 0 });
    expect(store.session?.canUndo).toBe(true);

    api.resolveConflict.mockResolvedValue(result);
    await actions.resolveConflict("overwrite");
    expect(api.resolveConflict).toHaveBeenCalledWith("overwrite");

    api.undo.mockResolvedValue({ view: makeSession(), message: "取り消しました: c.jpg" });
    await actions.undo();
    expect(toast).toHaveBeenCalledWith("取り消しました: c.jpg");
    expect(store.session?.canUndo).toBe(false);
  });

  it("移動系の操作", async () => {
    const view = makeSession({ total: 9 });
    api.cancelConflict.mockResolvedValue(view);
    api.navigate.mockResolvedValue(view);
    api.jumpTo.mockResolvedValue(view);
    await actions.cancelConflict();
    await actions.navigate(false);
    await actions.jumpTo(1);
    expect(api.navigate).toHaveBeenCalledWith(false);
    expect(api.jumpTo).toHaveBeenCalledWith(1);
    expect(store.session?.total).toBe(9);
  });
});

describe("startSession", () => {
  it("読み込んで仕分け画面に切り替え、未対応の形式を知らせる", async () => {
    api.startSession.mockResolvedValue(makeSession({ unsupported: ["/a/x.heic", "/a/y.HEIC", "/a/z.avif"] }));
    api.getConfig.mockResolvedValue(makeConfig());
    await actions.startSession("/a", true);
    expect(api.startSession).toHaveBeenCalledWith("/a", true);
    expect(store.screen).toBe("sort");
    expect(store.config).toEqual(makeConfig());
    expect(toast).toHaveBeenCalledWith(expect.stringMatching(/^HEIC \/ AVIF の 3 枚は/), "info", 6000);
  });

  it("画像がなければ知らせる", async () => {
    api.startSession.mockResolvedValue(makeSession({ total: 0, current: null }));
    api.getConfig.mockResolvedValue(makeConfig());
    await actions.startSession("/empty", false);
    expect(toast).toHaveBeenCalledWith("対象の画像が見つかりませんでした");
  });
});
