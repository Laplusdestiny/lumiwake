//! HEIC/HEIF のデコーダ（libheif）。`heic` feature で有効になる。

use super::Decoder;
use crate::format::Format;
use image::{DynamicImage, RgbImage, RgbaImage};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};
use std::path::Path;

pub struct HeifDecoder;

impl Decoder for HeifDecoder {
    fn name(&self) -> &'static str {
        "libheif"
    }

    fn formats(&self) -> &'static [Format] {
        &[Format::Heic]
    }

    fn decode(&self, path: &Path) -> Result<DynamicImage, String> {
        let path = path.to_str().ok_or("ファイル名を扱えません")?;
        let lib = LibHeif::new();
        let ctx = HeifContext::read_from_file(path).map_err(|e| e.to_string())?;
        let handle = ctx.primary_image_handle().map_err(|e| e.to_string())?;
        let alpha = handle.has_alpha_channel();
        let chroma = if alpha { RgbChroma::Rgba } else { RgbChroma::Rgb };
        // libheif は既定で回転・反転（irot/imir）を反映してデコードする
        let img = lib.decode(&handle, ColorSpace::Rgb(chroma), None).map_err(|e| e.to_string())?;
        let planes = img.planes();
        let plane = planes.interleaved.ok_or("HEIC の画素データを取り出せません")?;
        let channels = if alpha { 4 } else { 3 };
        let (w, h) = (plane.width, plane.height);
        let row = w as usize * channels;
        let mut buf = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            let start = y * plane.stride;
            buf.extend_from_slice(&plane.data[start..start + row]);
        }
        let err = || "HEIC の画素データの大きさが合いません".to_string();
        Ok(if alpha {
            DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, buf).ok_or_else(err)?)
        } else {
            DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, buf).ok_or_else(err)?)
        })
    }
}
