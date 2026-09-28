// 仕分け画面（サイドバー型・全画面集中型）
import { lumiUrl, type Config, type SessionView } from "../api";
import * as actions from "../actions";
import { displayCombo } from "../keys";
import { store } from "../store";
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
        <div class="filmstrip" data-slot="filmstrip"></div>
        <div class="hints" data-slot="hints"></div>
      </main>
      <aside class="targets-panel">
        <div class="panel-head"><div class="panel-title">フォルダ</div><div class="muted small">同名フォルダはパスで区別</div></div>
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

function filmstripHtml(s: SessionView): string {
  const tiles: string[] = [];
  const history = [...s.history].reverse();
  for (const h of history) tiles.push(`<div class="tile tile-done" title="${esc(h.label)}">${esc(h.label)}</div>`);
  for (let i = history.length; i < 3; i++) tiles.push(`<div class="tile tile-empty"></div>`);
  if (s.current) {
    tiles.push(`<div class="tile tile-current"><img src="${lumiUrl(`thumb/${s.generation}/${s.current.index}`)}" alt=""></div>`);
  }
  for (const i of s.upcoming) {
    tiles.push(`<div class="tile"><img src="${lumiUrl(`thumb/${s.generation}/${i}`)}" alt="" loading="lazy"></div>`);
  }
  return tiles.join("");
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

function deleteRow(s: SessionView): string {
  const c = store.config!.config;
  const trash = c.general.delete_folder;
  return `<button class="target-row delete-row" data-action="delete" ${s.current ? "" : "disabled"}>
      ${keycap(displayCombo(c.keys.delete))}
      <span class="target-text">
        <span class="target-name">削除</span>
        <span class="target-path">${trash ? esc(sp(trash)) : "削除予定として記録（削除フォルダ未指定）"}</span>
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
  const rows = c.targets
    .map(
      (t, i) => `<button class="target-row" data-action="move" data-target="${i}" ${s.current ? "" : "disabled"}>
        ${keycap(displayCombo(t.key))}
        <span class="target-text">
          <span class="target-name">${esc(t.name || t.path.split(/[\\/]/).pop())}</span>
          <span class="target-path" title="${esc(t.path)}">${esc(sp(t.path))}</span>
        </span>
        <span class="count" title="今回移動した枚数">${s.movedCounts[i] || ""}</span>
      </button>`,
    )
    .join("");
  return rows + deleteRow(s);
}

function chipsHtml(): string {
  const c = store.config!.config;
  return c.targets
    .map(
      (t, i) =>
        `<button class="chip" data-action="move" data-target="${i}">${keycap(displayCombo(t.key))}<span>${esc(t.name || t.path.split(/[\\/]/).pop())}</span></button>`,
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
  const film = slot("filmstrip");
  if (film) film.innerHTML = filmstripHtml(s);
  slot("hints")!.innerHTML = hintsHtml();
  slot("targets")!.innerHTML = mode === "focus" ? chipsHtml() : targetsHtml(s);
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
