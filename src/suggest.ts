// AI 候補の表示用ロジック（画面から切り離して、表記のルールをテストできるようにする）
import type { Ai, SuggestionLevel, Suggestions } from "./api";

function clamp01(x: number): number {
  return Math.min(1, Math.max(0, x));
}

/** 確率は「72%」。一致度は確率ではないので % を付けず、別途「一致度」と表記する */
export function scoreText(score: number, kind: Suggestions["kind"]): string {
  const n = Math.round(clamp01(score) * 100);
  return kind === "probability" ? `${n}%` : String(n);
}

export function scoreLabel(kind: Suggestions["kind"]): string {
  return kind === "match" ? "一致度" : "";
}

export function barPercent(score: number): number {
  return Math.round(clamp01(score) * 100);
}

/** ヘッダーのバッジ。AI を使わないなら出さない。外部送信するときは必ずそれとわかる文言にする */
export function badgeText(ai: Ai): string | null {
  switch (ai.backend) {
    case "off":
      return null;
    case "local":
      return "AI：ローカル";
    case "systemone":
      return ai.systemone.external_consent ? "AI：System One · 外部送信" : "AI：System One（外部送信への同意待ち）";
  }
}

/** 再診断の案内の文言。未評価の振り分け先がなければ null */
export function unevaluatedText(names: string[]): string | null {
  if (names.length === 0) return null;
  const shown = names.slice(0, 3).map((n) => `「${n}」`).join("");
  const rest = names.length > 3 ? `ほか ${names.length - 3} 件` : "";
  return `新しい振り分け先${shown}${rest}が未評価です`;
}

/** 使えない・失敗したときの案内。それ以外は null */
export function noticeText(v: Suggestions | null): string | null {
  if (!v) return null;
  if (v.state === "unavailable") return `AI 候補は使えません：${v.message}`;
  if (v.state === "failed") return `AI 候補を取得できませんでした：${v.message}`;
  return null;
}

/** 振り分け先の番号 → 候補としての強さ（キー割り当て一覧の強調表示用） */
export function candidateLevels(v: Suggestions | null): Map<number, SuggestionLevel> {
  const m = new Map<number, SuggestionLevel>();
  if (v?.state === "ready") for (const c of v.cards) m.set(c.target, c.level);
  return m;
}
