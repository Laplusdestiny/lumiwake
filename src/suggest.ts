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

/** しきい値の入力値を 0〜1 に収める。数値でなければ元の値のまま */
export function parseUnit(value: string, fallback: number): number {
  if (value.trim() === "") return fallback;
  const n = Number(value);
  return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : fallback;
}

/** 整数の入力値を範囲に収める。数値でなければ元の値のまま */
export function parseIntIn(value: string, min: number, max: number, fallback: number): number {
  if (value.trim() === "") return fallback;
  const n = Math.round(Number(value));
  return Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : fallback;
}

/** 送信先のホスト名（同意の確認文に出す）。解釈できなければ入力そのまま */
export function endpointHost(endpoint: string): string {
  try {
    return new URL(endpoint).hostname || endpoint;
  } catch {
    return endpoint;
  }
}

/** 画像を外部へ送ることへの同意の確認文 */
export function consentMessage(endpoint: string): string {
  return [
    `System One を使うと、表示中の画像と、先読みする数枚先の画像を、縮小・再圧縮して ${endpointHost(endpoint)} へ送信します。`,
    "・元のファイルや撮影情報（EXIF）は送りません。",
    "・振り分け先のフォルダ名と説明文は、選択肢として一緒に送ります（パスは送りません）。",
    "・送りたくないフォルダは、振り分け先ごとの設定で「外部に送らない」にできます。",
    "・同意は、設定でいつでも取り消せます。",
    "",
    "画像を外部へ送信することに同意しますか？",
  ].join("\n");
}
