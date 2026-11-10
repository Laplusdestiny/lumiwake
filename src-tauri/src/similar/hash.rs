//! dHash（差分ハッシュ）: 画像を小さく縮め、隣り合う画素の明暗の向きを 64 ビットにする。
//! 縮小・再圧縮・小さなマークの有無ではほとんど変わらない。

use image::imageops::FilterType;
use image::DynamicImage;

/// ハッシュのビット数
pub const BITS: u32 = 64;
/// 縮めたあとの大きさ。各行で隣どうしを比べるので、幅は 1 つ多い
const COLS: u32 = 9;
const ROWS: u32 = 8;

/// 画像の 64 ビットのハッシュ
pub fn dhash(img: &DynamicImage) -> u64 {
    let small = image::imageops::resize(&img.to_luma8(), COLS, ROWS, FilterType::Triangle);
    let mut hash = 0u64;
    for y in 0..ROWS {
        for x in 0..COLS - 1 {
            let brighter_to_the_right = small.get_pixel(x, y).0[0] < small.get_pixel(x + 1, y).0[0];
            hash = (hash << 1) | u64::from(brighter_to_the_right);
        }
    }
    hash
}

/// 2 つのハッシュで違うビットの数（0〜64）
pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::similar::{similarity_percent, DEFAULT_MAX_DISTANCE, MAX_DISTANCE_LIMIT};
    use image::imageops::FilterType;
    use image::{Rgb, RgbImage};

    /// 線形合同法の乱数（テスト用の絵を毎回同じにする）
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, max: u32) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as u32) % max.max(1)
        }
    }

    /// 写真のように大きな色の面と図形がある絵
    pub(crate) fn scene(seed: u64, w: u32, h: u32) -> RgbImage {
        let mut rng = Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03);
        let mut small = RgbImage::new(6, 4);
        for p in small.pixels_mut() {
            *p = Rgb([rng.next(256) as u8, rng.next(256) as u8, rng.next(256) as u8]);
        }
        let mut img = image::imageops::resize(&small, w, h, FilterType::CatmullRom);
        for _ in 0..12 {
            let (rw, rh) = (w / 12 + rng.next(w / 4), h / 12 + rng.next(h / 4));
            let (x0, y0) = (rng.next(w), rng.next(h));
            let color = Rgb([rng.next(256) as u8, rng.next(256) as u8, rng.next(256) as u8]);
            for y in y0..(y0 + rh).min(h) {
                for x in x0..(x0 + rw).min(w) {
                    img.put_pixel(x, y, color);
                }
            }
        }
        img
    }

    fn hash_of(img: &RgbImage) -> u64 {
        dhash(&DynamicImage::ImageRgb8(img.clone()))
    }

    fn jpeg_roundtrip(img: &RgbImage, quality: u8) -> RgbImage {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(img).unwrap();
        image::load_from_memory(&bytes).unwrap().to_rgb8()
    }

    #[test]
    fn identical_images_have_distance_zero() {
        let a = scene(1, 800, 600);
        assert_eq!(distance(hash_of(&a), hash_of(&a)), 0);
        assert_eq!(distance(0, u64::MAX), 64);
        assert_eq!(distance(0b1011, 0b0001), 2);
    }

    #[test]
    fn resized_and_recompressed_copies_stay_within_default() {
        for seed in 0..10 {
            let a = scene(seed, 800, 600);
            let b = jpeg_roundtrip(&image::imageops::resize(&a, 400, 300, FilterType::Triangle), 70);
            let d = distance(hash_of(&a), hash_of(&b));
            assert!(d <= DEFAULT_MAX_DISTANCE, "seed {seed}: 距離 {d}");
        }
    }

    #[test]
    fn a_corner_mark_stays_within_default() {
        for seed in 0..10 {
            let a = scene(seed, 800, 600);
            let mut b = a.clone();
            for y in 600 - 48 - 8..600 - 8 {
                for x in 800 - 160 - 8..800 - 8 {
                    b.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
            let d = distance(hash_of(&a), hash_of(&b));
            assert!(d <= DEFAULT_MAX_DISTANCE, "seed {seed}: 距離 {d}");
        }
    }

    #[test]
    fn different_images_are_far_apart() {
        let mut distances: Vec<u32> = (0..10)
            .map(|seed| distance(hash_of(&scene(seed, 800, 600)), hash_of(&scene(seed + 100, 800, 600))))
            .collect();
        eprintln!("別の絵どうしの距離: {distances:?}");
        // 既定の閾値では、別の絵は 1 組も類似にならない（余裕を持って 2 倍より遠い）
        assert!(distances.iter().all(|&d| d > DEFAULT_MAX_DISTANCE * 2), "{distances:?}");
        // いちばんゆるい設定でも、ほとんどの別の絵は類似にならない
        distances.sort_unstable();
        assert!(distances[1] > MAX_DISTANCE_LIMIT, "{distances:?}");
    }

    #[test]
    fn flat_and_tiny_images_do_not_panic() {
        let tiny = hash_of(&RgbImage::from_pixel(1, 1, Rgb([200, 10, 10])));
        let flat = hash_of(&RgbImage::from_pixel(100, 100, Rgb([30, 30, 30])));
        assert_eq!((tiny, flat), (0, 0), "明暗の差がなければ全ビット 0");
    }

    #[test]
    fn similarity_percent_rounds() {
        assert_eq!([0, 6, 16, 64].map(similarity_percent), [100, 91, 75, 0]);
        assert_eq!(similarity_percent(999), 0, "範囲外は 0%");
    }
}
