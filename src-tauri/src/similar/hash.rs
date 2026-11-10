//! dHash（差分ハッシュ）: 画像を小さく縮め、隣り合う画素の明暗の向きを 64 ビットにする。
//! 縮小・再圧縮・小さなマークの有無ではほとんど変わらない。

use image::DynamicImage;

/// ハッシュのビット数
pub const BITS: u32 = 64;
/// 縮めたあとの大きさ。各行で隣どうしを比べるので、幅は 1 つ多い
const COLS: u32 = 9;
const ROWS: u32 = 8;

/// 画像の 64 ビットのハッシュ
pub fn dhash(img: &DynamicImage) -> u64 {
    let cells = match img {
        DynamicImage::ImageRgb8(b) => cell_means(b.as_raw(), b.width(), b.height(), 3),
        DynamicImage::ImageRgba8(b) => cell_means(b.as_raw(), b.width(), b.height(), 4),
        DynamicImage::ImageLuma8(b) => cell_means(b.as_raw(), b.width(), b.height(), 1),
        DynamicImage::ImageLumaA8(b) => cell_means(b.as_raw(), b.width(), b.height(), 2),
        // 16 ビットなどは 8 ビットのグレースケールにしてから（めったにないので速さは求めない）
        other => {
            let b = other.to_luma8();
            cell_means(b.as_raw(), b.width(), b.height(), 1)
        }
    };
    let mut hash = 0u64;
    for row in &cells {
        for pair in row.windows(2) {
            let brighter_to_the_right = pair[0] < pair[1];
            hash = (hash << 1) | u64::from(brighter_to_the_right);
        }
    }
    hash
}

/// 明るさの重み（Rec. 709。`image` クレートのグレースケール化と同じ）。合計 10000
const WEIGHTS: [u64; 3] = [2126, 7152, 722];
const WEIGHT_SUM: u64 = 10000;

/// 画像を 9×8 のます目に分け、ますごとの明るさの平均（0〜255）を求める。
///
/// 原寸の画像をいったんグレースケールにして縮小フィルタをかけると、デコードより時間がかかる。
/// ここでは画素を 1 回なぞって足し込むだけにする。画素を間引かずすべて平均するので、
/// 細かい模様（網点など）のある画像でも、縮小した複製と同じ結果になる
fn cell_means(raw: &[u8], width: u32, height: u32, channels: usize) -> [[u8; COLS as usize]; ROWS as usize] {
    let (w, h) = (width as usize, height as usize);
    let mut means = [[0u8; COLS as usize]; ROWS as usize];
    if w == 0 || h == 0 || raw.len() < w * h * channels {
        return means;
    }
    let colored = channels >= 3;
    let xs = ranges(w, COLS as usize);
    let ys = ranges(h, ROWS as usize);
    for (r, &(y0, y1)) in ys.iter().enumerate() {
        let mut sums = [0u64; COLS as usize];
        for y in y0..y1 {
            let line = &raw[y * w * channels..(y + 1) * w * channels];
            for (c, &(x0, x1)) in xs.iter().enumerate() {
                let mut sum = 0u64;
                for px in line[x0 * channels..x1 * channels].chunks_exact(channels) {
                    sum += if colored {
                        WEIGHTS[0] * u64::from(px[0]) + WEIGHTS[1] * u64::from(px[1]) + WEIGHTS[2] * u64::from(px[2])
                    } else {
                        WEIGHT_SUM * u64::from(px[0])
                    };
                }
                sums[c] += sum;
            }
        }
        for (c, &(x0, x1)) in xs.iter().enumerate() {
            let count = ((x1 - x0) * (y1 - y0)) as u64 * WEIGHT_SUM;
            // 四捨五入して 8 ビットにする（圧縮のわずかな誤差で明暗の向きが変わらないように）
            means[r][c] = ((sums[c] + count / 2) / count) as u8;
        }
    }
    means
}

/// 長さ `len` を `parts` 個の区間に分ける。ますより小さい画像では、隣の区間と重ねて必ず 1 画素以上にする
fn ranges(len: usize, parts: usize) -> Vec<(usize, usize)> {
    (0..parts)
        .map(|i| {
            let start = (i * len / parts).min(len - 1);
            let end = ((i + 1) * len / parts).clamp(start + 1, len);
            (start, end)
        })
        .collect()
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
    fn fine_patterns_hash_like_their_downscaled_copy() {
        // 網点やスクリーントーンのような細かい模様。画素を間引いて読むと縮小版と違う絵になるので、
        // すべての画素を平均していることを確かめる
        for seed in 0..5 {
            let mut a = scene(seed, 900, 600);
            for (x, y, p) in a.enumerate_pixels_mut() {
                if (x + y) % 2 == 0 {
                    *p = Rgb([p.0[0] / 3, p.0[1] / 3, p.0[2] / 3]);
                }
            }
            let b = image::imageops::resize(&a, 300, 200, FilterType::Triangle);
            let d = distance(hash_of(&a), hash_of(&b));
            assert!(d <= DEFAULT_MAX_DISTANCE, "seed {seed}: 距離 {d}");
        }
    }

    #[test]
    fn pixel_formats_of_the_same_picture_hash_alike() {
        let rgb = DynamicImage::ImageRgb8(scene(4, 320, 240));
        let base = dhash(&rgb);
        let variants = [
            DynamicImage::ImageRgba8(rgb.to_rgba8()),
            DynamicImage::ImageLuma8(rgb.to_luma8()),
            DynamicImage::ImageLumaA8(rgb.to_luma_alpha8()),
            DynamicImage::ImageRgb16(rgb.to_rgb16()),
            DynamicImage::ImageRgba32F(rgb.to_rgba32f()),
        ];
        for v in variants {
            let d = distance(base, dhash(&v));
            assert!(d <= 2, "{:?}: 距離 {d}", v.color());
        }
    }

    #[test]
    fn images_narrower_than_the_grid_do_not_panic() {
        for (w, h) in [(1, 1), (3, 2), (9, 8), (8, 9), (2, 500), (500, 2)] {
            let img = scene(1, 600, 600);
            let small = image::imageops::resize(&img, w, h, FilterType::Triangle);
            let _ = hash_of(&small);
        }
    }

    #[test]
    fn similarity_percent_rounds() {
        assert_eq!([0, 6, 16, 64].map(similarity_percent), [100, 91, 75, 0]);
        assert_eq!(similarity_percent(999), 0, "範囲外は 0%");
    }
}
