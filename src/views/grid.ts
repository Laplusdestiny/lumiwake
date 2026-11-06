// 一覧画面。読み込んだ画像をサムネイルで並べ、選んだ画像を 1 枚表示で開く（ファイル操作はしない）
import { api, errorText, lumiUrl, type GridItem, type GridList } from "../api";
import * as actions from "../actions";
import { displayCombo } from "../keys";
import { notify, store } from "../store";
import { toast } from "../toast";
import { esc, formatCount, icons } from "../util";

/** タイルの幅（px）の段階。キーとホイールではこの順に変える */
export const TILE_STEPS = [96, 160, 240, 360] as const;
const TILE_MIN = TILE_STEPS[0];
const TILE_MAX = TILE_STEPS[TILE_STEPS.length - 1];
const TILE_DEFAULT = 160;
/** タイルの間隔（px）。styles.css の .grid-tiles の gap とそろえる */
export const GRID_GAP = 12;
/** タイルのファイル名・ラベルの分の高さの目安（1 画面の行数を求める） */
const LABEL_HEIGHT = 40;
const TILE_KEY = "lumiwake.gridTile";

const ALREADY_SORTED = "この画像はすでに振り分け済みです（取り消しで戻せます）";

const ZOOM_IN = new Set(["Ctrl+Equal", "Ctrl+Shift+Equal", "Ctrl+Semicolon", "Ctrl+Shift+Semicolon", "Ctrl+NumAdd"]);
const ZOOM_OUT = new Set(["Ctrl+Minus", "Ctrl+NumSubtract"]);
const ZOOM_RESET = new Set(["Ctrl+0", "Ctrl+Num0"]);

/** 保存値などをタイルの幅にする。数値でなければ既定値、範囲外は端に丸める */
export function clampTile(v: unknown): number {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  if (!Number.isFinite(n)) return TILE_DEFAULT;
  return Math.round(Math.min(TILE_MAX, Math.max(TILE_MIN, n)));
}

/** 今の幅より 1 段階大きい（小さい）段階。端では端のまま */
export function stepTile(size: number, dir: 1 | -1): number {
  if (dir > 0) return TILE_STEPS.find((s) => s > size) ?? TILE_MAX;
  return [...TILE_STEPS].reverse().find((s) => s < size) ?? TILE_MIN;
}

/** 幅 `width` に `tile` 幅のタイルが何列並ぶか（CSS の auto-fill と同じ計算） */
export function gridColumns(width: number, tile: number, gap: number): number {
  return Math.max(1, Math.floor((width + gap) / (tile + gap)));
}

function loadTile(): number {
  try {
    return clampTile(localStorage.getItem(TILE_KEY));
  } catch {
    return TILE_DEFAULT;
  }
}

function saveTile(size: number): void {
  try {
    localStorage.setItem(TILE_KEY, String(size));
  } catch {
    // 保存できなくても今回の表示には影響しない
  }
}

let filter = "";
/** 選択中の画像の番号 */
let selected: number | null = null;
let tile = loadTile();
let rootEl: HTMLElement | null = null;
/** タイルを作ったときの一覧と絞り込み。変わったときだけ作り直す */
let built: { list: object; filter: string } | null = null;

/** 絞り込み結果。一覧を開いているときだけ呼ぶ */
function visible(): GridItem[] {
  const items = store.grid!.items;
  const q = filter.trim().toLowerCase();
  return q ? items.filter((i) => i.name.toLowerCase().includes(q)) : items;
}

/** 選択が絞り込みで隠れたら、絞り込み結果の先頭を選ぶ */
function keepSelectionVisible(): void {
  const items = visible();
  if (!items.some((i) => i.index === selected)) selected = items[0]?.index ?? null;
}

/** 同名ファイルの確認中など、一覧を開けない状態か */
function cannotOpen(): boolean {
  return !store.session || !!store.session.conflict || store.screen !== "sort";
}

/**
 * 一覧を開く。先に押した振り分けの結果を見てから開くよう、操作の列に並べる
 * （振り分けが同名ファイルの確認になったら、確認を優先して開かない）
 */
export function openGrid(): Promise<void> {
  if (cannotOpen()) return Promise.resolve();
  return actions.enqueue(async () => {
    if (cannotOpen()) return;
    let list: GridList;
    try {
      list = await api.listItems();
    } catch (e) {
      toast(errorText(e), "error");
      return;
    }
    // 取得中に設定画面へ移ったり、同名ファイルの確認が始まったりしたら開かない
    if (cannotOpen()) return;
    store.grid = list;
    filter = "";
    built = null;
    selected = list.current ?? list.items[0]?.index ?? null;
    store.screen = "grid";
    notify();
    focusList();
  });
}

export function closeGrid(): void {
  store.grid = null;
  built = null;
  store.screen = "sort";
  notify();
}

// ---- 描画 ----

function skeleton(): string {
  const k = store.config!.config.keys;
  return `
  <div class="grid-view">
    <header class="topbar grid-bar">
      <div class="brand">一覧</div>
      <input class="grid-filter" type="search" data-grid="filter" placeholder="ファイル名で絞り込み（Ctrl+F）" aria-label="ファイル名で絞り込み" spellcheck="false">
      <span class="grid-count" data-slot="grid-count"></span>
      <div class="grow"></div>
      <label class="grid-size" title="サムネイルの大きさ（Ctrl+= で大きく、Ctrl+- で小さく、Ctrl+0 で元に戻す。Ctrl+ホイールでも変えられます）">
        <span class="muted small">大きさ</span>
        <input type="range" data-grid="size" min="${TILE_MIN}" max="${TILE_MAX}" step="8" aria-label="サムネイルの大きさ">
      </label>
      <button class="icon-btn" data-action="grid-close" aria-label="元の画面に戻る" title="元の画面に戻る（Esc / ${esc(displayCombo(k.toggle_grid))}）">${icons.close}</button>
    </header>
    <div class="grid-scroll" data-slot="grid-scroll" tabindex="-1">
      <div class="grid-tiles" data-slot="grid-tiles" role="listbox" aria-label="読み込んだ画像"></div>
    </div>
  </div>`;
}

function tileLabel(item: GridItem): string {
  switch (item.state) {
    case "moved":
      return `→${item.label ?? ""}`;
    case "deleted":
      return "削除予定";
    case "skipped":
      return "保留";
    default:
      return "";
  }
}

function tileHtml(item: GridItem, generation: number, current: number | null): string {
  const label = tileLabel(item);
  const cls = `grid-tile state-${item.state}${item.index === current ? " is-current" : ""}`;
  return `<button class="${cls}" data-action="grid-open" data-index="${item.index}" role="option" aria-selected="false" title="${esc(item.name)}">
      <span class="grid-thumb"><img src="${lumiUrl(`grid/${generation}/${item.index}`)}" alt="" loading="lazy" decoding="async"></span>
      <span class="grid-name">${esc(item.name)}</span>
      ${label ? `<span class="grid-label">${esc(label)}</span>` : ""}
    </button>`;
}

function buildTiles(root: HTMLElement): void {
  const list = store.grid!;
  const items = visible();
  const tiles = root.querySelector<HTMLElement>('[data-slot="grid-tiles"]')!;
  tiles.innerHTML = items.length
    ? items.map((i) => tileHtml(i, list.generation, list.current)).join("")
    : `<div class="grid-empty">該当する画像がありません</div>`;
  tiles.querySelectorAll("img").forEach((img) =>
    img.addEventListener("error", () => (img.parentElement!.innerHTML = icons.image), { once: true }),
  );
  const total = list.items.length;
  root.querySelector('[data-slot="grid-count"]')!.textContent = filter.trim()
    ? `${formatCount(items.length)} / ${formatCount(total)} 枚`
    : `${formatCount(total)} 枚`;
  built = { list, filter };
}

/** 選択中のタイルに印を付け、見える位置までスクロールする */
function updateSelection(): void {
  part("grid-tiles")
    .querySelectorAll<HTMLElement>(".grid-tile")
    .forEach((el) => {
      const on = Number(el.dataset.index) === selected;
      el.classList.toggle("is-selected", on);
      el.setAttribute("aria-selected", String(on));
      if (on) el.scrollIntoView?.({ block: "nearest" });
    });
}

export function renderGrid(root: HTMLElement): void {
  const list = store.grid;
  const s = store.session;
  // 設定で並び順を変えたときなど、画像の番号が変わったら古い一覧は使えない。
  // 同名ファイルの確認が始まったら、確認ダイアログを出せるよう仕分け画面へ戻る
  if (!list || !s || list.generation !== s.generation || s.conflict) return closeGrid();
  rootEl = root;
  if (root.dataset.view !== "grid") {
    root.innerHTML = skeleton();
    root.dataset.view = "grid";
    built = null;
  }
  const input = root.querySelector<HTMLInputElement>('[data-grid="filter"]')!;
  if (input.value !== filter) input.value = filter;
  document.documentElement.style.setProperty("--grid-tile", `${tile}px`);
  root.querySelector<HTMLInputElement>('[data-grid="size"]')!.value = String(tile);
  if (built?.list !== list || built.filter !== filter) buildTiles(root);
  updateSelection();
}

// ---- 操作 ----

/** 一覧の部品。一覧を開くと notify で必ず描画されるので、操作するときには存在する */
function part(slot: "grid-tiles" | "grid-scroll"): HTMLElement {
  return rootEl!.querySelector<HTMLElement>(`[data-slot="${slot}"]`)!;
}

function focusList(): void {
  part("grid-scroll").focus();
}

/** 大きさを変えて保存する。描き直すと選択中の画像が見える位置までスクロールする */
function setTile(size: number): void {
  tile = clampTile(size);
  saveTile(tile);
  notify();
}

function columns(): number {
  return gridColumns(part("grid-tiles").clientWidth, tile, GRID_GAP);
}

function pageRows(): number {
  return Math.max(1, Math.floor(part("grid-scroll").clientHeight / (tile + LABEL_HEIGHT)));
}

function openItem(index: number | null): void {
  const item = index == null ? undefined : visible().find((i) => i.index === index);
  if (!item) return;
  if (item.state === "moved" || item.state === "deleted") {
    toast(ALREADY_SORTED);
    return;
  }
  closeGrid();
  void actions.jumpTo(item.index);
}

function moveSelection(combo: string): boolean {
  const items = visible();
  const last = items.length - 1;
  const pos = Math.max(0, items.findIndex((i) => i.index === selected));
  const cols = columns();
  const target: Record<string, number> = {
    Left: pos - 1,
    Right: pos + 1,
    Up: pos - cols,
    Down: pos + cols,
    PageUp: pos - cols * pageRows(),
    PageDown: pos + cols * pageRows(),
    Home: 0,
    End: last,
  };
  if (!(combo in target)) return false;
  if (items.length === 0) return true;
  selected = items[Math.min(last, Math.max(0, target[combo]))].index;
  updateSelection();
  return true;
}

/** 一覧のキー操作。処理したら true。振り分け・削除・スキップのキーは受け付けない */
export function handleGridKey(combo: string): boolean {
  if (!store.grid) return false;
  if (combo === "Escape" || combo === store.config!.config.keys.toggle_grid) return closeGrid(), true;
  if (combo === "Enter") return openItem(selected), true;
  if (combo === "Ctrl+F") {
    rootEl!.querySelector<HTMLInputElement>('[data-grid="filter"]')!.focus();
    return true;
  }
  if (ZOOM_IN.has(combo)) return setTile(stepTile(tile, 1)), true;
  if (ZOOM_OUT.has(combo)) return setTile(stepTile(tile, -1)), true;
  if (ZOOM_RESET.has(combo)) return setTile(TILE_DEFAULT), true;
  return moveSelection(combo);
}

/** 絞り込み欄でのキー操作。Enter / ↓ で一覧に戻る */
export function handleGridFilterKey(combo: string): boolean {
  if (combo !== "Enter" && combo !== "Down") return false;
  keepSelectionVisible();
  updateSelection();
  focusList();
  return true;
}

export function handleGridAction(action: string, el: HTMLElement): boolean {
  switch (action) {
    case "grid-open":
      openItem(Number(el.dataset.index));
      return true;
    case "grid-close":
      closeGrid();
      return true;
    default:
      return false;
  }
}

export function handleGridInput(el: HTMLInputElement): void {
  if (el.dataset.grid === "filter") {
    filter = el.value;
    keepSelectionVisible();
    notify();
  } else if (el.dataset.grid === "size") {
    setTile(Number(el.value));
  }
}

/** Ctrl+ホイールで大きさを 1 段階ずつ変える。処理したら true */
export function handleGridWheel(e: Pick<WheelEvent, "ctrlKey" | "deltaY">): boolean {
  if (!e.ctrlKey || e.deltaY === 0) return false;
  setTile(stepTile(tile, e.deltaY < 0 ? 1 : -1));
  return true;
}
