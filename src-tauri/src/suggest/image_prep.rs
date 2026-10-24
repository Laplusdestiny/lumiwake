//! 外部 API へ送る前の画像の縮小・再圧縮。
//!
//! 元の画像（サイズ・形式・メタデータ）をそのまま送らない。長辺を縮め、JPEG に再圧縮し、
//! 上限（`max_image_kb`）に収まるまで段階的に小さくする。EXIF など元のメタデータは含まれない。

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, RgbImage};

/// 試す長辺の大きさ（大きい順）。元の画像より大きくはしない
const SIDES: [u32; 6] = [1280, 1024, 768, 512, 384, 256];
/// 試す JPEG の品質（高い順）
const QUALITIES: [u8; 4] = [85, 75, 65, 50];

/// 透明部分は白で塗りつぶして RGB にする（JPEG は透過を持てないため）
fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = u32::from(p[3]);
        let blend = |c: u8| ((u32::from(c) * a + 255 * (255 - a)) / 255) as u8;
        out.put_pixel(x, y, image::Rgb([blend(p[0]), blend(p[1]), blend(p[2])]));
    }
    out
}

fn encode(img: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, quality).encode_image(img).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// `max_kb` に収まる JPEG を作る。最小の設定でも収まらなければエラー（送らない）
pub fn prepare_jpeg(img: &DynamicImage, max_kb: u32) -> Result<Vec<u8>, String> {
    let max_bytes = max_kb as usize * 1024;
    let rgb = flatten(img);
    let longest = rgb.width().max(rgb.height());
    let mut last_len = 0;
    for side in SIDES {
        let resized;
        let candidate = if longest > side {
            resized = DynamicImage::ImageRgb8(rgb.clone()).resize(side, side, FilterType::Triangle).to_rgb8();
            &resized
        } else {
            &rgb
        };
        for q in QUALITIES {
            let bytes = encode(candidate, q)?;
            if bytes.len() <= max_bytes {
                return Ok(bytes);
            }
            last_len = bytes.len();
        }
    }
    Err(format!("画像を {max_kb} KB 以下に縮小できませんでした（最小設定でも {} KB）", last_len.div_ceil(1024)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, ImageFormat, Rgba, RgbaImage};

    /// 圧縮しにくい（ノイズの多い）画像
    fn noisy(w: u32, h: u32) -> DynamicImage {
        let mut state: u32 = 12345;
        let img = RgbImage::from_fn(w, h, |_, _| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            image::Rgb([(state >> 8) as u8, (state >> 16) as u8, (state >> 24) as u8])
        });
        DynamicImage::ImageRgb8(img)
    }

    fn decoded_size(bytes: &[u8]) -> (u32, u32) {
        assert_eq!(image::guess_format(bytes).unwrap(), ImageFormat::Jpeg);
        image::load_from_memory(bytes).unwrap().dimensions()
    }

    #[test]
    fn large_images_are_shrunk_to_fit_the_limit() {
        let bytes = prepare_jpeg(&noisy(1600, 1100), 190).unwrap();
        assert!(bytes.len() <= 190 * 1024, "{} bytes", bytes.len());
        let (w, h) = decoded_size(&bytes);
        assert!(w.max(h) <= 1280, "長辺は縮める: {w}x{h}");
        assert!(w > h, "縦横比は保つ");
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let bytes = prepare_jpeg(&noisy(120, 80), 190).unwrap();
        assert_eq!(decoded_size(&bytes), (120, 80));
    }

    #[test]
    fn output_is_always_jpeg_even_for_png_like_input_with_alpha() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0])));
        let bytes = prepare_jpeg(&img, 190).unwrap();
        let back = image::load_from_memory(&bytes).unwrap().to_rgb8();
        let p = back.get_pixel(32, 32);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "透明部分は白になる: {p:?}");
    }

    #[test]
    fn a_tight_limit_forces_more_shrinking() {
        let img = noisy(800, 800);
        let loose = prepare_jpeg(&img, 150).unwrap();
        let tight = prepare_jpeg(&img, 30).unwrap();
        assert!(tight.len() <= 30 * 1024 && tight.len() < loose.len());
    }

    #[test]
    fn an_impossible_limit_fails_instead_of_sending_something_larger() {
        let err = prepare_jpeg(&noisy(300, 300), 1).unwrap_err();
        assert!(err.contains("縮小できません"), "{err}");
    }

    #[test]
    fn original_metadata_is_not_carried_over() {
        // 再エンコードしたデータには EXIF（APP1）が含まれない
        let bytes = prepare_jpeg(&noisy(200, 200), 190).unwrap();
        assert!(!bytes.windows(4).any(|w| w == b"Exif"));
    }
}
