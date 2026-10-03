import { describe, expect, it } from "vitest";
import type { Ai, SuggestionCard, Suggestions } from "./api";
import {
  badgeText,
  barPercent,
  candidateLevels,
  consentMessage,
  endpointHost,
  noticeText,
  parseIntIn,
  parseUnit,
  scoreLabel,
  scoreText,
  unevaluatedText,
} from "./suggest";

const card = (target: number, score: number, level: SuggestionCard["level"] = "mid"): SuggestionCard => ({
  target,
  key: String(target + 1),
  name: `t${target}`,
  path: `/p/${target}`,
  score,
  level,
});

const view = (over: Partial<Suggestions> & Record<string, unknown> = {}): Suggestions =>
  ({
    backend: "local",
    kind: "match",
    sendsImages: false,
    cards: [],
    noneOfAbove: null,
    unevaluated: [],
    fromCache: false,
    state: "ready",
    ...over,
  }) as Suggestions;

describe("scoreText / scoreLabel", () => {
  it("shows probabilities as percent and match scores as a bare number labelled 一致度", () => {
    expect(scoreText(0.724, "probability")).toBe("72%");
    expect(scoreText(0.724, "match")).toBe("72");
    expect(scoreLabel("match")).toBe("一致度");
    expect(scoreLabel("probability")).toBe("");
    expect(scoreLabel(null)).toBe("");
  });
  it("clamps out-of-range scores", () => {
    expect(scoreText(1.4, "probability")).toBe("100%");
    expect(scoreText(-0.2, "match")).toBe("0");
    expect(barPercent(2)).toBe(100);
    expect(barPercent(-1)).toBe(0);
    expect(barPercent(0.5)).toBe(50);
  });
});

describe("badgeText", () => {
  const ai = (backend: Ai["backend"], consent = false): Ai =>
    ({ backend, systemone: { external_consent: consent } }) as Ai;
  it("is hidden when AI is off", () => {
    expect(badgeText(ai("off"))).toBeNull();
  });
  it("names the local backend without mentioning external sending", () => {
    expect(badgeText(ai("local"))).toBe("AI：ローカル");
  });
  it("makes external sending explicit, and shows when consent is still missing", () => {
    expect(badgeText(ai("systemone", true))).toContain("外部送信");
    expect(badgeText(ai("systemone", false))).toContain("同意");
    expect(badgeText(ai("systemone", false))).not.toContain("外部送信中");
  });
});

describe("unevaluatedText", () => {
  it("is empty when nothing is unevaluated", () => {
    expect(unevaluatedText([])).toBeNull();
  });
  it("lists up to three names and counts the rest", () => {
    expect(unevaluatedText(["旅行"])).toBe("新しい振り分け先「旅行」が未評価です");
    expect(unevaluatedText(["A", "B"])).toBe("新しい振り分け先「A」「B」が未評価です");
    expect(unevaluatedText(["A", "B", "C", "D", "E"])).toBe("新しい振り分け先「A」「B」「C」ほか 2 件が未評価です");
  });
});

describe("noticeText", () => {
  it("only reports unavailable and failed states", () => {
    expect(noticeText(view())).toBeNull();
    expect(noticeText(view({ state: "off" }))).toBeNull();
    expect(noticeText(view({ state: "unavailable", message: "同意が必要" }))).toBe("AI 候補は使えません：同意が必要");
    expect(noticeText(view({ state: "failed", message: "x" }))).toBe("AI 候補を取得できませんでした：x");
  });
});

describe("candidateLevels", () => {
  it("maps target index to its level for highlighting the key list", () => {
    const m = candidateLevels(view({ cards: [card(2, 0.9, "high"), card(0, 0.5, "mid")] }));
    expect(m.get(2)).toBe("high");
    expect(m.get(0)).toBe("mid");
    expect(m.has(1)).toBe(false);
  });
  it("is empty unless the state is ready", () => {
    expect(candidateLevels(view({ state: "failed", message: "x", cards: [card(0, 0.9, "high")] })).size).toBe(0);
    expect(candidateLevels(null).size).toBe(0);
  });
});

describe("settings input parsing", () => {
  it("clamps thresholds to 0..1 and keeps the old value for garbage", () => {
    expect(parseUnit("0.8", 0.5)).toBe(0.8);
    expect(parseUnit("1.7", 0.5)).toBe(1);
    expect(parseUnit("-3", 0.5)).toBe(0);
    expect(parseUnit("", 0.5)).toBe(0.5);
    expect(parseUnit("abc", 0.5)).toBe(0.5);
  });
  it("clamps integers to a range", () => {
    expect(parseIntIn("3", 1, 10, 5)).toBe(3);
    expect(parseIntIn("0", 1, 10, 5)).toBe(1);
    expect(parseIntIn("99", 1, 10, 5)).toBe(10);
    expect(parseIntIn("2.6", 1, 10, 5)).toBe(3);
    expect(parseIntIn("x", 1, 10, 5)).toBe(5);
    expect(parseIntIn("", 1, 10, 5)).toBe(5);
  });
});

describe("consent", () => {
  it("shows the host the images will be sent to", () => {
    expect(endpointHost("https://api.cloudflare.com/client/v4/accounts/x/ai/run/y")).toBe("api.cloudflare.com");
    expect(endpointHost("not a url")).toBe("not a url");
  });
  it("states what is and is not sent, and how to opt out", () => {
    const m = consentMessage("https://api.cloudflare.com/x");
    expect(m).toContain("api.cloudflare.com");
    expect(m).toContain("縮小");
    expect(m).toContain("EXIF");
    expect(m).toContain("外部に送らない");
    expect(m).toContain("取り消せます");
    expect(m.trim().endsWith("同意しますか？")).toBe(true);
  });
});
