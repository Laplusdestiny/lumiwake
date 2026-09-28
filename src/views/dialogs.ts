// 同名ファイルの比較ダイアログと、終了時の削除確認ダイアログ
import { api, errorText, lumiUrl, type ConflictChoice, type DeletionSummary, type FileView } from "../api";
import * as actions from "../actions";
import { notify, store } from "../store";
import { toast } from "../toast";
import { baseName, esc, formatBytes, formatDate, keycap, shortPath } from "../util";

// ---- 同名ファイル ----

const CHOICES: { key: string; choice: ConflictChoice; title: string; desc: string }[] = [
  { key: "1", choice: "keepExisting", title: "既存を残す", desc: "移動しようとした画像は削除予定にします" },
  { key: "2", choice: "overwrite", title: "上書き", desc: "既存のファイルは削除予定にします" },
  { key: "3", choice: "keepBoth", title: "両方残す", desc: "移動する画像の名前に番号を付けます" },
  { key: "4", choice: "skip", title: "スキップ", desc: "今回は移動しません（保留）" },
];

function fileCard(title: string, f: FileView, src: string): string {
  return `<figure class="compare-card">
      <figcaption class="compare-title">${esc(title)}</figcaption>
      <div class="compare-image"><img src="${src}" alt="${esc(f.name)}"></div>
      <div class="compare-meta">
        <div class="file-name small" title="${esc(f.path)}">${esc(f.name)}</div>
        <div class="muted small">${[formatBytes(f.size), formatDate(f.modified) && `${formatDate(f.modified)} 更新`].filter(Boolean).join(" · ")}</div>
      </div>
    </figure>`;
}

function conflictHtml(): string {
  const s = store.session!;
  const c = s.conflict!;
  const g = s.generation;
  // 同じ名前でも内容が入れ替わることがあるので、URL に項目番号を含めて毎回取り直す
  const bust = `?item=${c.item}`;
  return `<div class="modal-backdrop">
    <div class="modal modal-wide" role="dialog" aria-modal="true" aria-labelledby="conflict-title">
      <h2 id="conflict-title">同じ名前のファイルがあります</h2>
      <p class="muted">振り分け先「${esc(c.label)}」<span class="mono small">（${esc(shortPath(c.dir, store.home))}）</span>に <strong>${esc(c.incoming.name)}</strong> がすでにあります。</p>
      <div class="compare">
        ${fileCard("移動する画像", c.incoming, lumiUrl(`conflict/${g}/incoming`) + bust)}
        ${fileCard("移動先にある画像", c.existing, lumiUrl(`conflict/${g}/existing`) + bust)}
      </div>
      <div class="choices">
        ${CHOICES.map(
          (ch) => `<button class="choice" data-action="conflict" data-choice="${ch.choice}">
            ${keycap(ch.key, "md")}<span class="choice-text"><span class="choice-title">${ch.title}</span><span class="muted small">${ch.desc}</span></span>
          </button>`,
        ).join("")}
      </div>
      <div class="modal-foot muted small">${keycap("Esc")} キャンセル（この画像に戻る）</div>
    </div>
  </div>`;
}

export function handleConflictKey(combo: string): boolean {
  if (!store.session?.conflict) return false;
  const ch = CHOICES.find((c) => c.key === combo || `Num${c.key}` === combo);
  if (ch) {
    void actions.resolveConflict(ch.choice);
    return true;
  }
  if (combo === "Escape") {
    void actions.cancelConflict();
    return true;
  }
  return true; // ダイアログ表示中は他のキーを受け付けない
}

// ---- 終了時の削除確認 ----

type ExitState =
  | { kind: "confirm"; summary: DeletionSummary }
  | { kind: "failed"; failed: [string, string][]; deleted: number };

let exitState: ExitState | null = null;

const REASON: Record<string, string> = {
  deleteKey: "削除キー",
  keepExisting: "同名ファイルがあったため（既存を残す）",
  overwritten: "上書きで置き換えられたファイル",
};

function exitHtml(st: ExitState): string {
  if (st.kind === "failed") {
    return `<div class="modal-backdrop">
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="exit-title">
        <h2 id="exit-title">一部のファイルを削除できませんでした</h2>
        <p class="muted">${st.deleted} 件を削除しました。次の ${st.failed.length} 件は削除できなかったため、そのまま残っています。</p>
        <ul class="pending-list">
          ${st.failed.map(([p, err]) => `<li><div class="mono small">${esc(p)}</div><div class="danger small">${esc(err)}</div></li>`).join("")}
        </ul>
        <div class="row gap end">
          <button class="btn" data-action="exit-cancel">戻る</button>
          <button class="btn btn-accent" data-action="exit-force">このまま終了</button>
        </div>
      </div>
    </div>`;
  }
  const { summary } = st;
  const inTrash = summary.items.filter((i) => i.inTrash).length;
  const inPlace = summary.items.length - inTrash;
  const places: string[] = [];
  if (inTrash) places.push(`削除フォルダ内の ${inTrash} 件`);
  if (inPlace) places.push(`元の場所で削除予定にした ${inPlace} 件`);
  return `<div class="modal-backdrop">
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby="exit-title">
      <h2 id="exit-title">削除予定のファイルが ${summary.items.length} 件あります</h2>
      <p class="muted">${places.join("と、")}を完全に削除しますか？ 完全に削除したファイルは元に戻せません。削除せずに終了した場合、ファイルはそのまま残ります。</p>
      ${
        summary.trashDir && inTrash
          ? `<div class="trash-line"><span class="muted small">削除フォルダ</span><span class="mono small">${esc(shortPath(summary.trashDir, store.home))}</span><button class="btn btn-sm" data-action="open-location" data-path="${esc(summary.trashDir)}">フォルダを開く</button></div>`
          : ""
      }
      <ul class="pending-list">
        ${summary.items
          .map(
            (i) => `<li>
              <div class="pending-main">
                <div class="small">${esc(baseName(i.path))}</div>
                <div class="mono small muted" title="${esc(i.path)}">${esc(shortPath(i.path, store.home))}</div>
                <div class="small muted">${esc(REASON[i.reason] ?? i.reason)}</div>
              </div>
              <button class="btn btn-sm" data-action="open-location" data-path="${esc(i.path)}">場所を開く</button>
            </li>`,
          )
          .join("")}
      </ul>
      <div class="row gap end">
        <button class="btn" data-action="exit-cancel" autofocus>キャンセル</button>
        <button class="btn" data-action="exit-keep">削除せずに終了</button>
        <button class="btn btn-danger" data-action="exit-delete">完全に削除して終了</button>
      </div>
    </div>
  </div>`;
}

/** アプリを終了する。削除予定があれば設定に従って確認する */
export async function requestQuit(): Promise<void> {
  if (exitState) return;
  try {
    const summary = await api.pendingDeletions();
    if (summary.items.length === 0) return void (await api.exitApp());
    if (summary.onExit === "delete") return void (await finalize(true));
    exitState = { kind: "confirm", summary };
    store.modal = true;
    notify();
  } catch (e) {
    toast(errorText(e), "error");
  }
}

async function finalize(del: boolean): Promise<void> {
  const report = await api.finalizeAndExit(del);
  // すべて削除できた場合はここで終了している
  if (report.failed.length > 0) {
    exitState = { kind: "failed", failed: report.failed, deleted: report.deleted.length };
    store.modal = true;
    notify();
  }
}

function closeExit(): void {
  exitState = null;
  store.modal = false;
  notify();
}

export async function handleExitAction(action: string, el: HTMLElement): Promise<boolean> {
  switch (action) {
    case "exit-cancel":
      closeExit();
      return true;
    case "exit-keep":
      await finalize(false);
      return true;
    case "exit-delete":
      await finalize(true);
      return true;
    case "exit-force":
      await api.exitApp();
      return true;
    case "open-location":
      await api.openLocation(el.dataset.path!).catch((e) => toast(errorText(e), "error"));
      return true;
  }
  return false;
}

export function handleExitKey(combo: string): boolean {
  if (!exitState) return false;
  if (combo === "Escape") closeExit();
  return true;
}

// ---- 表示 ----

export function renderOverlay(): void {
  let el = document.querySelector<HTMLDivElement>("#overlay");
  if (!el) {
    el = document.createElement("div");
    el.id = "overlay";
    document.body.appendChild(el);
  }
  const html = exitState ? exitHtml(exitState) : store.screen === "sort" && store.session?.conflict ? conflictHtml() : "";
  const key = exitState ? `exit-${exitState.kind}` : html ? `conflict-${store.session?.conflict?.item}` : "";
  if (el.dataset.key === key) return;
  el.dataset.key = key;
  el.innerHTML = html;
  el.querySelector<HTMLElement>("[autofocus]")?.focus();
}
