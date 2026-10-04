import { describe, expect, it } from "vitest";
import { baseName, esc, formatBytes, formatCount, formatDate, keycap, shortPath } from "./util";

describe("esc", () => {
  it("ファイル名に入りうる HTML の特殊文字をエスケープする", () => {
    expect(esc(`<img src=x onerror="a('b')">&`)).toBe("&lt;img src=x onerror=&quot;a(&#39;b&#39;)&quot;&gt;&amp;");
  });

  it("null・undefined は空文字、数値は文字列にする", () => {
    expect(esc(null)).toBe("");
    expect(esc(undefined)).toBe("");
    expect(esc(42)).toBe("42");
  });
});

describe("formatBytes", () => {
  it("単位を切り替え、10 未満は小数 1 桁にする", () => {
    expect(formatBytes(null)).toBe("");
    expect(formatBytes(undefined)).toBe("");
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(50 * 1024)).toBe("50 KB");
    expect(formatBytes(3.2 * 1024 * 1024)).toBe("3.2 MB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
    expect(formatBytes(2048 * 1024 ** 3)).toBe("2048 GB");
  });
});

describe("formatDate", () => {
  it("ローカル時刻の YYYY-MM-DD HH:mm にする", () => {
    expect(formatDate(null)).toBe("");
    expect(formatDate(new Date(2026, 0, 5, 7, 3).getTime())).toBe("2026-01-05 07:03");
  });
});

describe("formatCount", () => {
  it("3 桁区切り", () => {
    expect(formatCount(1234567)).toBe("1,234,567");
  });
});

describe("baseName", () => {
  it("Unix と Windows の区切りの両方に対応する", () => {
    expect(baseName("/home/u/写真/風景")).toBe("風景");
    expect(baseName("C:\\Users\\u\\Pictures\\")).toBe("Pictures");
    expect(baseName("name.jpg")).toBe("name.jpg");
    expect(baseName("/")).toBe("/");
  });
});

describe("shortPath", () => {
  it("ホームディレクトリを ~ に縮める", () => {
    expect(shortPath("/home/u/写真", "/home/u")).toBe("~/写真");
    expect(shortPath("/mnt/nas/写真", "/home/u")).toBe("/mnt/nas/写真");
    expect(shortPath("/home/u/写真", null)).toBe("/home/u/写真");
  });
});

describe("keycap", () => {
  it("キー表示をエスケープして囲む", () => {
    expect(keycap("<")).toBe('<span class="keycap keycap-sm">&lt;</span>');
    expect(keycap("Esc", "md")).toContain("keycap-md");
  });
});
