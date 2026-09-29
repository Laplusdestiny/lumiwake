// 自動アップデート（Tauri updater プラグイン + GitHub Releases の latest.json）
import { ask } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { api, errorText } from "./api";
import { toast } from "./toast";
import { esc } from "./util";

let available: Update | null = null;

/** 起動時に新しいバージョンを確認する。失敗しても仕分けの邪魔はしない */
export async function checkForUpdates(): Promise<void> {
  try {
    available = await check();
  } catch (e) {
    // オフライン・リリース前・公開鍵の未設定などでは確認できない
    console.info("アップデートを確認できませんでした:", errorText(e));
    return;
  }
  if (available) showBanner(available);
}

function showBanner(update: Update): void {
  const el = document.createElement("div");
  el.className = "update-banner";
  el.innerHTML = `<span>新しいバージョン ${esc(update.version)} があります</span>
    <button class="btn btn-sm btn-accent" data-update="install">更新して再起動</button>
    <button class="icon-btn icon-btn-sm" data-update="dismiss" aria-label="閉じる">×</button>`;
  el.addEventListener("click", async (ev) => {
    const action = (ev.target as HTMLElement).closest<HTMLElement>("[data-update]")?.dataset.update;
    if (action === "dismiss") el.remove();
    if (action === "install") {
      el.remove();
      await install(update);
    }
  });
  document.body.appendChild(el);
}

async function install(update: Update): Promise<void> {
  const pending = await api.pendingDeletions().catch(() => null);
  if (pending && pending.items.length > 0) {
    const ok = await ask(
      `削除予定のファイルが ${pending.items.length} 件あります。更新のために再起動すると削除予定の記録は消えます（ファイルは削除されずに残ります）。続けますか？`,
      { title: "Lumiwake", kind: "warning", okLabel: "更新する", cancelLabel: "やめる" },
    );
    if (!ok) return;
  }
  try {
    toast("更新をダウンロードしています…", "info", 60000);
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    toast(`更新できませんでした: ${errorText(e)}`, "error");
  }
}
