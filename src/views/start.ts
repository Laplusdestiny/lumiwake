// 仕分け元フォルダを選ぶ画面
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "../api";
import * as actions from "../actions";
import { notify, store } from "../store";
import { toast } from "../toast";
import { baseName, esc, icons, shortPath } from "../util";

let chosen: string | null = null;

export function renderStart(root: HTMLElement): void {
  const cfg = store.config;
  if (!cfg) return;
  const g = cfg.config.general;
  const source = chosen ?? g.source_dir ?? null;
  const hasTargets = cfg.config.targets.length > 0;
  root.dataset.view = "start";
  root.innerHTML = `
  <div class="start">
    <div class="start-card">
      <div class="start-brand">Lumiwake</div>
      <p class="muted">キーボードで画像をすばやくフォルダへ振り分けます。</p>
      ${cfg.loadError ? `<div class="notice notice-error">設定ファイルを読み込めなかったため、既定の設定で起動しています。<br><span class="small">${esc(cfg.loadError)}</span></div>` : ""}
      <label class="field-label">仕分け元フォルダ</label>
      <div class="path-picker">
        <div class="path-box ${source ? "" : "muted"}" title="${esc(source ?? "")}">${source ? esc(shortPath(source, store.home)) : "フォルダを選んでください"}</div>
        <button class="btn" data-action="pick-source">${icons.folder}<span>選択</span></button>
      </div>
      ${renderRecent(g.recent_sources ?? [], source)}
      <label class="check"><input type="checkbox" id="include-subdirs" ${g.include_subdirs ? "checked" : ""}> サブフォルダの画像も含める</label>
      ${hasTargets ? "" : `<div class="notice">振り分け先がまだ登録されていません。<button class="link" data-action="settings">設定で登録</button>してから始めると便利です。</div>`}
      <div class="row gap end">
        ${store.session ? `<button class="btn" data-action="back-to-sort">仕分けに戻る</button>` : ""}
        <button class="btn btn-accent" data-action="start" ${source ? "" : "disabled"}>読み込んで開始</button>
      </div>
      <div class="start-foot muted small">
        <button class="link" data-action="settings">設定</button>
        <span>·</span>
        <span>Enter で開始</span>
        ${g.recent_sources?.length ? `<span>·</span><span>↑↓ で履歴から選択</span>` : ""}
      </div>
    </div>
  </div>`;
}

/** 最近使ったフォルダ。同名フォルダを区別できるよう、名前とパスを並べる */
function renderRecent(recent: string[], source: string | null): string {
  if (recent.length === 0) return "";
  const items = recent
    .map((p) => {
      const selected = p === source;
      return `<li class="recent-item ${selected ? "selected" : ""}">
        <button class="recent-pick" data-action="choose-source" data-path="${esc(p)}" title="${esc(p)}" ${selected ? `aria-current="true"` : ""}>
          <span class="recent-name">${esc(baseName(p))}</span>
          <span class="recent-path">${esc(shortPath(p, store.home))}</span>
        </button>
        <button class="icon-btn icon-btn-sm recent-forget" data-action="forget-source" data-path="${esc(p)}" title="履歴から外す" aria-label="履歴から外す">${icons.close}</button>
      </li>`;
    })
    .join("");
  return `<div class="recent">
      <div class="recent-head muted small">最近使ったフォルダ</div>
      <ul class="recent-list">${items}</ul>
    </div>`;
}

function rerender(): void {
  renderStart(document.querySelector("#app")!);
}

/** 履歴から仕分け元を選ぶ */
export function chooseSource(path: string): void {
  chosen = path;
  rerender();
}

/** ↑↓ キーで履歴の中を移る */
export function moveRecentSource(delta: number): void {
  const g = store.config?.config.general;
  const recent = g?.recent_sources ?? [];
  if (recent.length === 0) return;
  const i = recent.indexOf(chosen ?? g?.source_dir ?? "");
  chooseSource(recent[Math.min(Math.max(i + delta, 0), recent.length - 1)]);
}

/** 履歴から外す（フォルダそのものには触れない） */
export async function forgetSource(path: string): Promise<void> {
  store.config = await api.forgetSource(path);
  if (chosen === path) chosen = null;
  notify();
}

export async function pickSource(): Promise<void> {
  const current = chosen ?? store.config?.config.general.source_dir;
  const dir = await open({ directory: true, multiple: false, defaultPath: current ?? undefined, title: "仕分け元フォルダ" });
  if (typeof dir === "string") {
    chosen = dir;
    rerender();
  }
}

export async function startFromForm(): Promise<void> {
  const source = chosen ?? store.config?.config.general.source_dir;
  if (!source) return void (await pickSource());
  const include = document.querySelector<HTMLInputElement>("#include-subdirs")?.checked ?? false;
  try {
    await actions.startSession(source, include);
    chosen = null;
  } catch (e) {
    toast(errorText(e), "error");
  }
}
