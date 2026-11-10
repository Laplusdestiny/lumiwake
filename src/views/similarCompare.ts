// 類似画像の比較表示の計算（どの画像を並べるか、拡大の倍率と位置）。DOM には触れない

/** 並べて表示する最大の枚数 */
export const COMPARE_PANES = 3;
export const ZOOM_MAX = 8;
/** 1 段階で何倍にするか */
const ZOOM_STEP = 1.5;

/** 拡大の状態。x / y は枠の大きさに対する割合（並べた画像すべてに同じ値を使う） */
export interface Zoom {
  scale: number;
  x: number;
  y: number;
}

export const NO_ZOOM: Zoom = { scale: 1, x: 0, y: 0 };

/** `count` 枚のうち、`selected` 枚目（0 始まり）を中心に並べる範囲 [start, end) */
export function compareWindow(count: number, selected: number): [number, number] {
  const size = Math.min(COMPARE_PANES, count);
  const start = Math.min(Math.max(0, selected - Math.floor(size / 2)), count - size);
  return [start, start + size];
}

/** 1 段階拡大（dir > 0）・縮小した倍率。等倍と上限で止まる */
export function stepZoom(scale: number, dir: 1 | -1): number {
  const next = dir > 0 ? scale * ZOOM_STEP : scale / ZOOM_STEP;
  // 縮小を繰り返したときに 1.0000001 のような端数が残らないようにする
  const rounded = Math.round(next * 1000) / 1000;
  return Math.min(ZOOM_MAX, Math.max(1, rounded));
}

/** 画像が枠の外へ出ない範囲に、ずらす量（枠に対する割合）を丸める */
export function clampPan(offset: number, scale: number): number {
  const limit = (scale - 1) / 2;
  return Math.min(limit, Math.max(-limit, offset));
}

export function zoomTransform(z: Zoom): string {
  const pct = (v: number) => `${Math.round(v * 1000) / 10}%`;
  return `translate(${pct(z.x)}, ${pct(z.y)}) scale(${z.scale})`;
}
