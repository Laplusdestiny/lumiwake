import { describe, expect, it } from "vitest";
import { ZOOM_MAX, clampPan, compareWindow, stepZoom, zoomTransform } from "./similarCompare";

describe("compareWindow", () => {
  it("選択中の画像を中心に、最大 3 枚の範囲を返す", () => {
    expect(compareWindow(2, 0)).toEqual([0, 2]);
    expect(compareWindow(2, 1)).toEqual([0, 2]);
    expect(compareWindow(3, 1)).toEqual([0, 3]);
    expect(compareWindow(5, 0)).toEqual([0, 3]);
    expect(compareWindow(5, 2)).toEqual([1, 4]);
    expect(compareWindow(5, 4)).toEqual([2, 5]);
    expect(compareWindow(1, 0)).toEqual([0, 1]);
  });
});

describe("ズーム", () => {
  it("1 段階ずつ拡大・縮小し、等倍と上限で止まる", () => {
    expect(stepZoom(1, 1)).toBe(1.5);
    expect(stepZoom(1.5, 1)).toBe(2.25);
    expect(stepZoom(1.5, -1)).toBe(1);
    expect(stepZoom(1, -1)).toBe(1);
    expect(stepZoom(ZOOM_MAX, 1)).toBe(ZOOM_MAX);
    expect(stepZoom(7, 1)).toBe(ZOOM_MAX);
  });

  it("画像が枠の外へ出ない範囲に、ずらす量を丸める", () => {
    expect(clampPan(0.3, 1)).toBe(0); // 等倍では動かせない
    expect(clampPan(0.3, 2)).toBe(0.3);
    expect(clampPan(0.9, 2)).toBe(0.5);
    expect(clampPan(-0.9, 2)).toBe(-0.5);
    expect(clampPan(2, 4)).toBe(1.5);
  });

  it("倍率とずらす量を CSS の transform にする", () => {
    expect(zoomTransform({ scale: 1, x: 0, y: 0 })).toBe("translate(0%, 0%) scale(1)");
    expect(zoomTransform({ scale: 2, x: 0.25, y: -0.5 })).toBe("translate(25%, -50%) scale(2)");
  });
});
