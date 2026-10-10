// Lumiwake の画面。表示とキー入力の受け渡しだけを行い、ファイル操作はすべて Rust コアに任せる。
import { homeDir } from "@tauri-apps/api/path";
import { listen } from "@tauri-apps/api/event";
import { api, errorText, type Config } from "./api";
import * as actions from "./actions";
import { eventToCombo, isBrowserKey } from "./keys";
import { notify, onChange, store } from "./store";
import { toast } from "./toast";
import { handleConflictKey, handleExitAction, handleExitKey, renderOverlay, requestQuit } from "./views/dialogs";
import {
  handleCaptureKey,
  handleSettingsAction,
  handleSettingsInput,
  isCapturing,
  openSettings,
  renderSettings,
} from "./views/settings";
import { handleSorterKey, initSidebarResize, onModeChange, renderSorter, setMode, togglePaths } from "./views/sorter";
import { chooseSource, forgetSource, moveRecentSource, pickSource, renderStart, startFromForm } from "./views/start";
import { checkForUpdates } from "./updater";

const root = document.querySelector<HTMLDivElement>("#app")!;

function render(): void {
  if (!store.config) return;
  if (store.screen !== "settings") document.documentElement.dataset.accent = store.config.config.general.accent;
  switch (store.screen) {
    case "start":
      renderStart(root);
      break;
    case "sort":
      if (store.session) renderSorter(root);
      else renderStart(root);
      break;
    case "settings":
      renderSettings(root);
      break;
  }
  renderOverlay();
}

onChange(render);
initSidebarResize();

// 表示モードの切り替えは、その場で反映してから設定にも保存する（次回起動時も同じモード）
onModeChange(async (mode: Config["general"]["view_mode"]) => {
  if (!store.config || store.config.config.general.view_mode === mode) return;
  store.config.config.general.view_mode = mode;
  notify();
  try {
    store.config = await api.setViewMode(mode);
  } catch (e) {
    toast(errorText(e), "error");
  }
});

// ---- キー入力 ----

function isTextInput(el: EventTarget | null): boolean {
  return el instanceof HTMLInputElement
    ? !["checkbox", "radio", "button"].includes(el.type)
    : el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;
}

window.addEventListener("keydown", (e) => {
  if (e.isComposing) return;
  const combo = eventToCombo(e);
  if (!combo) return;
  const handled = (() => {
    if (isCapturing()) return handleCaptureKey(combo), true;
    if (handleExitKey(combo)) return true;
    if (isTextInput(e.target)) {
      if (combo === "Escape") (e.target as HTMLElement).blur();
      return combo === "Escape";
    }
    if (combo === "Ctrl+Comma" && store.screen !== "settings") return openSettings(), true;
    if (combo === "Ctrl+Q") return void requestQuit(), true;
    switch (store.screen) {
      case "sort":
        if (store.session?.conflict) return handleConflictKey(combo);
        return handleSorterKey(combo, e);
      case "start":
        if (combo === "Enter") return void startFromForm(), true;
        if (combo === "Up" || combo === "Down") return moveRecentSource(combo === "Down" ? 1 : -1), true;
        if (combo === "Escape" && store.session) return ((store.screen = "sort"), notify()), true;
        return false;
      case "settings":
        if (combo === "Escape") return void handleSettingsAction("close-settings", root), true;
        return false;
    }
  })();
  // 仕分けのキーと WebView の既定動作（再読み込み・印刷・検索など）が重ならないようにする
  if (handled || isBrowserKey(combo)) e.preventDefault();
});

// ---- クリック ----

document.addEventListener("click", async (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (!el || (el as HTMLButtonElement).disabled) return;
  const action = el.dataset.action!;
  try {
    if (await handleExitAction(action, el)) return;
    if (store.screen === "settings" && (await handleSettingsAction(action, el))) return;
    switch (action) {
      case "move":
        return void actions.perform({ kind: "move", target: Number(el.dataset.target) });
      case "delete":
        return void actions.perform({ kind: "delete" });
      case "undo":
        return void actions.undo();
      case "jump":
        return void actions.jumpTo(Number(el.dataset.index));
      case "conflict":
        return void actions.resolveConflict(el.dataset.choice as never);
      case "mode":
        return setMode(el.dataset.mode as Config["general"]["view_mode"]);
      case "toggle-paths":
        return await togglePaths();
      case "settings":
        return openSettings();
      case "change-source":
        store.screen = "start";
        return notify();
      case "back-to-sort":
        store.screen = "sort";
        return notify();
      case "pick-source":
        return await pickSource();
      case "choose-source":
        return chooseSource(el.dataset.path!);
      case "forget-source":
        return await forgetSource(el.dataset.path!);
      case "start":
        return await startFromForm();
      case "quit":
        return await requestQuit();
    }
  } catch (err) {
    toast(errorText(err), "error");
  }
});

document.addEventListener("input", (e) => {
  if (store.screen === "settings" && e.target instanceof HTMLInputElement) handleSettingsInput(e.target);
});

// 右クリックメニュー（再読み込みなど）は出さない
document.addEventListener("contextmenu", (e) => {
  if (!isTextInput(e.target)) e.preventDefault();
});

// ---- 起動 ----

async function boot(): Promise<void> {
  try {
    store.home = (await homeDir()).replace(/[\\/]+$/, "");
  } catch {
    store.home = null;
  }
  store.config = await api.getConfig();
  const session = await api.getSession();
  if (session) {
    store.screen = "sort";
    actions.setSession(session);
  } else {
    store.screen = "start";
    notify();
  }
  // ウィンドウの閉じるボタン: 削除予定があれば確認してから終了する
  await listen("lumiwake://close-requested", () => void requestQuit());
  if (store.config.config.general.check_updates) void checkForUpdates();
}

boot().catch((e) => {
  root.innerHTML = `<div class="fatal">起動できませんでした: ${String(errorText(e)).replace(/</g, "&lt;")}</div>`;
});
