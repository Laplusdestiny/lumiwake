import { esc } from "./util";

let timer: number | undefined;

/** 画面下に短いメッセージを出す */
export function toast(message: string, kind: "info" | "error" = "info", ms = 3200): void {
  let el = document.querySelector<HTMLDivElement>("#toast");
  if (!el) {
    el = document.createElement("div");
    el.id = "toast";
    el.setAttribute("role", "status");
    el.setAttribute("aria-live", "polite");
    document.body.appendChild(el);
  }
  el.className = `toast toast-${kind} toast-show`;
  el.innerHTML = esc(message);
  window.clearTimeout(timer);
  timer = window.setTimeout(() => el?.classList.remove("toast-show"), kind === "error" ? ms * 2 : ms);
}
