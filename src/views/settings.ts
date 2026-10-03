// 設定画面。編集中の内容は下書きとして持ち、「保存」で TOML に書き戻す。
import { ask, open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Config, type Issue } from "../api";
import { displayCombo } from "../keys";
import { notify, store } from "../store";
import { toast } from "../toast";
import * as actions from "../actions";
import { consentMessage, parseIntIn, parseUnit } from "../suggest";
import { baseName, esc, icons, keycap, shortPath } from "../util";

type Section = "general" | "keys" | "ai" | "formats" | "file";
type Capture = { kind: "target"; index: number } | { kind: "action"; name: keyof Config["keys"] } | null;

let draft: Config | null = null;
let issues: Issue[] = [];
let section: Section = "keys";
let capture: Capture = null;
let dirty = false;

const ACTIONS: { name: keyof Config["keys"]; label: string }[] = [
  { name: "skip", label: "スキップ（保留）" },
  { name: "delete", label: "削除" },
  { name: "undo", label: "取り消し" },
  { name: "prev", label: "前の画像" },
  { name: "next", label: "次の画像" },
  { name: "toggle_view", label: "表示モードの切り替え" },
  { name: "toggle_paths", label: "フォルダのパス表示の切り替え" },
  { name: "rediagnose", label: "AI 候補の再診断（表示中の 1 枚）" },
];

export function openSettings(sec?: Section): void {
  if (!store.config) return;
  if (store.screen !== "settings") {
    draft = structuredClone(store.config.config);
    issues = store.config.issues;
    dirty = false;
    capture = null;
  }
  if (sec) section = sec;
  store.screen = "settings";
  notify();
}

export async function closeSettings(): Promise<void> {
  if (dirty) {
    const ok = await ask("保存していない変更があります。破棄して戻りますか？", {
      title: "Lumiwake",
      kind: "warning",
      okLabel: "破棄して戻る",
      cancelLabel: "編集を続ける",
    });
    if (!ok) return;
  }
  draft = null;
  capture = null;
  dirty = false;
  store.screen = store.session ? "sort" : "start";
  // AI 設定を変えた可能性があるので、表示中の画像の候補を取り直す
  const cur = store.session?.current;
  if (store.session && cur) actions.loadSuggestions(store.session.generation, cur.index, false);
  notify();
}

// ---- 変更と検証 ----

let validateTimer: number | undefined;

function changed(rerender: boolean): void {
  dirty = true;
  if (rerender) notify();
  window.clearTimeout(validateTimer);
  validateTimer = window.setTimeout(async () => {
    if (!draft) return;
    try {
      issues = await api.validateConfig(draft);
      updateIssues();
    } catch (e) {
      toast(errorText(e), "error");
    }
  }, 150);
}

function usedKeys(except?: Capture): Set<string> {
  const used = new Set<string>();
  if (!draft) return used;
  for (const a of ACTIONS) if (!(except?.kind === "action" && except.name === a.name)) used.add(draft.keys[a.name]);
  draft.targets.forEach((t, i) => {
    if (!(except?.kind === "target" && except.index === i)) used.add(t.key);
  });
  // アプリが固定で使うキー
  for (const k of ["Escape", "Ctrl+Comma", "Ctrl+Q"]) used.add(k);
  return used;
}

/** まだ使われていないキーを順に選ぶ（1〜9, 0 → Ctrl+1〜 → Shift+1〜 → A〜Z） */
function nextFreeKey(used: Set<string>): string {
  const digits = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
  const letters = "ABCEGHIJKLMNOPQRTUVWXY".split("");
  const candidates = [...digits, ...digits.map((d) => `Ctrl+${d}`), ...digits.map((d) => `Shift+${d}`), ...letters];
  return candidates.find((k) => !used.has(k)) ?? "";
}

async function pickFolder(title: string, defaultPath?: string): Promise<string | null> {
  const dir = await open({ directory: true, multiple: false, title, defaultPath });
  return typeof dir === "string" ? dir : null;
}

async function addTarget(): Promise<void> {
  if (!draft) return;
  const last = draft.targets[draft.targets.length - 1]?.path;
  const dir = await pickFolder("振り分け先フォルダ", last);
  if (!dir) return;
  draft.targets.push({ key: nextFreeKey(usedKeys()), name: "", path: dir });
  changed(true);
}

/** 親フォルダを選び、その直下のフォルダをまとめて振り分け先にする */
async function addSubfolders(): Promise<void> {
  if (!draft) return;
  const parent = await pickFolder("振り分け先をまとめて追加（親フォルダを選択）");
  if (!parent) return;
  try {
    const dirs = await api.listSubfolders(parent);
    const existing = new Set(draft.targets.map((t) => t.path));
    let added = 0;
    for (const d of dirs) {
      if (existing.has(d) || d === draft.general.delete_folder) continue;
      draft.targets.push({ key: nextFreeKey(usedKeys()), name: "", path: d });
      added++;
    }
    toast(added ? `${added} 個のフォルダを追加しました` : "追加できるフォルダがありませんでした");
    if (added) changed(true);
  } catch (e) {
    toast(errorText(e), "error");
  }
}

export function isCapturing(): boolean {
  return capture !== null;
}

/** キー登録中に押されたキー */
export function handleCaptureKey(combo: string): void {
  if (!draft || !capture) return;
  if (combo === "Escape") {
    capture = null;
    notify();
    return;
  }
  if (capture.kind === "target") draft.targets[capture.index].key = combo;
  else draft.keys[capture.name] = combo;
  capture = null;
  changed(true);
}

export async function save(): Promise<void> {
  if (!draft) return;
  try {
    store.config = await api.saveConfig(draft);
    draft = structuredClone(store.config.config);
    issues = store.config.issues;
    dirty = false;
    toast("設定を保存しました");
    notify();
  } catch (e) {
    toast(errorText(e), "error");
  }
}

// ---- 表示 ----

function issueFor(key: string): Issue | undefined {
  return issues.find((i) => i.key === key && i.severity === "error") ?? issues.find((i) => i.key === key);
}

function keyButton(combo: string, active: boolean, attrs: string): string {
  const issue = issueFor(combo);
  const cls = ["key-btn", active ? "capturing" : "", issue ? `has-${issue.severity}` : ""].join(" ");
  return `<button class="${cls}" ${attrs} data-key="${esc(combo)}" title="${esc(issue?.message ?? "クリックしてキーを押すと変更できます")}">
    ${active ? "キーを押してください…" : combo ? keycap(displayCombo(combo), "md") : `<span class="muted">未設定</span>`}
  </button>`;
}

function keysSection(d: Config): string {
  const rows = d.targets
    .map((t, i) => {
      const active = capture?.kind === "target" && capture.index === i;
      return `<div class="key-row">
        ${keyButton(t.key, active, `data-action="capture-target" data-index="${i}"`)}
        <input class="input" data-field="target-name" data-index="${i}" value="${esc(t.name)}" placeholder="${esc(baseName(t.path))}" aria-label="表示名">
        <div class="path-cell">
          <span class="mono small" title="${esc(t.path)}">${esc(shortPath(t.path, store.home)) || '<span class="danger">未指定</span>'}</span>
        </div>
        <div class="row-actions">
          <button class="btn btn-sm" data-action="target-path" data-index="${i}">変更</button>
          <button class="btn btn-sm btn-ghost" data-action="target-up" data-index="${i}" ${i === 0 ? "disabled" : ""} aria-label="上へ">↑</button>
          <button class="btn btn-sm btn-ghost" data-action="target-remove" data-index="${i}" aria-label="削除">${icons.close}</button>
        </div>
      </div>`;
    })
    .join("");
  const actions = ACTIONS.map((a) => {
    const active = capture?.kind === "action" && capture.name === a.name;
    return `<div class="action-row"><span>${a.label}</span>${keyButton(d.keys[a.name], active, `data-action="capture-action" data-name="${a.name}"`)}</div>`;
  }).join("");
  return `
    <section>
      <h1>キー割り当て</h1>
      <p class="muted">キーを押して振り分け先フォルダへ移動します。Ctrl / Shift / Alt との組み合わせも使えます。キーの欄をクリックして、割り当てたいキーを押してください。</p>
      <div class="key-table">
        <div class="key-head"><span>キー</span><span>表示名</span><span>パス</span><span></span></div>
        ${rows || `<div class="empty muted">振り分け先はまだありません。</div>`}
      </div>
      <div class="row gap">
        <button class="btn btn-accent" data-action="target-add">＋ 振り分け先を追加</button>
        <button class="btn" data-action="target-add-sub">フォルダ内のサブフォルダをまとめて追加</button>
      </div>
    </section>
    <section>
      <h2>操作キー</h2>
      <div class="action-keys">${actions}</div>
    </section>`;
}

function radio(name: string, value: string, current: string, label: string): string {
  return `<label class="radio"><input type="radio" name="${name}" value="${value}" ${value === current ? "checked" : ""} data-field="${name}"> ${label}</label>`;
}

function generalSection(d: Config): string {
  const g = d.general;
  return `
    <section>
      <h1>一般</h1>
      <h2>削除フォルダ</h2>
      <p class="muted">削除キーを押した画像の移動先です。指定しない場合、画像は元の場所に置いたまま「削除予定」として記録します。どちらの場合も、完全に削除するのはアプリの終了時だけです。</p>
      <div class="path-picker">
        <div class="path-box ${g.delete_folder ? "" : "muted"}" title="${esc(g.delete_folder ?? "")}">${g.delete_folder ? esc(shortPath(g.delete_folder, store.home)) : "未指定（元の場所で削除予定として記録）"}</div>
        <button class="btn" data-action="pick-delete-folder">${icons.folder}<span>選択</span></button>
        ${g.delete_folder ? `<button class="btn btn-ghost" data-action="clear-delete-folder">指定しない</button>` : ""}
      </div>
    </section>
    <section>
      <h2>終了時の削除</h2>
      <div class="radios">
        ${radio("on_exit", "confirm", g.on_exit, "完全に削除するか確認する（おすすめ）")}
        ${radio("on_exit", "delete", g.on_exit, "確認せずにそのまま完全に削除する")}
      </div>
    </section>
    <section>
      <h2>表示</h2>
      <div class="radios">
        ${radio("view_mode", "sidebar", g.view_mode, "サイドバー型（プレビュー＋振り分け先リスト）")}
        ${radio("view_mode", "focus", g.view_mode, "全画面集中型")}
      </div>
      <label class="check"><input type="checkbox" data-field="show_paths" ${g.show_paths ? "checked" : ""}> 振り分け先リストにフォルダのパスを表示する（同名のフォルダは常に表示）</label>
      <div class="radios accent-radios">
        ${(["amber", "blue", "green"] as const)
          .map(
            (a) =>
              `<label class="radio"><input type="radio" name="accent" value="${a}" ${g.accent === a ? "checked" : ""} data-field="accent"><span class="swatch swatch-${a}"></span>${{ amber: "アンバー", blue: "ブルー", green: "グリーン" }[a]}</label>`,
          )
          .join("")}
      </div>
    </section>
    <section>
      <h2>読み込み</h2>
      <label class="check"><input type="checkbox" data-field="include_subdirs" ${g.include_subdirs ? "checked" : ""}> サブフォルダの画像も含める（読み込み時にも変更できます）</label>
      <label class="inline-field">先読みする枚数 <input class="input input-num" type="number" min="0" max="16" data-field="prefetch" value="${g.prefetch}"></label>
    </section>
    <section>
      <h2>アップデート</h2>
      <label class="check"><input type="checkbox" data-field="check_updates" ${g.check_updates ? "checked" : ""}> 起動時に新しいバージョンを確認する</label>
    </section>`;
}

function numField(field: string, value: number, min: number, max: number, step: number, label: string): string {
  return `<label class="inline-field">${label} <input class="input input-num" type="number" min="${min}" max="${max}" step="${step}" data-field="${field}" value="${value}"></label>`;
}

let keyStatus: { envName: string; detected: boolean } | null = null;
let connection: { state: "idle" | "testing" | "ok" | "error"; text: string } = { state: "idle", text: "" };

async function refreshKeyStatus(): Promise<void> {
  try {
    keyStatus = await api.aiKeyStatus();
  } catch {
    keyStatus = null;
  }
  if (store.screen === "settings" && section === "ai") notify();
}

function aiSection(d: Config): string {
  const ai = d.ai;
  const targets = d.targets
    .map(
      (t, i) => `<div class="ai-target">
        <span class="ai-target-name" title="${esc(t.path)}">${esc(t.name || baseName(t.path))}</span>
        <input class="input" data-field="target-desc" data-index="${i}" value="${esc(t.description ?? "")}" placeholder="何を入れるフォルダか（例：旅行先の風景写真）" aria-label="説明文">
        <label class="check ai-target-ext" title="System One（外部 API）へ、このフォルダ名・説明文・選択肢を送りません"><input type="checkbox" data-field="target-exclude" data-index="${i}" ${t.exclude_external ? "checked" : ""}> 外部に送らない</label>
      </div>`,
    )
    .join("");
  const s1 = ai.systemone;
  const keyLine = keyStatus
    ? keyStatus.detected
      ? `<span class="ok small">環境変数 ${esc(keyStatus.envName)} を検出しました</span>`
      : `<span class="warn small">環境変数 ${esc(keyStatus.envName)} が見つかりません</span>`
    : `<span class="muted small">確認中…</span>`;
  const conn =
    connection.state === "idle"
      ? ""
      : `<span class="small ${connection.state === "ok" ? "ok" : connection.state === "error" ? "danger" : "muted"}">${esc(connection.text)}</span>`;
  return `
    <section>
      <h1>AI 候補</h1>
      <p class="muted">表示中の画像に対して、振り分け先の候補を提示します。最終的な振り分けは、いつもどおりあなたのキー操作で行います（AI が自動でファイルを動かすことはありません）。</p>
      <div class="radios">
        ${radio("ai_backend", "local", ai.backend, "ローカル（端末上で実行。画像は外へ送られません）")}
        ${radio("ai_backend", "systemone", ai.backend, "System One 互換 API（画像を縮小して外部へ送信）")}
        ${radio("ai_backend", "off", ai.backend, "使わない")}
      </div>
    </section>
    <section>
      <h2>共通</h2>
      ${numField("ai_top_k", ai.top_k, 1, 10, 1, "候補の数")}
      ${numField("ai_prefetch", ai.prefetch, 0, 16, 1, "先読みする枚数")}
    </section>
    <section>
      <h2>ローカル</h2>
      <p class="muted small">スコアは較正された確率ではないため、画面では「一致度」と表記します。モデルの導入は今後対応します（未導入の間は候補が出ません）。</p>
      <label class="inline-field">モデル <input class="input" data-field="ai_local_model" value="${esc(ai.local.model)}"></label>
      <div class="radios">
        ${radio("ai_local_strategy", "zeroshot", ai.local.strategy, "フォルダ名・説明文だけで判定")}
        ${radio("ai_local_strategy", "knn", ai.local.strategy, "振り分け済みの画像との類似度で判定")}
        ${radio("ai_local_strategy", "hybrid", ai.local.strategy, "両方を組み合わせる（おすすめ）")}
      </div>
      ${numField("ai_local_high", ai.local.high, 0, 1, 0.05, "強調表示のしきい値（高）")}
      ${numField("ai_local_low", ai.local.low, 0, 1, 0.05, "表示するしきい値（低）")}
    </section>
    <section>
      <h2>System One 互換 API</h2>
      <p class="muted small">有効にすると、画像を縮小・再圧縮して下のエンドポイントへ送信します。API キーは設定ファイルには書かず、環境変数から読みます。</p>
      <label class="inline-field wide">エンドポイント <input class="input" data-field="ai_s1_endpoint" value="${esc(s1.endpoint)}" spellcheck="false"></label>
      <p class="muted small">{account} は Cloudflare のアカウント ID に置き換えてください。末尾の clef / clef-flash からモデルを判別します。</p>
      <label class="inline-field">API キーの環境変数名 <input class="input" data-field="ai_s1_env" value="${esc(s1.api_key_env)}" spellcheck="false"></label>
      <div class="row gap">${keyLine}<button class="btn btn-sm btn-ghost" data-action="ai-recheck-key">再確認</button></div>
      ${numField("ai_s1_kb", s1.max_image_kb, 16, 4096, 10, "送信する画像の上限（KB）")}
      ${numField("ai_s1_high", s1.high, 0, 1, 0.05, "強調表示のしきい値（高）")}
      ${numField("ai_s1_low", s1.low, 0, 1, 0.05, "表示するしきい値（低）")}
      <div class="row gap">
        <button class="btn" data-action="ai-test" ${connection.state === "testing" ? "disabled" : ""}>接続テスト</button>
        ${conn}
      </div>
      <p class="muted small">接続テストは保存済みの設定で、合成した小さな画像だけを 1 回送ります（あなたの画像は送りません）。</p>
      <div class="row gap consent ${s1.external_consent ? "given" : ""}">
        <span>外部送信への同意：<strong>${s1.external_consent ? "同意済み" : "未同意"}</strong></span>
        ${
          s1.external_consent
            ? `<button class="btn btn-sm" data-action="ai-revoke-consent">同意を取り消す</button>`
            : `<span class="muted small">System One を選ぶと、確認のうえ同意できます</span>`
        }
      </div>
    </section>
    <section>
      <h2>振り分け先ごとの設定</h2>
      <p class="muted small">説明文は、フォルダ名だけでは伝わらない「何を入れるフォルダか」を書くと判定の助けになります。「外部に送らない」にしたフォルダは、System One へ送る選択肢に含まれません。</p>
      <div class="ai-targets">${targets || `<div class="empty muted">振り分け先はまだありません。</div>`}</div>
    </section>`;
}

function formatsSection(): string {
  const formats = store.config?.formats ?? [];
  return `
    <section>
      <h1>対応形式</h1>
      <p class="muted">RAW 画像と動画は対象外です。表示に対応していない形式のファイルは読み込み時にスキップし、その件数をお知らせします。</p>
      <ul class="format-list">
        ${formats
          .map(
            (f) => `<li><span class="format-label">${esc(f.label)}</span>${
              f.supported
                ? `<span class="ok">対応</span><span class="muted small">${esc(f.decoder ?? "")}</span>`
                : `<span class="muted">このビルドでは未対応</span>`
            }</li>`,
          )
          .join("")}
      </ul>
      <p class="muted small">AVIF（libdav1d）と HEIC（libheif）はネイティブライブラリが必要なため、ビルド時の設定で有効になります。</p>
    </section>`;
}

function fileSection(): string {
  const path = store.config?.path ?? "";
  return `
    <section>
      <h1>設定ファイル</h1>
      <p class="muted">設定は TOML ファイルに保存されています。ファイルを直接編集した場合は「ファイルから再読み込み」で反映できます。この画面で保存すると、ファイルは上書きされます（コメントは保持されません）。</p>
      <div class="path-box mono small" title="${esc(path)}">${esc(shortPath(path, store.home))}</div>
      <div class="row gap">
        <button class="btn" data-action="open-config-file">ファイルを開く</button>
        <button class="btn" data-action="reload-config">ファイルから再読み込み</button>
      </div>
    </section>`;
}

function issuesHtml(): string {
  if (issues.length === 0) return `<span class="muted small">問題はありません</span>`;
  return `<ul class="issues">${issues
    .map((i) => `<li class="${i.severity === "error" ? "danger" : "warn"} small">${i.severity === "error" ? "エラー" : "注意"}: ${esc(i.message)}</li>`)
    .join("")}</ul>`;
}

function updateIssues(): void {
  const el = document.querySelector<HTMLElement>('[data-slot="issues"]');
  if (el) el.innerHTML = issuesHtml();
  const hasError = issues.some((i) => i.severity === "error");
  const saveBtn = document.querySelector<HTMLButtonElement>('[data-action="save-config"]');
  if (saveBtn) saveBtn.disabled = hasError || !dirty;
  document.querySelectorAll<HTMLElement>(".key-btn[data-key]").forEach((b) => {
    const issue = issueFor(b.dataset.key!);
    b.classList.toggle("has-error", issue?.severity === "error");
    b.classList.toggle("has-warning", issue?.severity === "warning");
    if (issue) b.title = issue.message;
  });
}

export function renderSettings(root: HTMLElement): void {
  if (!draft) return;
  const nav = (s: Section, label: string) =>
    `<button class="nav-item ${section === s ? "active" : ""}" data-action="section" data-section="${s}">${label}</button>`;
  const body =
    section === "general"
      ? generalSection(draft)
      : section === "keys"
        ? keysSection(draft)
        : section === "ai"
          ? aiSection(draft)
          : section === "formats"
            ? formatsSection()
            : fileSection();
  const scroll = root.querySelector(".settings-content")?.scrollTop ?? 0;
  // アクセント色は保存前でも見た目に反映する
  document.documentElement.dataset.accent = draft.general.accent;
  root.dataset.view = "settings";
  root.innerHTML = `
  <div class="settings">
    <nav class="settings-nav" aria-label="設定メニュー">
      <button class="back" data-action="close-settings">← ${store.session ? "仕分けに戻る" : "戻る"}</button>
      <div class="nav-title">設定</div>
      ${nav("keys", "キー割り当て")}
      ${nav("ai", "AI 候補")}
      ${nav("general", "一般")}
      ${nav("formats", "対応形式")}
      ${nav("file", "設定ファイル")}
    </nav>
    <div class="settings-main">
      <div class="settings-content">${body}</div>
      <footer class="settings-foot">
        <div class="grow" data-slot="issues">${issuesHtml()}</div>
        <button class="btn" data-action="revert-config" ${dirty ? "" : "disabled"}>元に戻す</button>
        <button class="btn btn-accent" data-action="save-config">保存</button>
      </footer>
    </div>
  </div>`;
  root.querySelector(".settings-content")!.scrollTop = scroll;
  updateIssues();
}

/** クリック操作。処理したら true */
export async function handleSettingsAction(action: string, el: HTMLElement): Promise<boolean> {
  if (!draft) return false;
  const index = Number(el.dataset.index);
  switch (action) {
    case "section":
      section = el.dataset.section as Section;
      capture = null;
      if (section === "ai") void refreshKeyStatus();
      notify();
      return true;
    case "ai-recheck-key":
      keyStatus = null;
      notify();
      void refreshKeyStatus();
      return true;
    case "ai-test":
      connection = { state: "testing", text: "接続しています…" };
      notify();
      try {
        const model = await api.testSystemone();
        connection = { state: "ok", text: `接続できました（モデル: ${model}）` };
      } catch (e) {
        connection = { state: "error", text: errorText(e) };
      }
      notify();
      return true;
    case "ai-revoke-consent":
      draft.ai.systemone.external_consent = false;
      changed(true);
      return true;
    case "close-settings":
      await closeSettings();
      return true;
    case "capture-target":
      capture = { kind: "target", index };
      notify();
      return true;
    case "capture-action":
      capture = { kind: "action", name: el.dataset.name as keyof Config["keys"] };
      notify();
      return true;
    case "target-add":
      await addTarget();
      return true;
    case "target-add-sub":
      await addSubfolders();
      return true;
    case "target-path": {
      const dir = await pickFolder("振り分け先フォルダ", draft.targets[index].path || undefined);
      if (dir) {
        draft.targets[index].path = dir;
        changed(true);
      }
      return true;
    }
    case "target-up":
      if (index > 0) {
        const t = draft.targets;
        [t[index - 1], t[index]] = [t[index], t[index - 1]];
        changed(true);
      }
      return true;
    case "target-remove":
      draft.targets.splice(index, 1);
      changed(true);
      return true;
    case "pick-delete-folder": {
      const dir = await pickFolder("削除フォルダ", draft.general.delete_folder);
      if (dir) {
        draft.general.delete_folder = dir;
        changed(true);
      }
      return true;
    }
    case "clear-delete-folder":
      delete draft.general.delete_folder;
      changed(true);
      return true;
    case "save-config":
      await save();
      return true;
    case "revert-config":
      draft = structuredClone(store.config!.config);
      issues = store.config!.issues;
      dirty = false;
      notify();
      return true;
    case "open-config-file":
      await api.openConfigFile().catch((e) => toast(errorText(e), "error"));
      return true;
    case "reload-config":
      if (dirty && !(await ask("編集中の内容を破棄して、ファイルから読み込み直しますか？", { title: "Lumiwake", kind: "warning" }))) {
        return true;
      }
      try {
        store.config = await api.reloadConfig();
        draft = structuredClone(store.config.config);
        issues = store.config.issues;
        dirty = false;
        toast("設定ファイルを読み込み直しました");
        notify();
      } catch (e) {
        toast(errorText(e), "error");
      }
      return true;
  }
  return false;
}

/** 入力欄の変更 */
export function handleSettingsInput(el: HTMLInputElement): void {
  if (!draft) return;
  const g = draft.general;
  switch (el.dataset.field) {
    case "target-name":
      draft.targets[Number(el.dataset.index)].name = el.value;
      return changed(false);
    case "on_exit":
      g.on_exit = el.value as Config["general"]["on_exit"];
      return changed(false);
    case "view_mode":
      g.view_mode = el.value as Config["general"]["view_mode"];
      return changed(false);
    case "accent":
      g.accent = el.value as Config["general"]["accent"];
      document.documentElement.dataset.accent = g.accent;
      return changed(false);
    case "include_subdirs":
      g.include_subdirs = el.checked;
      return changed(false);
    case "check_updates":
      g.check_updates = el.checked;
      return changed(false);
    case "show_paths":
      g.show_paths = el.checked;
      return changed(false);
    case "prefetch":
      g.prefetch = Math.max(0, Math.min(16, Number(el.value) || 0));
      return changed(false);
    case "ai_backend":
      return void chooseBackend(el.value as Config["ai"]["backend"]);
    case "ai_top_k":
      draft.ai.top_k = parseIntIn(el.value, 1, 10, draft.ai.top_k);
      return changed(false);
    case "ai_prefetch":
      draft.ai.prefetch = parseIntIn(el.value, 0, 16, draft.ai.prefetch);
      return changed(false);
    case "ai_local_model":
      draft.ai.local.model = el.value;
      return changed(false);
    case "ai_local_strategy":
      draft.ai.local.strategy = el.value as Config["ai"]["local"]["strategy"];
      return changed(false);
    case "ai_local_high":
      draft.ai.local.high = parseUnit(el.value, draft.ai.local.high);
      return changed(false);
    case "ai_local_low":
      draft.ai.local.low = parseUnit(el.value, draft.ai.local.low);
      return changed(false);
    case "ai_s1_endpoint":
      draft.ai.systemone.endpoint = el.value;
      return changed(false);
    case "ai_s1_env":
      draft.ai.systemone.api_key_env = el.value;
      return changed(false);
    case "ai_s1_kb":
      draft.ai.systemone.max_image_kb = parseIntIn(el.value, 16, 4096, draft.ai.systemone.max_image_kb);
      return changed(false);
    case "ai_s1_high":
      draft.ai.systemone.high = parseUnit(el.value, draft.ai.systemone.high);
      return changed(false);
    case "ai_s1_low":
      draft.ai.systemone.low = parseUnit(el.value, draft.ai.systemone.low);
      return changed(false);
    case "target-desc":
      draft.targets[Number(el.dataset.index)].description = el.value;
      return changed(false);
    case "target-exclude":
      draft.targets[Number(el.dataset.index)].exclude_external = el.checked;
      return changed(false);
  }
}

/** バックエンドの選択。System One は、画像が外部へ送られることへの同意が済むまで有効にしない */
async function chooseBackend(backend: Config["ai"]["backend"]): Promise<void> {
  if (!draft) return;
  if (backend === "systemone" && !draft.ai.systemone.external_consent) {
    const ok = await ask(consentMessage(draft.ai.systemone.endpoint), {
      title: "画像の外部送信への同意",
      kind: "warning",
      okLabel: "同意して有効にする",
      cancelLabel: "有効にしない",
    });
    if (!ok) {
      notify(); // ラジオボタンを元の選択に戻す
      return;
    }
    draft.ai.systemone.external_consent = true;
  }
  draft.ai.backend = backend;
  changed(true);
}
