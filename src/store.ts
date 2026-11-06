// 画面の状態。Rust 側が正本で、ここには表示に必要な写しだけを置く。
import type { ConfigPayload, GridList, ImageInfo, SessionView } from "./api";

export type Screen = "start" | "sort" | "settings" | "grid";

export const store = {
  config: null as ConfigPayload | null,
  session: null as SessionView | null,
  /** 現在の画像の詳細（大きさ・撮影日時）。画像番号と組で持つ */
  info: null as { generation: number; index: number; info: ImageInfo | null } | null,
  screen: "start" as Screen,
  /** 一覧表示中の画像の一覧（開いたときに取得した写し） */
  grid: null as GridList | null,
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
