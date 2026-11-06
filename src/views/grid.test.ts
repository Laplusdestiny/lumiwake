// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const actions = vi.hoisted(() => ({
  jumpTo: vi.fn(),
  perform: vi.fn(),
  enqueue: vi.fn((task: () => Promise<void>) => task()),
}));
const api = vi.hoisted(() => ({ listItems: vi.fn() }));
const toast = vi.hoisted(() => vi.fn());
vi.mock("../actions", () => actions);
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), api }));
vi.mock("../toast", () => ({ toast }));

import type { GridList } from "../api";
import { makeConfig, makeGridList, makeSession } from "../test/fixtures";

type Grid = typeof import("./grid");
type Store = typeof import("../store")["store"];

let root: HTMLElement;
let g: Grid;
let store: Store;

/** 3 列になる幅（タイル 160px、間隔 12px） */
const THREE_COLUMNS = 3 * 160 + 2 * 12;

/** 一覧はモジュール内に選択・絞り込み・大きさを持つので、テストごとに読み込み直す */
async function load(): Promise<void> {
  vi.resetModules();
  const storeMod = await import("../store");
  store = storeMod.store;
  g = await import("./grid");
  store.config = makeConfig();
  store.session = makeSession();
  store.screen = "sort";
  store.grid = null;
  storeMod.onChange(() => store.screen === "grid" && g.renderGrid(root));
}

async function open(list: GridList = makeGridList()): Promise<void> {
  api.listItems.mockResolvedValue(list);
  await g.openGrid();
  const tiles = root.querySelector<HTMLElement>('[data-slot="grid-tiles"]');
  if (tiles) Object.defineProperty(tiles, "clientWidth", { value: THREE_COLUMNS, configurable: true });
}

const selected = () => Number(root.querySelector<HTMLElement>(".grid-tile.is-selected")?.dataset.index ?? NaN);
const tile = (i: number) => root.querySelector<HTMLElement>(`.grid-tile[data-index="${i}"]`);
const tileText = (i: number) => tile(i)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
const shownIndexes = () => [...root.querySelectorAll<HTMLElement>(".grid-tile")].map((t) => Number(t.dataset.index));
const tileSize = () => document.documentElement.style.getPropertyValue("--grid-tile");
const filterInput = () => root.querySelector<HTMLInputElement>('[data-grid="filter"]')!;
const sizeInput = () => root.querySelector<HTMLInputElement>('[data-grid="size"]')!;

function typeFilter(text: string): void {
  filterInput().value = text;
  g.handleGridInput(filterInput());
}

beforeEach(async () => {
  vi.clearAllMocks();
  localStorage.clear();
  document.documentElement.style.removeProperty("--grid-tile");
  document.body.innerHTML = `<div id="app"></div>`;
  root = document.querySelector("#app")!;
  await load();
});

describe("開く・表示", () => {
  it("開くと現在の画像を選び、状態ごとのラベルを出す", async () => {
    await open();
    expect(store.screen).toBe("grid");
    expect(root.dataset.view).toBe("grid");
    expect(selected()).toBe(2);
    expect(tile(2)!.classList.contains("is-current")).toBe(true);
    expect(tileText(0)).toContain("→風景");
    expect(tileText(1)).toContain("削除予定");
    expect(tileText(3)).toContain("保留");
    expect(tile(0)!.classList.contains("state-moved")).toBe(true);
    expect(tile(2)!.querySelector("img")!.getAttribute("src")).toContain("grid/1/2");
    expect(tile(2)!.querySelector("img")!.getAttribute("loading")).toBe("lazy");
    expect(root.querySelector('[data-slot="grid-count"]')!.textContent).toBe("6 枚");
  });

  it("すべて処理済み（現在の画像なし）なら先頭を選ぶ", async () => {
    await open(makeGridList({ current: null }));
    expect(selected()).toBe(0);
  });

  it("同名ファイルの確認中は開かない", async () => {
    const file = { name: "c.jpg", path: "/c.jpg", size: 1, modified: null };
    store.session = makeSession({ conflict: { item: 2, label: "a", dir: "/a", incoming: file, existing: file } });
    await open();
    expect(api.listItems).not.toHaveBeenCalled();
    expect(store.screen).toBe("sort");
  });

  it("一覧の取得中に同名ファイルの確認が始まったら開かない", async () => {
    let resolve!: (l: GridList) => void;
    api.listItems.mockReturnValue(new Promise<GridList>((r) => (resolve = r)));
    const opening = g.openGrid();
    const file = { name: "c.jpg", path: "/c.jpg", size: 1, modified: null };
    store.session = makeSession({ conflict: { item: 2, label: "a", dir: "/a", incoming: file, existing: file } });
    resolve(makeGridList());
    await opening;
    expect(store.screen).toBe("sort");
    expect(store.grid).toBeNull();
  });

  it("一覧の取得中に設定画面へ移ったら、設定画面のまま", async () => {
    let resolve!: (l: GridList) => void;
    api.listItems.mockReturnValue(new Promise<GridList>((r) => (resolve = r)));
    const opening = g.openGrid();
    store.screen = "settings";
    resolve(makeGridList());
    await opening;
    expect(store.screen).toBe("settings");
    expect(store.grid).toBeNull();
  });

  it("一覧を開いている間に同名ファイルの確認が始まったら閉じる", async () => {
    await open();
    const file = { name: "c.jpg", path: "/c.jpg", size: 1, modified: null };
    store.session = makeSession({ conflict: { item: 2, label: "a", dir: "/a", incoming: file, existing: file } });
    g.renderGrid(root);
    expect(store.screen).toBe("sort");
  });

  it("一覧を取得できなければ知らせて仕分け画面のまま", async () => {
    api.listItems.mockRejectedValue("仕分け元フォルダが読み込まれていません");
    await g.openGrid();
    expect(store.screen).toBe("sort");
    expect(toast).toHaveBeenCalledWith("仕分け元フォルダが読み込まれていません", "error");
  });

  it("generation が変わったら閉じる", async () => {
    await open();
    store.session = makeSession({ generation: 2 });
    g.renderGrid(root);
    expect(store.screen).toBe("sort");
    expect(store.grid).toBeNull();
  });

  it("サムネイルを読めなければアイコンに置き換える", async () => {
    await open();
    tile(4)!.querySelector("img")!.dispatchEvent(new Event("error"));
    expect(tile(4)!.querySelector("img")).toBeNull();
    expect(tile(4)!.querySelector("svg")).not.toBeNull();
  });
});

describe("キー操作", () => {
  it("矢印・Home/End で選択が動き、端で止まる（3 列）", async () => {
    await open();
    const moves: [string, number][] = [
      ["Right", 3],
      ["Down", 5],
      ["Down", 5],
      ["Home", 0],
      ["Up", 0],
      ["End", 5],
      ["Left", 4],
      ["Up", 1],
    ];
    for (const [combo, want] of moves) {
      expect(g.handleGridKey(combo)).toBe(true);
      expect(selected(), combo).toBe(want);
    }
  });

  it("PageUp / PageDown は 1 画面分動く", async () => {
    await open();
    const scroll = root.querySelector<HTMLElement>('[data-slot="grid-scroll"]')!;
    Object.defineProperty(scroll, "clientHeight", { value: 200 }); // 1 行分（160 + 40）
    g.handleGridKey("Home");
    g.handleGridKey("PageDown");
    expect(selected()).toBe(3);
    g.handleGridKey("PageUp");
    expect(selected()).toBe(0);
  });

  it("Enter で未処理の画像を開いて仕分け画面に戻る", async () => {
    await open();
    g.handleGridKey("Right");
    expect(g.handleGridKey("Enter")).toBe(true);
    expect(actions.jumpTo).toHaveBeenCalledWith(3);
    expect(store.screen).toBe("sort");
  });

  it("処理済みの画像は開かずに知らせる", async () => {
    await open();
    g.handleGridKey("Home");
    g.handleGridKey("Enter");
    expect(actions.jumpTo).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith("この画像はすでに振り分け済みです（取り消しで戻せます）");
    expect(store.screen).toBe("grid");
  });

  it("Esc と toggle_grid のキーで閉じ、振り分けキーは受け付けない", async () => {
    await open();
    expect(g.handleGridKey("1")).toBe(false);
    expect(g.handleGridKey("Space")).toBe(false);
    expect(actions.perform).not.toHaveBeenCalled();
    expect(g.handleGridKey("Escape")).toBe(true);
    expect(store.screen).toBe("sort");
    await open();
    expect(g.handleGridKey("Ctrl+Shift+L")).toBe(true);
    expect(store.screen).toBe("sort");
  });

  it("Ctrl+F で絞り込み欄に移り、欄で Enter / ↓ を押すと一覧に戻る", async () => {
    await open();
    expect(g.handleGridKey("Ctrl+F")).toBe(true);
    expect(document.activeElement).toBe(filterInput());
    expect(g.handleGridFilterKey("Enter")).toBe(true);
    expect(document.activeElement).toBe(root.querySelector('[data-slot="grid-scroll"]'));
    filterInput().focus();
    expect(g.handleGridFilterKey("Down")).toBe(true);
    expect(document.activeElement).toBe(root.querySelector('[data-slot="grid-scroll"]'));
    expect(g.handleGridFilterKey("Left")).toBe(false);
  });

  it("クリックで開く・閉じるボタンで戻る", async () => {
    await open();
    expect(g.handleGridAction("grid-open", tile(4)!)).toBe(true);
    expect(actions.jumpTo).toHaveBeenCalledWith(4);
    await open();
    expect(g.handleGridAction("grid-close", document.createElement("button"))).toBe(true);
    expect(store.screen).toBe("sort");
    expect(g.handleGridAction("other", document.createElement("button"))).toBe(false);
  });
});

describe("絞り込み", () => {
  it("ファイル名の部分一致（大文字小文字を区別しない）で絞り込み、選択が隠れたら先頭を選ぶ", async () => {
    await open();
    typeFilter("CAT");
    expect(shownIndexes()).toEqual([3, 5]);
    expect(root.querySelector('[data-slot="grid-count"]')!.textContent).toBe("2 / 6 枚");
    expect(selected()).toBe(3);
    g.handleGridKey("Right");
    expect(selected()).toBe(5);
    g.handleGridKey("Enter");
    expect(actions.jumpTo).toHaveBeenCalledWith(5);
  });

  it("選択が残っていれば選択を変えない", async () => {
    await open();
    typeFilter(".jpg");
    expect(selected()).toBe(2);
  });

  it("0 件なら案内を出し、矢印と Enter では何もしない", async () => {
    await open();
    typeFilter("zzz");
    expect(root.querySelector('[data-slot="grid-tiles"]')!.textContent).toContain("該当する画像がありません");
    expect(g.handleGridKey("Right")).toBe(true);
    expect(g.handleGridKey("Enter")).toBe(true);
    expect(actions.jumpTo).not.toHaveBeenCalled();
  });

  it("画像が 0 枚でも開ける", async () => {
    await open(makeGridList({ current: null, items: [] }));
    expect(root.querySelector('[data-slot="grid-tiles"]')!.textContent).toContain("該当する画像がありません");
    expect(g.handleGridKey("End")).toBe(true);
    expect(g.handleGridKey("Enter")).toBe(true);
    expect(actions.jumpTo).not.toHaveBeenCalled();
  });

  it("開き直すと絞り込みを解除する", async () => {
    await open();
    typeFilter("cat");
    g.closeGrid();
    await open();
    expect(filterInput().value).toBe("");
    expect(shownIndexes()).toHaveLength(6);
  });
});

describe("サムネイルの大きさ", () => {
  it("既定は 160px で、キーで段階的に変えて保存する", async () => {
    await open();
    expect(tileSize()).toBe("160px");
    expect(sizeInput().value).toBe("160");
    const steps: [string, string][] = [
      ["Ctrl+Equal", "240px"],
      ["Ctrl+Shift+Semicolon", "360px"],
      ["Ctrl+NumAdd", "360px"],
      ["Ctrl+Minus", "240px"],
      ["Ctrl+NumSubtract", "160px"],
      ["Ctrl+Minus", "96px"],
      ["Ctrl+Minus", "96px"],
      ["Ctrl+Semicolon", "160px"],
      ["Ctrl+Shift+Equal", "240px"],
      ["Ctrl+0", "160px"],
    ];
    for (const [combo, want] of steps) {
      expect(g.handleGridKey(combo)).toBe(true);
      expect(tileSize(), combo).toBe(want);
    }
    g.handleGridKey("Ctrl+Equal");
    expect(localStorage.getItem("lumiwake.gridTile")).toBe("240");
    expect(sizeInput().value).toBe("240");
  });

  it("スライダーで自由に変えられる", async () => {
    await open();
    sizeInput().value = "200";
    g.handleGridInput(sizeInput());
    expect(tileSize()).toBe("200px");
    expect(localStorage.getItem("lumiwake.gridTile")).toBe("200");
  });

  it("保存した大きさを次に開いたときに使う", async () => {
    localStorage.setItem("lumiwake.gridTile", "300");
    await load();
    await open();
    expect(tileSize()).toBe("300px");
  });

  it("壊れた保存値は既定値か範囲内に丸める", async () => {
    localStorage.setItem("lumiwake.gridTile", "abc");
    await load();
    await open();
    expect(tileSize()).toBe("160px");
  });

  it("大きさに合わせて列数が変わる", async () => {
    await open();
    g.handleGridKey("Home");
    g.handleGridKey("Ctrl+Minus"); // 96px なら 504px に 4 列
    g.handleGridKey("Down");
    expect(selected()).toBe(4);
  });

  it("Ctrl+ホイールで 1 段階ずつ変え、Ctrl なしのホイールは扱わない", async () => {
    await open();
    expect(g.handleGridWheel({ deltaY: -100, ctrlKey: true })).toBe(true);
    expect(tileSize()).toBe("240px");
    expect(g.handleGridWheel({ deltaY: 100, ctrlKey: true })).toBe(true);
    expect(tileSize()).toBe("160px");
    expect(g.handleGridWheel({ deltaY: 100, ctrlKey: false })).toBe(false);
    expect(tileSize()).toBe("160px");
  });
});

describe("clampTile / stepTile / gridColumns", () => {
  it("壊れた保存値を丸める", () => {
    expect(g.clampTile("abc")).toBe(160);
    expect(g.clampTile(null)).toBe(160);
    expect(g.clampTile("")).toBe(160);
    expect(g.clampTile("9999")).toBe(360);
    expect(g.clampTile(10)).toBe(96);
    expect(g.clampTile("200")).toBe(200);
  });

  it("段階の途中からは次の段階へ", () => {
    expect(g.stepTile(200, 1)).toBe(240);
    expect(g.stepTile(200, -1)).toBe(160);
    expect(g.stepTile(96, -1)).toBe(96);
    expect(g.stepTile(360, 1)).toBe(360);
  });

  it("列数は 1 以上", () => {
    expect(g.gridColumns(0, 160, 12)).toBe(1);
    expect(g.gridColumns(504, 160, 12)).toBe(3);
    expect(g.gridColumns(503, 160, 12)).toBe(2);
  });
});
