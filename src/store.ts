// 画面の状態。Rust 側が正本で、ここには表示に必要な写しだけを置く。
import type { ConfigPayload, ImageInfo, SessionView, Suggestions } from "./api";

export type Screen = "start" | "sort" | "settings";

export const store = {
  config: null as ConfigPayload | null,
  session: null as SessionView | null,
  /** 現在の画像の詳細（大きさ・撮影日時）。画像番号と組で持つ */
  info: null as { generation: number; index: number; info: ImageInfo | null } | null,
  /** 現在の画像の AI 候補。画像番号と組で持つ。view が null の間は診断中 */
  suggest: null as { generation: number; index: number; view: Suggestions | null; loading: boolean } | null,
  screen: "start" as Screen,
  home: null as string | null,
  /** アプリのバージョン（開始画面に表示する） */
  version: null as string | null,
  /** モーダル（終了確認など）を表示中。表示中は仕分けのキーを受け付けない */
  modal: false,
};

type Listener = () => void;
const listeners: Listener[] = [];

export function onChange(fn: Listener): void {
  listeners.push(fn);
}

export function notify(): void {
  for (const fn of listeners) fn();
}
