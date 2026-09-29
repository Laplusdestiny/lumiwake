//! デコーダ: 画像を読み込み、表示用のサイズに縮小してエンコードし直す。
//!
//! 形式ごとにデコーダを追加できるよう [`Decoder`] トレイトで抽象化している。
//! HEIC・AVIF は C ライブラリに依存するため Cargo の feature（`heic` / `avif`）で有効にする。
//! 有効でない形式はスキャナが「未対応」として分け、読み込み時に表示から外す。

mod cache;
#[cfg(feature = "heic")]
mod heif;

pub use cache::{PrefetchJob, Prefetcher, PreviewCache};

use crate::format::Format;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader};
use serde::Serialize;
use std::io::Cursor;
use std::path::Path;

/// 表示用の最大辺（px）。これより大きい画像は縮小して WebView に渡す
pub const PREVIEW_MAX: u32 = 2560;
/// フィルムストリップのサムネイルの最大辺（px）
pub const THUMB_MAX: u32 = 240;

pub trait Decoder: Send + Sync {
    fn name(&self) -> &'static str;
    fn formats(&self) -> &'static [Format];
    /// 向き（EXIF Orientation など）を反映した画像を返す
    fn decode(&self, path: &Path) -> Result<DynamicImage, String>;
}

/// `image` クレートによるデコーダ
struct ImageCrateDecoder;

impl Decoder for ImageCrateDecoder {
    fn name(&self) -> &'static str {
        "image"
    }

    fn formats(&self) -> &'static [Format] {
        #[cfg(feature = "avif")]
        const F: &[Format] =
            &[Format::Jpeg, Format::Png, Format::Webp, Format::Gif, Format::Bmp, Format::Tiff, Format::Avif];
        #[cfg(not(feature = "avif"))]
        const F: &[Format] = &[Format::Jpeg, Format::Png, Format::Webp, Format::Gif, Format::Bmp, Format::Tiff];
        F
    }

    fn decode(&self, path: &Path) -> Result<DynamicImage, String> {
        // 拡張子と中身が食い違うファイルもあるので、中身から形式を推定する
        let reader =
            ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
        let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
        let orientation = decoder.orientation().map_err(|e| e.to_string())?;
        let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
        img.apply_orientation(orientation);
        Ok(img)
    }
}

/// 形式ごとの対応状況（設定画面の表示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatSupport {
    pub format: Format,
    pub label: &'static str,
    pub supported: bool,
    pub decoder: Option<&'static str>,
}

pub struct Registry {
    decoders: Vec<Box<dyn Decoder>>,
}

impl Registry {
    /// ビルド時に有効にした形式のデコーダをすべて登録する
    pub fn builtin() -> Self {
        #[cfg_attr(not(feature = "heic"), allow(unused_mut))]
        let mut decoders: Vec<Box<dyn Decoder>> = vec![Box::new(ImageCrateDecoder)];
        #[cfg(feature = "heic")]
        decoders.push(Box::new(heif::HeifDecoder));
        Registry { decoders }
    }

    fn find(&self, format: Format) -> Option<&dyn Decoder> {
        self.decoders.iter().find(|d| d.formats().contains(&format)).map(|d| d.as_ref())
    }

    pub fn supports(&self, format: Format) -> bool {
        self.find(format).is_some()
    }

    pub fn format_support(&self) -> Vec<FormatSupport> {
        Format::ALL
            .iter()
            .map(|&format| {
                let decoder = self.find(format).map(|d| d.name());
                FormatSupport { format, label: format.label(), supported: decoder.is_some(), decoder }
            })
            .collect()
    }

    pub fn decode(&self, path: &Path) -> Result<DynamicImage, String> {
        let format = Format::from_path(path).ok_or_else(|| "画像ファイルではありません".to_string())?;
        let decoder = self.find(format).ok_or_else(|| format!("{} の表示には対応していません", format.label()))?;
        decoder.decode(path)
    }
}

/// WebView に渡すエンコード済みの画像
#[derive(Debug, Clone)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// 表示用に変換した画像と、元画像の情報
#[derive(Debug, Clone)]
pub struct Preview {
    pub full: Encoded,
    pub thumb: Encoded,
    /// 元画像の幅・高さ（向きを反映後）
    pub width: u32,
    pub height: u32,
}

/// 画像を読み込み、表示用とサムネイル用に縮小・エンコードする
pub fn make_preview(registry: &Registry, path: &Path) -> Result<Preview, String> {
    let img = registry.decode(path)?;
    let (width, height) = (img.width(), img.height());
    let full = fit(img, PREVIEW_MAX);
    let thumb = fit(full.clone(), THUMB_MAX);
    Ok(Preview { full: encode(&full)?, thumb: encode(&thumb)?, width, height })
}

fn fit(img: DynamicImage, max: u32) -> DynamicImage {
    if img.width() <= max && img.height() <= max {
        return img;
    }
    img.resize(max, max, FilterType::Triangle)
}

/// 透過がなければ JPEG、あれば PNG にする
pub fn encode(img: &DynamicImage) -> Result<Encoded, String> {
    let mut out = Cursor::new(Vec::new());
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive)
            .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        Ok(Encoded { bytes: out.into_inner(), mime: "image/png" })
    } else {
        let rgb = img.to_rgb8();
        JpegEncoder::new_with_quality(&mut out, 90)
            .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
            .map_err(|e| e.to_string())?;
        Ok(Encoded { bytes: out.into_inner(), mime: "image/jpeg" })
    }
}

/// EXIF の撮影日時（`2026-08-14 10:22` 形式）。読めなければ `None`
pub fn taken_at(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let exif = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)).ok()?;
    let field = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let exif::Value::Ascii(ref v) = field.value else { return None };
    let dt = exif::DateTime::from_ascii(v.first()?).ok()?;
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}", dt.year, dt.month, dt.day, dt.hour, dt.minute))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    /// テスト用のダミー画像を書き出す（実在の写真は使わない）
    pub fn write_dummy(path: &Path, w: u32, h: u32) {
        let img = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
        img.save(path).unwrap();
    }

    #[test]
    fn builtin_registry_supports_standard_formats() {
        let r = Registry::builtin();
        for f in [Format::Jpeg, Format::Png, Format::Webp, Format::Gif, Format::Bmp, Format::Tiff] {
            assert!(r.supports(f), "{f:?}");
        }
        assert_eq!(r.supports(Format::Avif), cfg!(feature = "avif"));
        assert_eq!(r.supports(Format::Heic), cfg!(feature = "heic"));
        assert_eq!(r.format_support().len(), Format::ALL.len());
    }

    #[test]
    fn preview_downscales_large_images() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.png");
        write_dummy(&p, 3000, 1500);
        let preview = make_preview(&Registry::builtin(), &p).unwrap();
        assert_eq!((preview.width, preview.height), (3000, 1500));
        assert_eq!(preview.full.mime, "image/jpeg");
        let full = image::load_from_memory(&preview.full.bytes).unwrap();
        assert_eq!((full.width(), full.height()), (2560, 1280));
        let thumb = image::load_from_memory(&preview.thumb.bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (240, 120));
    }

    #[test]
    fn keeps_transparency_as_png() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("alpha.png");
        RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 10])).save(&p).unwrap();
        let preview = make_preview(&Registry::builtin(), &p).unwrap();
        assert_eq!(preview.full.mime, "image/png");
    }

    #[test]
    fn decodes_by_content_even_with_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("real.png");
        write_dummy(&png, 8, 8);
        let jpg = dir.path().join("actually-png.jpg");
        std::fs::rename(&png, &jpg).unwrap();
        assert!(make_preview(&Registry::builtin(), &jpg).is_ok());
    }

    #[test]
    fn corrupt_files_are_errors_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("broken.jpg");
        std::fs::write(&p, b"not an image").unwrap();
        assert!(make_preview(&Registry::builtin(), &p).is_err());
        assert!(taken_at(&p).is_none());
    }

    #[cfg(any(feature = "heic", feature = "avif"))]
    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
    }

    #[cfg(feature = "heic")]
    #[test]
    fn decodes_heic() {
        let preview = make_preview(&Registry::builtin(), &fixture("sample.heic")).unwrap();
        assert_eq!((preview.width, preview.height), (64, 48));
    }

    #[cfg(feature = "avif")]
    #[test]
    fn decodes_avif() {
        let preview = make_preview(&Registry::builtin(), &fixture("sample.avif")).unwrap();
        assert_eq!((preview.width, preview.height), (64, 48));
    }

    #[cfg(not(feature = "heic"))]
    #[test]
    fn unsupported_format_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("photo.heic");
        std::fs::write(&p, b"....").unwrap();
        let err = make_preview(&Registry::builtin(), &p).unwrap_err();
        assert!(err.contains("HEIC"));
    }
}
