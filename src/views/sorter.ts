// 仕分け画面（サイドバー型・全画面集中型）
import { api, errorText, lumiUrl, type Config, type SessionView } from "../api";
import * as actions from "../actions";
import { displayCombo } from "../keys";
import { notify, store } from "../store";
import { toast } from "../toast";
import {
  badgeText,
  barPercent,
  candidateLevels,
  noticeText,
  scoreLabel,
  scoreText,
  unevaluatedText,
} from "../suggest";
import { esc, formatBytes, formatCount, formatDate, icons, keycap, shortPath } from "../util";

type Mode = Config["general"]["view_mode"];

function sp(path: string): string {
  return shortPath(path, store.home);
}

// ---- 骨組み（表示モードが変わったときだけ作り直す） ----

function sidebarSkeleton(): string {
  return `
  <div class="sorter sidebar-mode">
    <header class="topbar">
      <div class="brand">Lumiwake</div>
      <button class="source-btn" data-action="change-source" title="仕分け元フォルダを変更">${icons.folder}<span data-slot="source"></span></button>
      <div class="grow"></div>
      <div data-slot="ai-badge"></div>
      <div class="segmented" role="group" aria-label="表示モード">
        <button data-action="mode" data-mode="sidebar" aria-pressed="true">サイドバー</button>
        <button data-action="mode" data-mode="focus" aria-pressed="false">全画面</button>
      </div>
      <div class="progress" data-slot="progress"></div>
      <button class="icon-btn" data-action="undo" aria-label="取り消し" title="取り消し">${icons.undo}</button>
      <button class="icon-btn" data-action="settings" aria-label="設定" title="設定（Ctrl+,）">${icons.settings}</button>
    </header>
    <div class="sorter-body">
      <main class="stage">
        <div class="meta" data-slot="meta"></div>
        <div class="preview" data-slot="preview"></div>
        <div class="suggest" data-slot="suggest"></div>
        <div class="filmstrip" data-slot="filmstrip"></div>
        <div class="hints" data-slot="hints"></div>
      </main>
      <aside class="targets-panel">
        <div class="resize-handle" data-resize title="ドラッグで幅を変更（ダブルクリックで元の幅に戻す）"></div>
        <div class="panel-head"><div class="panel-title">フォルダ</div><div data-slot="path-toggle"></div></div>
        <div class="target-list" data-slot="targets"></div>
      </aside>
    </div>
  </div>`;
}

function focusSkeleton(): string {
  return `
  <div class="sorter focus-mode">
    <div class="preview preview-full" data-slot="preview"></div>
    <div class="focus-top-left"><div class="meta" data-slot="meta"></div></div>
    <div class="focus-top-right">
      <div class="progress" data-slot="progress"></div>
      <button class="icon-btn glass" data-action="undo" aria-label="取り消し" title="取り消し">${icons.undo}</button>
      <button class="icon-btn glass" data-action="mode" data-mode="sidebar" aria-label="全画面を終了" title="全画面を終了（Esc）">${icons.exitFull}</button>
    </div>
    <div class="focus-bottom">
      <div class="suggest" data-slot="suggest"></div>
      <div class="focus-chips" data-slot="targets"></div>
      <div class="hints" data-slot="hints"></div>
    </div>
  </div>`;
}

// ---- 各部分 ----

function progressHtml(s: SessionView): string {
  const done = s.total - s.remaining;
  const pos = s.current ? s.current.index + 1 : s.total;
  const pct = s.total ? (done / s.total) * 100 : 0;
  return `<span>${formatCount(pos)} / ${formatCount(s.total)} 枚</span>
    <div class="bar" title="処理済み ${formatCount(done)} 枚"><div style="width:${pct.toFixed(1)}%"></div></div>`;
}

function metaHtml(s: SessionView): string {
  if (!s.current) return `<div class="file-name">完了</div>`;
  const info = store.info?.index === s.current.index ? store.info.info : null;
  const parts: string[] = [];
  if (info) {
    parts.push(`${info.width}×${info.height}`);
    if (info.size != null) parts.push(formatBytes(info.size));
    if (info.taken) parts.push(`${esc(info.taken)} 撮影`);
    else if (info.modified) parts.push(`${formatDate(info.modified)} 更新`);
  }
  return `<div class="file-name" title="${esc(s.current.path)}">${esc(s.current.name)}</div>
    <div class="muted small">${parts.join(" · ")}</div>`;
}

function finishedHtml(s: SessionView): string {
  const k = store.config!.config.keys;
  const skipped = s.skipped
    ? `<p>保留した画像が ${formatCount(s.skipped)} 枚あります。${keycap(displayCombo(k.prev))} で戻って確認できます。</p>`
    : "";
  const empty = s.total === 0 ? `<p>このフォルダには対象の画像がありませんでした。</p>` : "";
  return `<div class="finished">
      <div class="finished-title">${s.total === 0 ? "画像がありません" : "すべての画像を処理しました"}</div>
      ${empty}${skipped}
      <div class="row gap">
        <button class="btn" data-action="change-source">別のフォルダを読み込む</button>
        <button class="btn" data-action="quit">終了する</button>
      </div>
    </div>`;
}

let shownSrc = "";

/** 画像の差し替え。新しい画像を読み終えてから入れ替え、ちらつきを抑える */
function updatePreview(slot: HTMLElement, s: SessionView): void {
  if (!s.current) {
    shownSrc = "";
    slot.innerHTML = finishedHtml(s);
    return;
  }
  const src = lumiUrl(`preview/${s.generation}/${s.current.index}`);
  if (shownSrc === src) return;
  shownSrc = src;
  const img = new Image();
  img.className = "main-image";
  img.alt = s.current.name;
  img.decoding = "async";
  img.src = src;
  const swap = () => {
    if (shownSrc !== src) return;
    slot.replaceChildren(img);
  };
  img.decode().then(swap, () => {
    if (shownSrc !== src) return;
    slot.innerHTML = `<div class="preview-error">${icons.image}<span>この画像は表示できません</span><span class="muted small">スキップまたは振り分けはできます</span></div>`;
  });
  // 読み込みに時間がかかるときは古い画像を薄くして待つ
  window.setTimeout(() => {
    if (shownSrc === src && !slot.contains(img)) slot.classList.add("loading");
  }, 120);
  img.addEventListener("load", () => slot.classList.remove("loading"), { once: true });
  img.addEventListener("error", () => slot.classList.remove("loading"), { once: true });
}

/** フィルムストリップに出す次の画像の数（Rust 側で先読みする upcoming と同じ 4 枚） */
const UPCOMING_TILES = 4;

function thumb(s: SessionView, index: number): string {
  return `<img src="${lumiUrl(`thumb/${s.generation}/${index}`)}" alt="" loading="lazy">`;
}

function filmstripHtml(s: SessionView): string {
  // 左: 直近の操作（古い順。幅が足りなければ古いものから隠れる）。保留した画像はクリックで戻って振り分け直せる
  const past = [...s.history]
    .reverse()
    .map((h) => {
      const label = `<span class="tile-label">${esc(h.label)}</span>`;
      return h.open
        ? `<button class="tile tile-hist tile-open" data-action="jump" data-index="${h.item}" title="${esc(h.label)}：クリックでこの画像に戻って振り分け直す">${thumb(s, h.item)}${label}</button>`
        : `<div class="tile tile-hist kind-${h.kind}" title="${esc(h.label)}（${esc(displayCombo(store.config!.config.keys.undo))} で取り消し）">${thumb(s, h.item)}${label}</div>`;
    })
    .join("");
  // 中央: 現在の画像 / 右: 次の画像（先読みと同じ枚数）
  const now = s.current ? `<div class="tile tile-current">${thumb(s, s.current.index)}</div>` : "";
  const next = s.upcoming
    .slice(0, UPCOMING_TILES)
    .map((i) => `<div class="tile">${thumb(s, i)}</div>`)
    .join("");
  return `<div class="film-past">${past}</div><div class="film-now">${now}</div><div class="film-next">${next}</div>`;
}

/** 保留中の画像の数。クリックで次の保留中の画像へ移動する */
function skipChipHtml(s: SessionView): string {
  if (s.skipped === 0) return "";
  const text = `保留 <strong>${formatCount(s.skipped)}</strong> 枚`;
  return s.nextSkipped != null
    ? `<button class="skip-chip" data-action="jump" data-index="${s.nextSkipped}" title="クリックで保留中の画像へ移動">${text}</button>`
    : `<span class="skip-chip" title="保留中の画像（表示中）">${text}</span>`;
}

function hintsHtml(): string {
  const k = store.config!.config.keys;
  const targets = store.config!.config.targets;
  // 1〜9 のような単独キーだけなら範囲で、組み合わせキーがあれば一般的な説明にする
  const simple = targets.length > 0 && targets.every((t) => !t.key.includes("+"));
  const move =
    targets.length === 0
      ? ""
      : simple && targets.length > 1
        ? `${displayCombo(targets[0].key)}〜${displayCombo(targets[targets.length - 1].key)} で移動`
        : "振り分けキーで移動";
  return [
    move,
    `${displayCombo(k.skip)} スキップ`,
    `${displayCombo(k.delete)} 削除`,
    `${displayCombo(k.prev)} ${displayCombo(k.next)} 前後`,
    `${displayCombo(k.undo)} 取り消し`,
    `${displayCombo(k.toggle_view)} 表示切替`,
  ]
    .filter(Boolean)
    .map((t) => `<span>${esc(t)}</span>`)
    .join("");
}

function targetName(t: Config["targets"][number]): string {
  return t.name || t.path.split(/[\\/]/).filter(Boolean).pop() || t.path;
}

/** 振り分け先リストの見出しにある、パス表示の切り替えボタン */
function pathToggleHtml(): string {
  const c = store.config!.config;
  const on = c.general.show_paths;
  return `<button class="toggle-btn" data-action="toggle-paths" aria-pressed="${on}" title="フォルダのパスを${on ? "隠す" : "表示する"}（${esc(displayCombo(c.keys.toggle_paths))}）。同名のフォルダは常にパスを表示します">パス${on ? "を隠す" : "を表示"}</button>`;
}

function deleteRow(s: SessionView): string {
  const c = store.config!.config;
  const trash = c.general.delete_folder;
  const showPath = c.general.show_paths;
  const detail = trash
    ? `${trash}\n削除フォルダへ移動します。完全に削除するのは終了時の確認後です`
    : "削除フォルダが未指定のため、ファイルは元の場所に置いたまま「削除予定」として記録します。完全に削除するのは終了時の確認後です（削除フォルダは設定の「一般」で指定できます）";
  return `<button class="target-row delete-row" data-action="delete" title="${esc(detail)}" ${s.current ? "" : "disabled"}>
      ${keycap(displayCombo(c.keys.delete))}
      <span class="target-text">
        <span class="target-name">削除</span>
        ${showPath ? `<span class="target-path">${trash ? esc(sp(trash)) : "未指定（その場で削除予定に）"}</span>` : ""}
      </span>
      <span class="count" title="削除予定">${s.pendingDeletions || ""}</span>
    </button>`;
}

function targetsHtml(s: SessionView): string {
  const c = store.config!.config;
  if (c.targets.length === 0) {
    return `<div class="empty-targets">
        <p>振り分け先がまだありません。</p>
        <button class="btn btn-accent" data-action="settings">設定で振り分け先を登録</button>
      </div>${deleteRow(s)}`;
  }
  // パスを隠していても、同じ名前の振り分け先があるものは区別できるようにパスを出す
  const names = c.targets.map(targetName);
  const duplicated = new Set(names.filter((n, i) => names.indexOf(n) !== i));
  const levels = candidateLevels(store.suggest?.view ?? null);
  const unevaluated = new Set(store.suggest?.view?.unevaluated ?? []);
  const rows = c.targets
    .map((t, i) => {
      const showPath = c.general.show_paths || duplicated.has(names[i]);
      const level = levels.get(i);
      const pending = level === undefined && unevaluated.has(names[i]);
      return `<button class="target-row${level ? ` suggested level-${level}` : ""}" data-action="move" data-target="${i}" title="${esc(t.path)}" ${s.current ? "" : "disabled"}>
        ${keycap(displayCombo(t.key))}
        <span class="target-text">
          <span class="target-name">${esc(names[i])}</span>
          ${showPath ? `<span class="target-path">${esc(sp(t.path))}</span>` : ""}
        </span>
        ${pending ? `<span class="tag-unevaluated" title="新しい振り分け先で、この画像はまだ評価されていません">未評価</span>` : ""}
        <span class="count" title="今回移動した枚数">${s.movedCounts[i] || ""}</span>
      </button>`;
    })
    .join("");
  return rows + deleteRow(s);
}

/** ヘッダーの AI バッジ。外部へ画像を送る設定のときは、そうとわかる色と文言で常に出す */
function aiBadgeHtml(): string {
  const text = badgeText(store.config!.config.ai);
  if (!text) return "";
  const external = store.config!.config.ai.backend === "systemone";
  return `<div class="ai-badge${external ? " external" : ""}" title="${external ? "画像を縮小して外部の API へ送信します" : "画像は端末の外へ送られません"}">${esc(text)}</div>`;
}

/** 候補ストリップ（プレビューの下）。AI を使わない設定なら何も出さない */
function suggestHtml(s: SessionView): string {
  const cfg = store.config!.config;
  if (cfg.ai.backend === "off" || !s.current) return "";
  const sg = store.suggest;
  const v = sg?.view ?? null;
  const rediagnose = displayCombo(cfg.keys.rediagnose);

  const notice = noticeText(v);
  if (notice) {
    return `<div class="sg-notice"><span>${esc(notice)}</span><span class="grow"></span><button class="sg-link" data-action="rediagnose">${keycap(rediagnose)} 再診断</button></div>`;
  }
  if (!v || v.state !== "ready") {
    return `<div class="sg-notice muted">候補を調べています…</div>`;
  }

  const unevaluated = unevaluatedText(v.unevaluated);
  const band = unevaluated
    ? `<div class="sg-band"><span>${esc(unevaluated)}</span><span class="grow"></span><button class="sg-link" data-action="rediagnose">${keycap(rediagnose)} 再診断</button></div>`
    : "";

  const label = scoreLabel(v.kind);
  const cards = v.cards
    .map((c) => {
      const pct = barPercent(c.score);
      const title = v.kind === "match" ? `${c.path}\n一致度（確率ではありません）` : c.path;
      return `<button class="sg-card level-${c.level}" data-action="move" data-target="${c.target}" title="${esc(title)}" ${s.current ? "" : "disabled"}>
        <span class="sg-head">${keycap(displayCombo(c.key), "md")}<span class="sg-name">${esc(c.name)}</span>
          <span class="sg-score">${label ? `<small>${label}</small>` : ""}${esc(scoreText(c.score, v.kind))}</span></span>
        <span class="sg-path">${esc(sp(c.path))}</span>
        <span class="sg-bar"><span style="width:${pct}%"></span></span>
      </button>`;
    })
    .join("");
  // 「該当なし」はスキップと同じ（確率を返すバックエンドのみ）
  const none =
    v.noneOfAbove != null
      ? `<button class="sg-card sg-none" data-action="skip" title="スキップして次へ">
        <span class="sg-head">${keycap(displayCombo(cfg.keys.skip), "md")}<span class="sg-name">該当なし</span>
          <span class="sg-score">${esc(scoreText(v.noneOfAbove, "probability"))}</span></span>
        <span class="sg-path">スキップして次へ</span>
        <span class="sg-bar"><span style="width:${barPercent(v.noneOfAbove)}%"></span></span>
      </button>`
      : "";
  const empty = !cards && !none ? `<div class="sg-notice muted">有力な候補はありません</div>` : "";
  return `${band}<div class="sg-cards${sg?.loading ? " refreshing" : ""}">${cards}${none}</div>${empty}`;
}

function chipsHtml(): string {
  const c = store.config!.config;
  return c.targets
    .map(
      (t, i) =>
        `<button class="chip" data-action="move" data-target="${i}" title="${esc(t.path)}">${keycap(displayCombo(t.key))}<span>${esc(targetName(t))}</span></button>`,
    )
    .join("");
}

export function renderSorter(root: HTMLElement): void {
  const s = store.session;
  const cfg = store.config;
  if (!s || !cfg) return;
  const mode: Mode = cfg.config.general.view_mode;
  const viewKey = `sort-${mode}`;
  if (root.dataset.view !== viewKey) {
    root.innerHTML = mode === "focus" ? focusSkeleton() : sidebarSkeleton();
    root.dataset.view = viewKey;
    shownSrc = "";
  }
  const slot = (name: string) => root.querySelector<HTMLElement>(`[data-slot="${name}"]`);
  const source = slot("source");
  if (source) {
    source.textContent = `仕分け元：${sp(s.source)}`;
    source.parentElement!.title = s.source;
  }
  slot("progress")!.innerHTML = progressHtml(s);
  slot("meta")!.innerHTML = metaHtml(s);
  updatePreview(slot("preview")!, s);
  const badge = slot("ai-badge");
  if (badge) badge.innerHTML = aiBadgeHtml();
  const suggest = slot("suggest");
  if (suggest) suggest.innerHTML = suggestHtml(s);
  const film = slot("filmstrip");
  if (film) film.innerHTML = filmstripHtml(s);
  slot("hints")!.innerHTML = skipChipHtml(s) + hintsHtml();
  const targets = slot("targets")!;
  targets.innerHTML = mode === "focus" ? chipsHtml() : targetsHtml(s);
  targets.classList.toggle("compact", !cfg.config.general.show_paths);
  const pathToggle = slot("path-toggle");
  if (pathToggle) pathToggle.innerHTML = pathToggleHtml();
  root.querySelectorAll<HTMLButtonElement>('[data-action="undo"]').forEach((b) => (b.disabled = !s.canUndo));
}

/** 仕分け画面のキー操作。処理したら true */
export function handleSorterKey(combo: string, e: KeyboardEvent): boolean {
  const s = store.session;
  const c = store.config?.config;
  if (!s || !c) return false;
  const k = c.keys;
  const nav = combo === k.prev || combo === k.next;
  // 押しっぱなしで大量に移動しないよう、キーリピートは前後移動だけ受け付ける
  if (e.repeat && !nav) return true;
  if (actions.busy() && !nav) return true;

  if (combo === k.undo) return void actions.undo(), true;
  if (combo === k.prev) return void actions.navigate(false), true;
  if (combo === k.next) return void actions.navigate(true), true;
  if (combo === k.toggle_view) return toggleMode(), true;
  if (combo === k.toggle_paths) return void togglePaths(), true;
  if (combo === k.rediagnose) return actions.rediagnose(), true;
  if (combo === "Escape" && c.general.view_mode === "focus") return setMode("sidebar"), true;
  if (!s.current) return false;
  if (combo === k.skip) return void actions.perform({ kind: "skip" }), true;
  if (combo === k.delete) return void actions.perform({ kind: "delete" }), true;
  const target = c.targets.findIndex((t) => t.key === combo);
  if (target >= 0) return void actions.perform({ kind: "move", target }), true;
  return false;
}

let modeListener: ((mode: Mode) => void) | null = null;

export function onModeChange(fn: (mode: Mode) => void): void {
  modeListener = fn;
}

export function setMode(mode: Mode): void {
  modeListener?.(mode);
}

function toggleMode(): void {
  setMode(store.config?.config.general.view_mode === "focus" ? "sidebar" : "focus");
}

/** 振り分け先リストのパス表示を切り替える。その場で反映し、設定にも保存する（次回も同じ表示） */
export async function togglePaths(): Promise<void> {
  const cfg = store.config;
  if (!cfg) return;
  const show = !cfg.config.general.show_paths;
  cfg.config.general.show_paths = show;
  notify();
  try {
    store.config = await api.setShowPaths(show);
  } catch (e) {
    toast(errorText(e), "error");
  }
}

// ---- サイドバーの幅 ----

const SIDEBAR_KEY = "lumiwake.sidebarWidth";
const SIDEBAR_DEFAULT = 360;
const SIDEBAR_MIN = 240;
/** プレビューに最低限残す幅 */
const STAGE_MIN = 420;

function clampSidebar(width: number): number {
  return Math.round(Math.max(SIDEBAR_MIN, Math.min(width, window.innerWidth - STAGE_MIN)));
}

function applySidebarWidth(width: number): void {
  document.documentElement.style.setProperty("--sidebar-w", `${clampSidebar(width)}px`);
}

function savedSidebarWidth(): number {
  try {
    return Number(localStorage.getItem(SIDEBAR_KEY)) || SIDEBAR_DEFAULT;
  } catch {
    return SIDEBAR_DEFAULT;
  }
}

function saveSidebarWidth(width: number): void {
  try {
    localStorage.setItem(SIDEBAR_KEY, String(width));
  } catch {
    // 保存できなくても今回の表示には影響しない
  }
}

/** 右サイドバーの左端をドラッグして幅を変えられるようにする（幅は次回も使う） */
export function initSidebarResize(): void {
  let width = savedSidebarWidth();
  applySidebarWidth(width);
  window.addEventListener("resize", () => applySidebarWidth(width));

  document.addEventListener("pointerdown", (e) => {
    const handle = (e.target as HTMLElement).closest<HTMLElement>("[data-resize]");
    if (!handle || e.button !== 0) return;
    e.preventDefault();
    handle.setPointerCapture(e.pointerId);
    document.body.classList.add("resizing");
    const move = (ev: PointerEvent) => {
      width = clampSidebar(window.innerWidth - ev.clientX);
      applySidebarWidth(width);
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      document.body.classList.remove("resizing");
      saveSidebarWidth(width);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up, { once: true });
    handle.addEventListener("pointercancel", up, { once: true });
  });

  document.addEventListener("dblclick", (e) => {
    if (!(e.target as HTMLElement).closest("[data-resize]")) return;
    width = SIDEBAR_DEFAULT;
    applySidebarWidth(width);
    saveSidebarWidth(width);
  });
}
