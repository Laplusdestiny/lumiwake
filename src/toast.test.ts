// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "./toast";

afterEach(() => vi.useRealTimers());

describe("toast", () => {
  it("1 つの要素を使い回してメッセージを出し、時間がたつと隠す", () => {
    vi.useFakeTimers();
    toast("<b>保存</b>しました");
    const el = document.querySelector("#toast")!;
    expect(el.innerHTML).toBe("&lt;b&gt;保存&lt;/b&gt;しました");
    expect(el.className).toBe("toast toast-info toast-show");
    vi.advanceTimersByTime(3200);
    expect(el.classList.contains("toast-show")).toBe(false);

    // エラーは 2 倍の時間表示する
    toast("失敗", "error", 1000);
    expect(document.querySelectorAll("#toast")).toHaveLength(1);
    vi.advanceTimersByTime(1999);
    expect(el.classList.contains("toast-show")).toBe(true);
    vi.advanceTimersByTime(1);
    expect(el.classList.contains("toast-show")).toBe(false);
  });
});
