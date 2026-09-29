// 仕分け元フォルダを選ぶ画面
import { open } from "@tauri-apps/plugin-dialog";
import { errorText } from "../api";
import * as actions from "../actions";
import { store } from "../store";
import { toast } from "../toast";
import { esc, icons, shortPath } from "../util";

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
      </div>
    </div>
  </div>`;
}

export async function pickSource(): Promise<void> {
  const current = chosen ?? store.config?.config.general.source_dir;
  const dir = await open({ directory: true, multiple: false, defaultPath: current ?? undefined, title: "仕分け元フォルダ" });
  if (typeof dir === "string") {
    chosen = dir;
    renderStart(document.querySelector("#app")!);
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
