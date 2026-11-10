// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 本物と同じく、操作は 1 つずつ順番に処理する
const actions = vi.hoisted(() => {
  let chain: Promise<void> = Promise.resolve();
  const enqueue = (task: () => Promise<void>): Promise<void> => (chain = chain.then(task));
  return {
    jumpTo: vi.fn(),
    undo: vi.fn(() => enqueue(async () => {})),
    setSession: vi.fn(),
    busy: vi.fn(() => false),
    enqueue: vi.fn(enqueue),
  };
});
const api = vi.hoisted(() => ({
  startSimilarScan: vi.fn(),
  similarStatus: vi.fn(),
  similarGroups: vi.fn(),
  deleteSimilar: vi.fn(),
  keepOnly: vi.fn(),
}));
const toast = vi.hoisted(() => vi.fn());
vi.mock("../actions", () => actions);
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), api }));
vi.mock("../toast", () => ({ toast }));

import type { SimilarGroups, SimilarStatus } from "../api";
import { makeConfig, makeSession, makeSimilarGroups, makeSimilarStatus } from "../test/fixtures";

type Similar = typeof import("./similar");
type Store = typeof import("../store")["store"];

let root: HTMLElement;
let v: Similar;
let store: Store;

/** 画面はモジュール内に選択・閾値・絞り込みを持つので、テストごとに読み込み直す */
async function load(): Promise<void> {
  vi.resetModules();
  const storeMod = await import("../store");
  store = storeMod.store;
  v = await import("./similar");
  store.config = makeConfig();
  store.session = makeSession();
  store.screen = "sort";
  store.similar = null;
  storeMod.onChange(() => store.screen === "similar" && v.renderSimilar(root));
}

async function open(groups: SimilarGroups = makeSimilarGroups(), status: SimilarStatus = makeSimilarStatus()): Promise<void> {
  api.startSimilarScan.mockResolvedValue(status);
  api.similarStatus.mockResolvedValue(status);
  api.similarGroups.mockResolvedValue(groups);
  await v.openSimilar();
}

const selected = () => Number(root.querySelector<HTMLElement>(".similar-tile.is-selected")?.dataset.index ?? NaN);
const tile = (i: number) => root.querySelector<HTMLElement>(`.similar-tile[data-index="${i}"]`);
const clean = (el: Element | null | undefined) => el?.textContent?.replace(/\s+/g, " ").trim() ?? "";
const slot = (name: string) => root.querySelector<HTMLElement>(`[data-slot="${name}"]`);
const shown = () =>
  [...root.querySelectorAll<HTMLElement>(".similar-group")].map((g) =>
    [...g.querySelectorAll<HTMLElement>(".similar-tile")].map((t) => Number(t.dataset.index)),
  );
const badges = (group: number) =>
  [...root.querySelectorAll(".similar-group")[group].querySelectorAll(".similar-badge")].map((b) => b.textContent);
const thresholdInput = () => root.querySelector<HTMLInputElement>('[data-similar="threshold"]')!;
const filterSelect = () => root.querySelector<HTMLSelectElement>('[data-similar="filter"]')!;
const key = (combo: string, repeat = false) => v.handleSimilarKey(combo, { repeat });

/** 1 枚を消したあとの取り直しの結果 */
function without(...gone: number[]): SimilarGroups {
  const all = makeSimilarGroups();
  return {
    ...all,
    groups: all.groups
      .map((g) => ({ ...g, members: g.members.filter((m) => !gone.includes(m.index)) }))
      .filter((g) => g.members.length > 1),
  };
}

beforeEach(async () => {
  vi.clearAllMocks();
  actions.busy.mockReturnValue(false);
  localStorage.clear();
  document.body.innerHTML = `<div id="app"></div>`;
  root = document.querySelector("#app")!;
  await load();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("開く・走査", () => {
  it("開くと走査を始め、既定の閾値でグループを取り、現在の画像を選ぶ", async () => {
    await open();
    expect(api.startSimilarScan).toHaveBeenCalledTimes(1);
    expect(api.similarGroups).toHaveBeenCalledWith(6);
    expect(store.screen).toBe("similar");
    expect(root.dataset.view).toBe("similar");
    expect(shown()).toEqual([
      [1, 2, 4],
      [5, 7],
    ]);
    expect(selected()).toBe(2);
    expect(clean(slot("similar-progress"))).toBe("2 グループ・5 枚");
    expect(clean(slot("similar-failed"))).toBe("");
  });

  it("現在の画像がグループになければ先頭を選ぶ", async () => {
    store.session = makeSession({ current: { index: 0, name: "z.jpg", path: "/z.jpg" } });
    const groups = makeSimilarGroups();
    groups.groups[0].members[1].current = false;
    await open(groups);
    expect(selected()).toBe(1);
  });

  it("走査中は進捗を出し、進むたびにグループを取り直し、終わったらポーリングを止める", async () => {
    vi.useFakeTimers();
    await open(makeSimilarGroups({ groups: [] }), makeSimilarStatus({ done: 2, total: 8, running: true }));
    expect(clean(slot("similar-progress"))).toBe("判定中 2 / 8");
    expect(clean(root.querySelector(".similar-empty"))).toBe("判定中…");
    expect(api.similarGroups).toHaveBeenCalledTimes(1);

    // 進んでいなければ取り直さない
    await vi.advanceTimersByTimeAsync(300);
    expect(api.similarStatus).toHaveBeenCalledTimes(1);
    expect(api.similarGroups).toHaveBeenCalledTimes(1);

    api.similarStatus.mockResolvedValue(makeSimilarStatus({ done: 5, total: 8, running: true }));
    api.similarGroups.mockResolvedValue(without(4, 5, 7));
    await vi.advanceTimersByTimeAsync(300);
    expect(clean(slot("similar-progress"))).toBe("判定中 5 / 8");
    expect(shown()).toEqual([[1, 2]]);

    api.similarStatus.mockResolvedValue(makeSimilarStatus({ failed: 1 }));
    api.similarGroups.mockResolvedValue(makeSimilarGroups());
    await vi.advanceTimersByTimeAsync(300);
    expect(clean(slot("similar-progress"))).toBe("2 グループ・5 枚");
    expect(clean(slot("similar-failed"))).toBe("読めなかった画像 1 枚");
    expect(shown()).toHaveLength(2);

    const calls = api.similarStatus.mock.calls.length;
    await vi.advanceTimersByTimeAsync(3000);
    expect(api.similarStatus).toHaveBeenCalledTimes(calls);
  });

  it("閉じたらポーリングを止める", async () => {
    vi.useFakeTimers();
    await open(makeSimilarGroups(), makeSimilarStatus({ done: 2, running: true }));
    v.closeSimilar();
    await vi.advanceTimersByTimeAsync(3000);
    expect(api.similarStatus).not.toHaveBeenCalled();
    expect(store.screen).toBe("sort");
    expect(store.similar).toBeNull();
  });

  it("似た画像がなければその旨を出す", async () => {
    await open(makeSimilarGroups({ groups: [] }));
    expect(clean(root.querySelector(".similar-empty"))).toBe("似た画像は見つかりませんでした");
    expect(clean(slot("similar-progress"))).toBe("0 グループ");
    expect(key("Right")).toBe(true);
    expect(key("Delete")).toBe(true);
    expect(api.deleteSimilar).not.toHaveBeenCalled();
  });

  it("同名ファイルの確認中・セッションがないときは開かない", async () => {
    store.session = makeSession({ conflict: {} as never });
    await open();
    expect(api.startSimilarScan).not.toHaveBeenCalled();
    store.session = null;
    await open();
    expect(api.startSimilarScan).not.toHaveBeenCalled();
    expect(store.screen).toBe("sort");
  });

  it("走査の開始やグループの取得に失敗したらトーストを出して仕分け画面のまま", async () => {
    api.startSimilarScan.mockRejectedValue("読み込まれていません");
    await v.openSimilar();
    expect(toast).toHaveBeenCalledWith("読み込まれていません", "error");
    expect(store.screen).toBe("sort");

    api.startSimilarScan.mockResolvedValue(makeSimilarStatus());
    api.similarGroups.mockRejectedValue("失敗");
    await v.openSimilar();
    expect(toast).toHaveBeenCalledWith("失敗", "error");
    expect(store.screen).toBe("sort");
  });

  it("並び順が変わって generation が変わったら閉じる", async () => {
    await open();
    store.session = makeSession({ generation: 2 });
    v.renderSimilar(root);
    expect(store.screen).toBe("sort");
    expect(store.similar).toBeNull();
  });

  it("同名ファイルの確認が始まったら閉じる", async () => {
    await open();
    store.session = makeSession({ conflict: {} as never });
    v.renderSimilar(root);
    expect(store.screen).toBe("sort");
  });
});

describe("表示", () => {
  it("グループの根拠のバッジと、画像の情報・目印を出す", async () => {
    await open();
    expect(badges(0)).toEqual(["名前が似ている"]);
    expect(badges(1)).toEqual(["完全一致", "サイズが同じ", "解像度が同じ"]);
    expect(clean(root.querySelector(".similar-group-title"))).toBe("グループ 1・3 枚");
    expect(clean(tile(1)!.querySelector(".similar-name"))).toBe("a.jpg");
    expect(clean(tile(1)!.querySelector(".similar-meta"))).toBe("800×600・2.0 KB・類似度 97%");
    expect([...tile(1)!.querySelectorAll(".similar-mark")].map((m) => m.textContent)).toEqual(["解像度が最大", "サイズが最大"]);
    expect([...tile(2)!.querySelectorAll(".similar-mark")].map((m) => m.textContent)).toEqual(["現在の画像"]);
    expect(tile(4)!.querySelectorAll(".similar-mark")).toHaveLength(0);
    expect(tile(1)!.querySelector("img")!.getAttribute("src")).toBe("lumi://localhost/grid/1/1");
    expect(clean(slot("similar-threshold"))).toBe("類似度 91% 以上");
  });

  it("ファイル名はエスケープする", async () => {
    const groups = makeSimilarGroups();
    groups.groups[0].members[0].name = `<img src=x onerror="1">.jpg`;
    await open(groups);
    expect(tile(1)!.querySelector(".similar-name")!.textContent).toBe(`<img src=x onerror="1">.jpg`);
    expect(tile(1)!.querySelectorAll("img")).toHaveLength(1);
  });

  it("サムネイルを読めなかったタイルはアイコンを出す", async () => {
    await open();
    tile(1)!.querySelector("img")!.dispatchEvent(new Event("error"));
    expect(tile(1)!.querySelector("img")).toBeNull();
    expect(tile(1)!.querySelector(".similar-thumb svg")).not.toBeNull();
  });
});

describe("選択の移動", () => {
  it("← → でグループ内を移動し、端では止まる", async () => {
    await open();
    expect(key("Left")).toBe(true);
    expect(selected()).toBe(1);
    key("Left");
    expect(selected()).toBe(1);
    key("Right");
    key("Right");
    key("Right");
    expect(selected()).toBe(4);
  });

  it("↑ ↓ で前後のグループの同じ位置（なければ末尾）へ移る", async () => {
    await open();
    key("Right"); // 3 枚目（番号 4）
    expect(key("Down")).toBe(true);
    expect(selected()).toBe(7);
    key("Down");
    expect(selected()).toBe(7);
    key("Left");
    key("Up");
    expect(selected()).toBe(1);
  });

  it("Home / End で最初と最後のグループへ移る", async () => {
    await open();
    key("End");
    expect(selected()).toBe(5);
    key("Home");
    expect(selected()).toBe(1);
  });

  it("タイルのクリックで選択する", async () => {
    await open();
    expect(v.handleSimilarAction("similar-select", tile(5)!)).toBe(true);
    expect(selected()).toBe(5);
    expect(v.handleSimilarAction("move", tile(5)!)).toBe(false);
  });
});

describe("削除・これだけ残す・取り消し", () => {
  it("削除キーで選択中の 1 枚を削除予定にし、グループを取り直して近い画像を選ぶ", async () => {
    await open();
    const view = makeSession({ pendingDeletions: 1 });
    api.deleteSimilar.mockResolvedValue({ view, message: null });
    api.similarGroups.mockResolvedValue(without(2));
    expect(key("Delete")).toBe(true);
    await vi.waitFor(() => expect(shown()[0]).toEqual([1, 4]));
    expect(api.deleteSimilar).toHaveBeenCalledWith(1, 2);
    expect(actions.setSession).toHaveBeenCalledWith(view);
    expect(toast).toHaveBeenCalledWith("削除予定にしました: a (1).jpg");
    expect(selected()).toBe(4);
  });

  it("グループが消えたら同じ位置のグループを選ぶ", async () => {
    await open();
    key("End");
    key("Right"); // 番号 7
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.similarGroups.mockResolvedValue(without(7));
    key("Delete");
    await vi.waitFor(() => expect(shown()).toEqual([[1, 2, 4]]));
    expect(selected()).toBe(2);
  });

  it("削除に失敗したらエラーを出し、グループを取り直す", async () => {
    await open();
    api.deleteSimilar.mockRejectedValue("削除フォルダが見つかりません");
    key("Delete");
    await vi.waitFor(() => expect(toast).toHaveBeenCalledWith("削除フォルダが見つかりません", "error"));
    expect(api.similarGroups).toHaveBeenCalledTimes(2);
    expect(selected()).toBe(2);
  });

  it("keep_only のキーで、選択中を残し、同じグループの残りを削除予定にする", async () => {
    await open();
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 2, error: null });
    api.similarGroups.mockResolvedValue(without(1, 4));
    expect(key("Ctrl+Shift+K")).toBe(true);
    await vi.waitFor(() => expect(shown()).toEqual([[5, 7]]));
    expect(api.keepOnly).toHaveBeenCalledWith(1, 2, [1, 4]);
    expect(toast).toHaveBeenCalledWith("2 枚を削除予定にしました（取り消しで 1 枚ずつ戻せます）");
    // 消えたグループと同じ位置のグループの、同じ位置（2 枚目）を選ぶ
    expect(selected()).toBe(7);
  });

  it("これだけ残すが途中で失敗したら、件数と理由をエラーで出す", async () => {
    await open();
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 1, error: "移動できませんでした" });
    api.similarGroups.mockResolvedValue(without(1));
    key("Ctrl+Shift+K");
    await vi.waitFor(() =>
      expect(toast).toHaveBeenCalledWith("1 枚を削除予定にしましたが、途中で失敗しました: 移動できませんでした", "error"),
    );
    expect(shown()[0]).toEqual([2, 4]);
  });

  it("キーリピートと処理待ちの間は、削除・これだけ残す・取り消しを重ねて呼ばない", async () => {
    await open();
    expect(key("Ctrl+Shift+K", true)).toBe(true);
    expect(key("Delete", true)).toBe(true);
    expect(key("Ctrl+Z", true)).toBe(true);
    actions.busy.mockReturnValue(true);
    expect(key("Ctrl+Shift+K")).toBe(true);
    expect(key("Delete")).toBe(true);
    expect(key("Ctrl+Z")).toBe(true);
    expect(api.keepOnly).not.toHaveBeenCalled();
    expect(api.deleteSimilar).not.toHaveBeenCalled();
    expect(actions.undo).not.toHaveBeenCalled();
  });

  it("これだけ残すを続けて 2 回押しても、2 回目は何も削除しない", async () => {
    await open();
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 2, error: null });
    api.similarGroups.mockResolvedValue(without(1, 4));
    key("Ctrl+Shift+K");
    expect(key("Ctrl+Shift+K")).toBe(true);
    await vi.waitFor(() => expect(shown()).toEqual([[5, 7]]));
    await actions.enqueue(async () => {});
    // 1 回目のあとに選び直された別のグループの画像（7）には何もしない
    expect(api.keepOnly.mock.calls).toEqual([[1, 2, [1, 4]]]);
  });

  it("削除キーを続けて 2 回押しても、選び直された隣の画像は削除しない", async () => {
    await open();
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.similarGroups.mockResolvedValue(without(2));
    key("Delete");
    expect(key("Delete")).toBe(true);
    await vi.waitFor(() => expect(shown()[0]).toEqual([1, 4]));
    await actions.enqueue(async () => {});
    expect(api.deleteSimilar.mock.calls).toEqual([[1, 2]]);
    // 処理が終われば、次の 1 回は受け付ける
    api.similarGroups.mockResolvedValue(without(2, 4));
    key("Delete");
    await vi.waitFor(() => expect(api.deleteSimilar).toHaveBeenCalledTimes(2));
    expect(api.deleteSimilar.mock.calls[1]).toEqual([1, 4]);
  });

  it("タイルのボタンを続けて押しても、処理中は次の操作を受け付けない", async () => {
    await open();
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 2, error: null });
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.similarGroups.mockResolvedValue(without(1, 4));
    v.handleSimilarAction("similar-keep", tile(2)!.querySelector<HTMLElement>('[data-action="similar-keep"]')!);
    v.handleSimilarAction("similar-delete", tile(5)!.querySelector<HTMLElement>('[data-action="similar-delete"]')!);
    await vi.waitFor(() => expect(shown()).toEqual([[5, 7]]));
    await actions.enqueue(async () => {});
    expect(api.keepOnly).toHaveBeenCalledTimes(1);
    expect(api.deleteSimilar).not.toHaveBeenCalled();
  });

  it("押したあとにグループが増えても、押した時点で見えていた画像だけを削除予定にする", async () => {
    await open();
    // 閾値を動かした直後（取り直しが順番待ち）に「これだけ残す」を押す
    const wider = makeSimilarGroups();
    wider.groups[0].members.push({ ...wider.groups[0].members[2], index: 9, name: "later.jpg" });
    api.similarGroups.mockResolvedValue(wider);
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 2, error: null });
    thresholdInput().value = "12";
    v.handleSimilarInput(thresholdInput());
    key("Ctrl+Shift+K");
    await vi.waitFor(() => expect(api.keepOnly).toHaveBeenCalledTimes(1));
    expect(api.keepOnly).toHaveBeenCalledWith(1, 2, [1, 4]);
  });

  it("押したあとに対象の画像がグループから消えていたら、何もしない", async () => {
    await open();
    // 取り直しで、押した時点のグループの 1 枚（4）が消える
    api.similarGroups.mockResolvedValue(without(4));
    thresholdInput().value = "3";
    v.handleSimilarInput(thresholdInput());
    key("Ctrl+Shift+K");
    await vi.waitFor(() => expect(shown()[0]).toEqual([1, 2]));
    await actions.enqueue(async () => {});
    expect(api.keepOnly).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("画像の一覧が変わりました。もう一度選んでください");
  });

  it("取り消しキーで undo を呼び、グループを取り直す", async () => {
    await open();
    expect(key("Ctrl+Z")).toBe(true);
    await vi.waitFor(() => expect(api.similarGroups).toHaveBeenCalledTimes(2));
    expect(actions.undo).toHaveBeenCalledTimes(1);
  });

  it("タイルのボタンでも削除・これだけ残すができる", async () => {
    await open();
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 1, error: null });
    v.handleSimilarAction("similar-delete", tile(4)!.querySelector<HTMLElement>('[data-action="similar-delete"]')!);
    await vi.waitFor(() => expect(api.deleteSimilar).toHaveBeenCalledWith(1, 4));
    v.handleSimilarAction("similar-keep", tile(7)!.querySelector<HTMLElement>('[data-action="similar-keep"]')!);
    await vi.waitFor(() => expect(api.keepOnly).toHaveBeenCalledWith(1, 7, [5]));
  });
});

describe("スクロール位置", () => {
  const scroller = () => slot("similar-scroll")!;
  let scrolled: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    scrolled = vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(() => {});
  });
  afterEach(() => scrolled.mockRestore());

  it("判定中の取り直しで中身が同じなら、描き直さずスクロールもしない", async () => {
    vi.useFakeTimers();
    await open(makeSimilarGroups(), makeSimilarStatus({ done: 2, total: 8, running: true }));
    const before = tile(5);
    scroller().scrollTop = 400; // 利用者が下へスクロールして見ている
    scrolled.mockClear();

    api.similarStatus.mockResolvedValue(makeSimilarStatus({ done: 5, total: 8, running: true }));
    api.similarGroups.mockResolvedValue(makeSimilarGroups());
    await vi.advanceTimersByTimeAsync(300);
    expect(clean(slot("similar-progress"))).toBe("判定中 5 / 8");
    expect(tile(5)).toBe(before);
    expect(scroller().scrollTop).toBe(400);
    expect(scrolled).not.toHaveBeenCalled();
  });

  it("判定中にグループが増えても、見ている位置を保ち、選択中の画像へ戻さない", async () => {
    vi.useFakeTimers();
    await open(without(5, 7), makeSimilarStatus({ done: 2, total: 8, running: true }));
    scroller().scrollTop = 400;
    scrolled.mockClear();

    api.similarStatus.mockResolvedValue(makeSimilarStatus({ done: 6, total: 8, running: true }));
    api.similarGroups.mockResolvedValue(makeSimilarGroups());
    await vi.advanceTimersByTimeAsync(300);
    expect(shown()).toHaveLength(2);
    expect(scroller().scrollTop).toBe(400);
    expect(scrolled).not.toHaveBeenCalled();
    expect(selected()).toBe(2);
  });

  it("タイルの削除ボタンを押すと、その画像を選んでから削除する（先頭の選択へ戻らない）", async () => {
    await open();
    expect(selected()).toBe(2);
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.similarGroups.mockResolvedValue(without(7));
    scroller().scrollTop = 400;
    v.handleSimilarAction("similar-delete", tile(7)!.querySelector<HTMLElement>('[data-action="similar-delete"]')!);
    expect(selected()).toBe(7);
    await vi.waitFor(() => expect(shown()).toEqual([[1, 2, 4]]));
    expect(scroller().scrollTop).toBe(400);
  });

  it("タイルの「これだけ残す」ボタンも、その画像を選んでから処理する", async () => {
    await open();
    api.keepOnly.mockResolvedValue({ view: makeSession(), deleted: 1, error: null });
    api.similarGroups.mockResolvedValue(without(5));
    v.handleSimilarAction("similar-keep", tile(7)!.querySelector<HTMLElement>('[data-action="similar-keep"]')!);
    expect(selected()).toBe(7);
    await vi.waitFor(() => expect(api.keepOnly).toHaveBeenCalledWith(1, 7, [5]));
  });

  it("キーで選択を動かしたときと、削除で選択が移ったときは、選択中の画像を画面内に入れる", async () => {
    await open();
    scrolled.mockClear();
    key("Right");
    expect(scrolled).toHaveBeenCalledTimes(1);
    expect(scrolled.mock.contexts[0]).toBe(tile(4));

    scrolled.mockClear();
    api.deleteSimilar.mockResolvedValue({ view: makeSession(), message: null });
    api.similarGroups.mockResolvedValue(without(4));
    key("Delete");
    await vi.waitFor(() => expect(shown()[0]).toEqual([1, 2]));
    expect(scrolled).toHaveBeenCalledTimes(1);
    expect(scrolled.mock.contexts[0]).toBe(tile(2));
  });

  it("タイルをクリックして選んでも、スクロールは動かさない", async () => {
    await open();
    scrolled.mockClear();
    v.handleSimilarAction("similar-select", tile(5)!);
    expect(selected()).toBe(5);
    expect(scrolled).not.toHaveBeenCalled();
  });
});

describe("閾値・絞り込み", () => {
  it("スライダーで閾値を変えるとグループを取り直し、値を保存する", async () => {
    await open();
    expect(thresholdInput().value).toBe("6");
    thresholdInput().value = "12";
    v.handleSimilarInput(thresholdInput());
    await vi.waitFor(() => expect(api.similarGroups).toHaveBeenLastCalledWith(12));
    expect(clean(slot("similar-threshold"))).toBe("類似度 81% 以上");
    expect(localStorage.getItem("lumiwake.similarThreshold")).toBe("12");
  });

  it("保存した閾値を復元し、範囲外や数値でない値は丸める", async () => {
    localStorage.setItem("lumiwake.similarThreshold", "10");
    await load();
    await open();
    expect(api.similarGroups).toHaveBeenCalledWith(10);
    expect(thresholdInput().value).toBe("10");

    expect(v.clampThreshold("99")).toBe(16);
    expect(v.clampThreshold(-3)).toBe(0);
    expect(v.clampThreshold("abc")).toBe(6);
    expect(v.clampThreshold(null)).toBe(6);
    expect(v.clampThreshold("")).toBe(6);
    expect(v.clampThreshold(7.6)).toBe(8);
    expect([0, 6, 16].map(v.similarityPercent)).toEqual([100, 91, 75]);
  });

  it("絞り込みで該当するグループだけを出し、選択を見える画像に移す", async () => {
    await open();
    const choose = (value: string) => {
      filterSelect().value = value;
      v.handleSimilarInput(filterSelect());
    };
    choose("exact");
    expect(shown()).toEqual([[5, 7]]);
    expect(selected()).toBe(7);
    expect(clean(slot("similar-progress"))).toBe("1 / 2 グループ・2 枚");
    choose("names");
    expect(shown()).toEqual([[1, 2, 4]]);
    choose("resolution");
    expect(shown()).toEqual([[5, 7]]);
    choose("all");
    expect(shown()).toHaveLength(2);

    const groups = makeSimilarGroups();
    groups.groups.pop();
    v.closeSimilar();
    await open(groups);
    choose("exact");
    expect(clean(root.querySelector(".similar-empty"))).toBe("該当するグループがありません");
  });
});

describe("開く・閉じる", () => {
  it("Enter で選択中の画像を 1 枚表示で開いて閉じる", async () => {
    await open();
    key("Right");
    expect(key("Enter")).toBe(true);
    expect(store.screen).toBe("sort");
    expect(actions.jumpTo).toHaveBeenCalledWith(4);
  });

  it("ダブルクリックでも開く", async () => {
    await open();
    expect(v.handleSimilarDblClick(tile(5)!.querySelector("img")!)).toBe(true);
    expect(actions.jumpTo).toHaveBeenCalledWith(5);
    await open();
    expect(v.handleSimilarDblClick(tile(5)!.querySelector<HTMLElement>('[data-action="similar-delete"]')!)).toBe(false);
  });

  it("Esc と toggle_similar のキー、閉じるボタンで閉じる", async () => {
    await open();
    expect(key("Escape")).toBe(true);
    expect(store.screen).toBe("sort");
    await open();
    expect(key("Ctrl+Shift+D")).toBe(true);
    expect(store.screen).toBe("sort");
    await open();
    expect(v.handleSimilarAction("similar-close", root)).toBe(true);
    expect(store.screen).toBe("sort");
    expect(key("Escape")).toBe(false);
  });

  it("振り分けキー・スキップは受け付けない", async () => {
    await open();
    expect(key("1")).toBe(false);
    expect(key("Space")).toBe(false);
    expect(store.screen).toBe("similar");
    expect(api.deleteSimilar).not.toHaveBeenCalled();
  });
});
