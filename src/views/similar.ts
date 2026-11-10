// 類似画像の確認画面。中身の似た画像をグループで並べ、不要なものを削除予定にする。
// 判定とファイル操作は Rust 側。ここでは自動で選んだり削除したりしない
import { api, errorText, lumiUrl, type SimilarGroup, type SimilarMember } from "../api";
import * as actions from "../actions";
import { displayCombo } from "../keys";
import { notify, store } from "../store";
import { toast } from "../toast";
import { esc, formatBytes, formatCount, icons } from "../util";
import { NO_ZOOM, clampPan, compareWindow, stepZoom, zoomTransform, type Zoom } from "./similarCompare";

/** 閾値はハッシュの違うビット数（0〜64）で持つ。小さいほど厳しい */
export const THRESHOLD_DEFAULT = 6;
export const THRESHOLD_MAX = 16;
const THRESHOLD_KEY = "lumiwake.similarThreshold";
/** 判定中に進み具合を見に行く間隔（ms） */
const POLL_MS = 300;

type Filter = "all" | "exact" | "names" | "resolution";

const FILTERS: { value: Filter; label: string; test: (g: SimilarGroup) => boolean }[] = [
  { value: "all", label: "すべて", test: () => true },
  { value: "exact", label: "完全一致", test: (g) => g.exact },
  { value: "names", label: "名前が似ている", test: (g) => g.similarNames },
  { value: "resolution", label: "解像度が同じ", test: (g) => g.sameResolution },
];

/** 保存値などを閾値にする。数値でなければ既定値、範囲外は端に丸める */
export function clampThreshold(v: unknown): number {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  if (!Number.isFinite(n)) return THRESHOLD_DEFAULT;
  return Math.round(Math.min(THRESHOLD_MAX, Math.max(0, n)));
}

/** 閾値（違うビット数）を類似度（%）にする。Rust 側の similarity_percent と同じ計算 */
export function similarityPercent(distance: number): number {
  return Math.round((1 - distance / 64) * 100);
}

function loadThreshold(): number {
  try {
    return clampThreshold(localStorage.getItem(THRESHOLD_KEY));
  } catch {
    return THRESHOLD_DEFAULT;
  }
}

function saveThreshold(value: number): void {
  try {
    localStorage.setItem(THRESHOLD_KEY, String(value));
  } catch {
    // 保存できなくても今回の表示には影響しない
  }
}

let threshold = loadThreshold();
let filter: Filter = "all";
/** 選択中の画像の番号 */
let selected: number | null = null;
let rootEl: HTMLElement | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;
/** 次の描画で、選択中の画像を画面内に入れる（キーでの移動や、削除で選択が移ったとき） */
let revealSelection = false;
/** 描いたときのグループと絞り込み。変わったときだけ作り直す */
let built: { groups: object; filter: Filter } | null = null;

/** 比較表示（選択中のグループを大きく並べる）。null なら一覧。side: 並べる / flip: 1 枚ずつ切り替える */
let compare: "side" | "flip" | null = null;
let zoom: Zoom = NO_ZOOM;
/** 比較表示に描いた内容。変わったときだけ作り直す */
let builtCompare: { key: string; members: number[] } | null = null;

const ZOOM_IN = new Set(["Ctrl+Equal", "Ctrl+Shift+Equal", "Ctrl+Semicolon", "Ctrl+Shift+Semicolon", "Ctrl+NumAdd"]);
const ZOOM_OUT = new Set(["Ctrl+Minus", "Ctrl+NumSubtract"]);
const ZOOM_RESET = new Set(["Ctrl+0", "Ctrl+Num0"]);

/** 絞り込み後のグループ。画面を開いているときだけ呼ぶ */
function visible(): SimilarGroup[] {
  const test = FILTERS.find((f) => f.value === filter)!.test;
  return store.similar!.groups.filter(test);
}

/** 画像が、表示中の何番目のグループの何枚目か */
function position(index: number | null): { group: number; member: number } | null {
  const groups = visible();
  for (let g = 0; g < groups.length; g++) {
    const m = groups[g].members.findIndex((x) => x.index === index);
    if (m >= 0) return { group: g, member: m };
  }
  return null;
}

/**
 * 選択中の画像が表示から消えていたら、`was`（消える前の位置）に近い画像を選ぶ:
 * 同じ位置のグループの同じ位置、なければその末尾
 */
function reselect(was: { group: number; member: number } | null): void {
  if (position(selected)) return;
  revealSelection = true;
  const groups = visible();
  if (groups.length === 0) {
    selected = null;
    return;
  }
  const g = groups[Math.min(was?.group ?? 0, groups.length - 1)];
  selected = g.members[Math.min(was?.member ?? 0, g.members.length - 1)].index;
}

/** 同名ファイルの確認中など、開けない状態か */
function cannotOpen(): boolean {
  return !store.session || !!store.session.conflict || store.screen !== "sort";
}

/**
 * 類似画像の確認画面を開き、判定を始める。先に押した振り分けの結果を見てから開くよう、操作の列に並べる
 */
export function openSimilar(): Promise<void> {
  if (cannotOpen()) return Promise.resolve();
  return actions.enqueue(async () => {
    if (cannotOpen()) return;
    try {
      const status = await api.startSimilarScan();
      const list = await api.similarGroups(threshold);
      // 取得中に設定画面へ移ったり、同名ファイルの確認が始まったりしたら開かない
      if (cannotOpen()) return;
      store.similar = { generation: list.generation, status, groups: list.groups };
    } catch (e) {
      toast(errorText(e), "error");
      return;
    }
    filter = "all";
    built = null;
    compare = null;
    selected = store.similar.groups.flatMap((g) => g.members).find((m) => m.current)?.index ?? null;
    reselect(null);
    revealSelection = true;
    store.screen = "similar";
    notify();
    focusList();
    if (store.similar.status.running) schedulePoll();
  });
}

export function closeSimilar(): void {
  if (timer) clearTimeout(timer);
  timer = null;
  store.similar = null;
  built = null;
  compare = null;
  builtCompare = null;
  store.screen = "sort";
  notify();
}

/** グループを取り直す。消えた画像を選んでいたら近い画像を選び直す */
async function refresh(): Promise<void> {
  if (!store.similar) return;
  const was = position(selected);
  try {
    const list = await api.similarGroups(threshold);
    if (!store.similar) return;
    store.similar.generation = list.generation;
    // 中身が同じなら元の配列のままにして、描き直さない（判定中は 0.3 秒ごとに取り直すため）
    if (JSON.stringify(list.groups) !== JSON.stringify(store.similar.groups)) store.similar.groups = list.groups;
  } catch (e) {
    toast(errorText(e), "error");
    return;
  }
  reselect(was);
  notify();
}

function schedulePoll(): void {
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => void poll(), POLL_MS);
}

/** 判定の進み具合を見て、進んでいればグループを取り直す */
async function poll(): Promise<void> {
  timer = null;
  const sim = store.similar;
  if (!sim) return;
  try {
    const status = await api.similarStatus();
    if (store.similar !== sim) return;
    const progressed = status.done !== sim.status.done || status.running !== sim.status.running;
    sim.status = status;
    if (progressed) await refresh();
    else notify();
  } catch (e) {
    toast(errorText(e), "error");
    return;
  }
  if (store.similar === sim && sim.status.running) schedulePoll();
}

// ---- 描画 ----

function skeleton(): string {
  const k = store.config!.config.keys;
  const options = FILTERS.map((f) => `<option value="${f.value}">${esc(f.label)}</option>`).join("");
  return `
  <div class="similar-view">
    <header class="topbar similar-bar">
      <div class="brand">類似画像</div>
      <span class="similar-progress" data-slot="similar-progress"></span>
      <span class="similar-failed" data-slot="similar-failed" title="壊れているなどで読めなかった画像は、判定の対象から外しています"></span>
      <div class="grow"></div>
      <label class="similar-threshold" title="右へ動かすほど、違いの大きい画像も同じグループにします">
        <span class="muted small" data-slot="similar-threshold"></span>
        <input type="range" data-similar="threshold" min="0" max="${THRESHOLD_MAX}" step="1" aria-label="類似とみなす範囲">
      </label>
      <select class="similar-filter" data-similar="filter" aria-label="根拠で絞り込み">${options}</select>
      <button class="icon-btn" data-action="similar-close" aria-label="元の画面に戻る" title="元の画面に戻る（Esc / ${esc(displayCombo(k.toggle_similar))}）">${icons.close}</button>
    </header>
    <div class="similar-hints hints">
      <span>← → ↑ ↓ 選択</span>
      <span>${esc(displayCombo(k.delete))} 削除</span>
      <span>${esc(displayCombo(k.keep_only))} これだけ残す</span>
      <span>${esc(displayCombo(k.undo))} 取り消し</span>
      <span>Space 拡大して比較</span>
      <span>Enter 1 枚表示で開く</span>
    </div>
    <div class="similar-scroll" data-slot="similar-scroll" tabindex="-1">
      <div class="similar-groups" data-slot="similar-groups"></div>
    </div>
    <div class="similar-compare" data-slot="similar-compare" tabindex="-1" hidden>
      <div class="compare-bar">
        <span class="compare-title" data-slot="compare-title"></span>
        <span class="muted small">← → 画像　↑ ↓ グループ　Tab 並べる／切り替える　ホイール 拡大（ドラッグで移動、Ctrl+0 で戻す）</span>
        <div class="grow"></div>
        <span class="compare-zoom" data-slot="compare-zoom"></span>
        <button class="btn btn-sm" data-action="similar-compare-mode" data-slot="compare-mode"></button>
        <button class="btn btn-sm" data-action="similar-compare-close" title="グループの一覧に戻る（Esc / Space）">一覧に戻る</button>
      </div>
      <div class="compare-panes" data-slot="compare-panes"></div>
    </div>
  </div>`;
}

function badgesHtml(g: SimilarGroup): string {
  const labels = [
    g.exact && "完全一致",
    g.sameSize && "サイズが同じ",
    g.sameResolution && "解像度が同じ",
    g.similarNames && "名前が似ている",
  ].filter(Boolean);
  return labels.map((l) => `<span class="similar-badge">${esc(l)}</span>`).join("");
}

function marksHtml(m: SimilarMember): string {
  return [m.largestResolution && "解像度が最大", m.largestSize && "サイズが最大", m.current && "現在の画像"]
    .filter(Boolean)
    .map((l) => `<span class="similar-mark">${esc(l)}</span>`)
    .join("");
}

function metaText(m: SimilarMember): string {
  return `${m.width}×${m.height}・${formatBytes(m.size)}・類似度 ${m.similarity}%`;
}

function tileHtml(m: SimilarMember, generation: number): string {
  const k = store.config!.config.keys;
  const marks = marksHtml(m);
  return `<div class="similar-tile" data-action="similar-select" data-index="${m.index}" role="option" aria-selected="false" title="${esc(m.name)}">
      <span class="similar-thumb"><img src="${lumiUrl(`grid/${generation}/${m.index}`)}" alt="" loading="lazy" decoding="async"></span>
      <span class="similar-name">${esc(m.name)}</span>
      <span class="similar-meta">${esc(metaText(m))}</span>
      <span class="similar-marks">${marks}</span>
      <span class="similar-actions">
        <button class="btn btn-sm" data-action="similar-delete" data-index="${m.index}" title="この画像を削除予定にする（${esc(displayCombo(k.delete))}）">削除</button>
        <button class="btn btn-sm" data-action="similar-keep" data-index="${m.index}" title="この画像を残し、グループの残りを削除予定にする（${esc(displayCombo(k.keep_only))}）">これだけ残す</button>
      </span>
    </div>`;
}

function emptyText(): string {
  const sim = store.similar!;
  if (sim.groups.length > 0) return "該当するグループがありません";
  return sim.status.running ? "判定中…" : "似た画像は見つかりませんでした";
}

function buildGroups(root: HTMLElement): void {
  const sim = store.similar!;
  const groups = visible();
  const el = root.querySelector<HTMLElement>('[data-slot="similar-groups"]')!;
  // 描き直しても、見ている位置を保つ
  const scroller = root.querySelector<HTMLElement>('[data-slot="similar-scroll"]')!;
  const top = scroller.scrollTop;
  el.innerHTML = groups.length
    ? groups
        .map(
          (g, i) => `<section class="similar-group" role="listbox" aria-label="グループ ${i + 1}">
          <header class="similar-group-head">
            <span class="similar-group-title">グループ ${i + 1}・${g.members.length} 枚</span>
            ${badgesHtml(g)}
            <div class="grow"></div>
            <button class="btn btn-sm" data-action="similar-compare" data-index="${g.members[0].index}" title="このグループの画像を大きく並べて見比べる（Space）">拡大して比較</button>
          </header>
          <div class="similar-tiles">${g.members.map((m) => tileHtml(m, sim.generation)).join("")}</div>
        </section>`,
        )
        .join("")
    : `<div class="similar-empty">${emptyText()}</div>`;
  el.querySelectorAll("img").forEach((img) =>
    img.addEventListener("error", () => (img.parentElement!.innerHTML = icons.image), { once: true }),
  );
  scroller.scrollTop = top;
  built = { groups: sim.groups, filter };
}

function progressText(): string {
  const sim = store.similar!;
  if (sim.status.running) return `判定中 ${formatCount(sim.status.done)} / ${formatCount(sim.status.total)}`;
  const groups = visible();
  const images = groups.reduce((n, g) => n + g.members.length, 0);
  const count = filter === "all" ? `${groups.length}` : `${groups.length} / ${sim.groups.length}`;
  return images > 0 ? `${count} グループ・${formatCount(images)} 枚` : `${count} グループ`;
}

/**
 * 選択中のタイルに印を付ける。スクロールするのは `revealSelection` のときだけ
 * （描き直すたびに選択中の画像へ戻すと、スクロールして見ている位置から引き戻してしまう）
 */
function updateSelection(): void {
  const reveal = revealSelection;
  revealSelection = false;
  rootEl!.querySelectorAll<HTMLElement>(".similar-tile").forEach((el) => {
    const on = Number(el.dataset.index) === selected;
    el.classList.toggle("is-selected", on);
    el.setAttribute("aria-selected", String(on));
    if (on && reveal) el.scrollIntoView?.({ block: "nearest" });
  });
}

export function renderSimilar(root: HTMLElement): void {
  const sim = store.similar;
  const s = store.session;
  // 設定で並び順を変えたときなど、画像の番号が変わったら古いグループは使えない。
  // 同名ファイルの確認が始まったら、確認ダイアログを出せるよう仕分け画面へ戻る
  if (!sim || !s || sim.generation !== s.generation || s.conflict) return closeSimilar();
  rootEl = root;
  if (root.dataset.view !== "similar") {
    root.innerHTML = skeleton();
    root.dataset.view = "similar";
    built = null;
    builtCompare = null;
    initDrag(root);
  }
  const slot = (name: string) => root.querySelector<HTMLElement>(`[data-slot="${name}"]`)!;
  root.querySelector<HTMLInputElement>('[data-similar="threshold"]')!.value = String(threshold);
  root.querySelector<HTMLSelectElement>('[data-similar="filter"]')!.value = filter;
  slot("similar-threshold").textContent = `類似度 ${similarityPercent(threshold)}% 以上`;
  slot("similar-failed").textContent = sim.status.failed > 0 ? `読めなかった画像 ${formatCount(sim.status.failed)} 枚` : "";
  if (built?.groups !== sim.groups || built.filter !== filter) buildGroups(root);
  slot("similar-progress").textContent = progressText();
  // 判定中かどうかで文言が変わる（グループがまだないとき）
  const empty = root.querySelector(".similar-empty");
  if (empty) empty.textContent = emptyText();
  renderCompare();
  updateSelection();
}

// ---- 比較表示 ----

/**
 * 比較表示を描く。並べる画像と表示の種類が変わったときだけ枠を作り直し、
 * 選択や拡大だけが変わったときは印と transform を書き換える（画像を読み直さない）
 */
function renderCompare(): void {
  const root = rootEl!;
  const slot = (name: string) => root.querySelector<HTMLElement>(`[data-slot="${name}"]`)!;
  const pos = compare ? position(selected) : null;
  // 見るグループがなくなったら一覧に戻る
  if (!pos) compare = null;
  const pane = slot("similar-compare");
  pane.hidden = !compare;
  slot("similar-scroll").hidden = !!compare;
  if (!compare || !pos) {
    builtCompare = null;
    return;
  }
  const sim = store.similar!;
  const groups = visible();
  const members = groups[pos.group].members;
  // 別のグループへ移ったら全体表示に戻す（同じグループの中では、同じ場所を見比べられるよう保つ）
  const ids = members.map((m) => m.index);
  if (builtCompare && !builtCompare.members.some((i) => ids.includes(i))) zoom = NO_ZOOM;

  const [start, end] = compare === "flip" ? [pos.member, pos.member + 1] : compareWindow(members.length, pos.member);
  const shown = members.slice(start, end);
  const key = `${compare}|${sim.generation}|${JSON.stringify(shown)}`;
  if (builtCompare?.key !== key) {
    const panes = slot("compare-panes");
    panes.innerHTML = shown
      .map(
        (m) => `<figure class="compare-pane" data-action="similar-select" data-index="${m.index}">
          <div class="compare-stage"><img src="${lumiUrl(`preview/${sim.generation}/${m.index}`)}" alt="" draggable="false" decoding="async"></div>
          <figcaption>
            <span class="similar-name">${esc(m.name)}</span>
            <span class="similar-meta">${esc(metaText(m))}</span>
            <span class="similar-marks">${marksHtml(m)}</span>
          </figcaption>
        </figure>`,
      )
      .join("");
    panes.querySelectorAll("img").forEach((img) =>
      img.addEventListener("error", () => (img.parentElement!.innerHTML = icons.image), { once: true }),
    );
  }
  builtCompare = { key, members: ids };
  pane.dataset.mode = compare;
  slot("compare-title").textContent =
    `グループ ${pos.group + 1} / ${groups.length}・${pos.member + 1} / ${members.length} 枚目`;
  slot("compare-mode").textContent = compare === "flip" ? "並べて表示" : "1 枚ずつ切り替える";
  slot("compare-mode").title = "並べて表示／1 枚ずつ切り替える（Tab）";
  pane.querySelectorAll<HTMLElement>(".compare-pane").forEach((el) => {
    el.classList.toggle("is-selected", Number(el.dataset.index) === selected);
  });
  applyZoom();
}

/** 並べた画像すべてに、同じ倍率と位置を当てる */
function applyZoom(): void {
  const root = rootEl!;
  const transform = zoomTransform(zoom);
  root.querySelectorAll<HTMLElement>(".compare-pane img").forEach((img) => (img.style.transform = transform));
  root.querySelector<HTMLElement>('[data-slot="compare-zoom"]')!.textContent = `${Math.round(zoom.scale * 100)}%`;
  root.querySelector<HTMLElement>('[data-slot="similar-compare"]')!.classList.toggle("is-zoomed", zoom.scale > 1);
}

function setZoom(scale: number): void {
  zoom = { scale, x: clampPan(zoom.x, scale), y: clampPan(zoom.y, scale) };
  applyZoom();
}

/** 拡大中の画像を動かす。dx / dy は枠の大きさに対する割合 */
export function panCompare(dx: number, dy: number): void {
  if (!compare) return;
  zoom = { scale: zoom.scale, x: clampPan(zoom.x + dx, zoom.scale), y: clampPan(zoom.y + dy, zoom.scale) };
  applyZoom();
}

function setCompare(mode: "side" | "flip" | null): void {
  if (mode && !position(selected)) return;
  if (!compare && mode) zoom = NO_ZOOM;
  compare = mode;
  // 一覧に戻ったときは、選択中の画像が見える位置にする
  if (!mode) revealSelection = true;
  renderCompare();
  updateSelection();
  rootEl?.querySelector<HTMLElement>(`[data-slot="${mode ? "similar-compare" : "similar-scroll"}"]`)?.focus({
    preventScroll: true,
  });
}

/** 比較表示でのホイール: 拡大・縮小する。処理したら true */
export function handleSimilarWheel(e: Pick<WheelEvent, "deltaY">): boolean {
  if (!compare || e.deltaY === 0) return false;
  setZoom(stepZoom(zoom.scale, e.deltaY < 0 ? 1 : -1));
  return true;
}

/** 比較表示でのドラッグ: 拡大中の画像を動かす */
function initDrag(root: HTMLElement): void {
  const area = root.querySelector<HTMLElement>('[data-slot="compare-panes"]')!;
  let last: { x: number; y: number } | null = null;
  area.addEventListener("pointerdown", (e) => {
    if (zoom.scale > 1) last = { x: e.clientX, y: e.clientY };
  });
  area.addEventListener("pointermove", (e) => {
    if (!last) return;
    const stage = area.querySelector<HTMLElement>(".compare-stage");
    const w = stage?.clientWidth || 1;
    const h = stage?.clientHeight || 1;
    panCompare((e.clientX - last.x) / w, (e.clientY - last.y) / h);
    last = { x: e.clientX, y: e.clientY };
  });
  const stop = () => (last = null);
  area.addEventListener("pointerup", stop);
  area.addEventListener("pointerleave", stop);
  area.addEventListener("pointercancel", stop);
}

// ---- 操作 ----

function focusList(): void {
  rootEl?.querySelector<HTMLElement>('[data-slot="similar-scroll"]')?.focus({ preventScroll: true });
}

function memberOf(index: number | null): { group: SimilarGroup; member: SimilarMember } | null {
  for (const group of store.similar?.groups ?? []) {
    const member = group.members.find((m) => m.index === index);
    if (member) return { group, member };
  }
  return null;
}

/** 削除・これだけ残すを処理している数。処理中は次の削除操作を受け付けない */
let pendingOps = 0;

const CHANGED = "画像の一覧が変わりました。もう一度選んでください";

/**
 * 削除の操作を 1 つ、操作の列に並べる。対象（`indexes`）は押した時点で決め、順番が来たときに
 * そのすべてがまだグループに出ていなければ何もしない（押したあとで選び直された画像や、
 * 押した時点では見えていなかった画像を削除しないため）。処理中に押された次の操作は捨てる
 */
function runOp(indexes: number[], op: (generation: number) => Promise<void>): void {
  const sim = store.similar;
  if (!sim || pendingOps > 0) return;
  const generation = sim.generation;
  pendingOps++;
  void actions
    .enqueue(async () => {
      if (store.similar !== sim || sim.generation !== generation) return;
      if (!indexes.every((i) => memberOf(i))) {
        toast(CHANGED);
        return;
      }
      try {
        await op(generation);
      } catch (e) {
        toast(errorText(e), "error");
      }
      await refresh();
    })
    .finally(() => {
      pendingOps--;
    });
}

/** 画像 1 枚を削除予定にする */
function deleteOne(index: number | null): void {
  const hit = memberOf(index);
  if (!hit) return;
  const { member } = hit;
  runOp([member.index], async (generation) => {
    const r = await api.deleteSimilar(generation, member.index);
    actions.setSession(r.view);
    toast(`削除予定にしました: ${member.name}`);
  });
}

/** 画像 1 枚を残し、いま表示している同じグループの残りを削除予定にする */
function keepOnly(index: number | null): void {
  const hit = memberOf(index);
  if (!hit) return;
  const keep = hit.member.index;
  const others = hit.group.members.map((m) => m.index).filter((i) => i !== keep);
  runOp([keep, ...others], async (generation) => {
    const r = await api.keepOnly(generation, keep, others);
    actions.setSession(r.view);
    if (r.error) toast(`${r.deleted} 枚を削除予定にしましたが、途中で失敗しました: ${r.error}`, "error");
    else toast(`${r.deleted} 枚を削除予定にしました（取り消しで 1 枚ずつ戻せます）`);
  });
}

function undo(): void {
  void actions.undo();
  void actions.enqueue(refresh);
}

function openItem(index: number | null): void {
  if (index == null || !memberOf(index)) return;
  closeSimilar();
  void actions.jumpTo(index);
}

function moveSelection(combo: string): boolean {
  if (!["Left", "Right", "Up", "Down", "Home", "End"].includes(combo)) return false;
  const groups = visible();
  if (groups.length === 0) return true;
  const pos = position(selected) ?? { group: 0, member: 0 };
  const lastGroup = groups.length - 1;
  let { group, member } = pos;
  if (combo === "Left") member--;
  if (combo === "Right") member++;
  if (combo === "Up") group--;
  if (combo === "Down") group++;
  if (combo === "Home") [group, member] = [0, 0];
  if (combo === "End") [group, member] = [lastGroup, 0];
  const g = groups[Math.min(lastGroup, Math.max(0, group))];
  selected = g.members[Math.min(g.members.length - 1, Math.max(0, member))].index;
  revealSelection = true;
  renderCompare();
  updateSelection();
  return true;
}

/** 類似画像の確認画面のキー操作。処理したら true。振り分けキー・スキップは受け付けない */
export function handleSimilarKey(combo: string, e: Pick<KeyboardEvent, "repeat">): boolean {
  if (!store.similar) return false;
  const k = store.config!.config.keys;
  if (combo === k.toggle_similar) return closeSimilar(), true;
  if (compare) {
    if (combo === "Escape" || combo === "Space") return setCompare(null), true;
    if (combo === "Tab") return setCompare(compare === "side" ? "flip" : "side"), true;
    if (ZOOM_IN.has(combo)) return setZoom(stepZoom(zoom.scale, 1)), true;
    if (ZOOM_OUT.has(combo)) return setZoom(stepZoom(zoom.scale, -1)), true;
    if (ZOOM_RESET.has(combo)) return ((zoom = NO_ZOOM), applyZoom()), true;
  } else {
    if (combo === "Escape") return closeSimilar(), true;
    if (combo === "Space") return setCompare("side"), true;
  }
  if (combo === k.delete || combo === k.keep_only || combo === k.undo) {
    // 押しっぱなしや処理待ちの間に、続けて削除しない
    if (e.repeat || actions.busy()) return true;
    if (combo === k.undo) undo();
    else if (combo === k.delete) deleteOne(selected);
    else keepOnly(selected);
    return true;
  }
  if (combo === "Enter") return openItem(selected), true;
  return moveSelection(combo);
}

/** クリックした画像を選ぶ（見えている画像なのでスクロールはしない） */
function select(index: number): void {
  if (!memberOf(index)) return;
  selected = index;
  renderCompare();
  updateSelection();
}

export function handleSimilarAction(action: string, el: HTMLElement): boolean {
  const index = Number(el.dataset.index);
  switch (action) {
    case "similar-close":
      closeSimilar();
      return true;
    case "similar-select":
      select(index);
      if (!compare) focusList();
      return true;
    case "similar-compare":
      // 選択中の画像がそのグループになければ、先頭の画像を選ぶ
      if (memberOf(index)?.group !== memberOf(selected)?.group) select(index);
      setCompare("side");
      return true;
    case "similar-compare-mode":
      if (compare) setCompare(compare === "side" ? "flip" : "side");
      return true;
    case "similar-compare-close":
      setCompare(null);
      return true;
    // ボタンを押した画像を選んでから処理する（処理のあと、その近くの画像が選ばれる）
    case "similar-delete":
      select(index);
      deleteOne(index);
      return true;
    case "similar-keep":
      select(index);
      keepOnly(index);
      return true;
    default:
      return false;
  }
}

/** タイルのダブルクリックで 1 枚表示を開く（タイル上のボタンは除く）。処理したら true */
export function handleSimilarDblClick(target: HTMLElement): boolean {
  const el = target.closest<HTMLElement>("[data-action]");
  if (!store.similar || el?.dataset.action !== "similar-select") return false;
  openItem(Number(el.dataset.index));
  return true;
}

export function handleSimilarInput(el: HTMLInputElement | HTMLSelectElement): void {
  if (el.dataset.similar === "threshold") {
    threshold = clampThreshold(el.value);
    saveThreshold(threshold);
    notify();
    void actions.enqueue(refresh);
  } else if (el.dataset.similar === "filter") {
    const was = position(selected);
    filter = FILTERS.some((f) => f.value === el.value) ? (el.value as Filter) : "all";
    reselect(was);
    notify();
  }
}
