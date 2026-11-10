//! 類似画像の判定。計算だけを行い、ファイル操作はしない（削除は `fileops` が持つ）。

pub mod group;
pub mod hash;
pub mod names;

/// 既定の閾値（ハッシュの違うビット数がこれ以下なら類似とみなす）
pub const DEFAULT_MAX_DISTANCE: u32 = 6;
/// 画面からゆるめられる上限
pub const MAX_DISTANCE_LIMIT: u32 = 16;

/// 距離を類似度（%）にする
pub fn similarity_percent(distance: u32) -> u8 {
    let d = distance.min(hash::BITS) as f64;
    ((1.0 - d / hash::BITS as f64) * 100.0).round() as u8
}
