//! 画像形式の判定。スキャナとデコーダが共有する。

use serde::Serialize;
use std::path::Path;

/// Lumiwake が扱う画像形式。RAW・動画は対象外。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Avif,
    Heic,
}

impl Format {
    pub const ALL: [Format; 8] =
        [Format::Jpeg, Format::Png, Format::Webp, Format::Gif, Format::Bmp, Format::Tiff, Format::Avif, Format::Heic];

    /// 拡張子から形式を判定する（大文字小文字は区別しない）。
    pub fn from_extension(ext: &str) -> Option<Format> {
        let ext = ext.to_ascii_lowercase();
        Some(match ext.as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => Format::Jpeg,
            "png" => Format::Png,
            "webp" => Format::Webp,
            "gif" => Format::Gif,
            "bmp" | "dib" => Format::Bmp,
            "tif" | "tiff" => Format::Tiff,
            "avif" => Format::Avif,
            "heic" | "heif" | "hif" => Format::Heic,
            _ => return None,
        })
    }

    pub fn from_path(path: &Path) -> Option<Format> {
        path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension)
    }

    /// 画面表示用の名前
    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Webp => "WebP",
            Format::Gif => "GIF",
            Format::Bmp => "BMP",
            Format::Tiff => "TIFF",
            Format::Avif => "AVIF",
            Format::Heic => "HEIC",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_extensions_case_insensitively() {
        assert_eq!(Format::from_path(Path::new("a/IMG_1.JPG")), Some(Format::Jpeg));
        assert_eq!(Format::from_path(Path::new("b.jpeg")), Some(Format::Jpeg));
        assert_eq!(Format::from_path(Path::new("c.HeIc")), Some(Format::Heic));
        assert_eq!(Format::from_path(Path::new("d.tif")), Some(Format::Tiff));
        assert_eq!(Format::from_path(Path::new("e.avif")), Some(Format::Avif));
    }

    #[test]
    fn ignores_non_images_and_raw() {
        assert_eq!(Format::from_path(Path::new("notes.txt")), None);
        assert_eq!(Format::from_path(Path::new("IMG_1.CR2")), None);
        assert_eq!(Format::from_path(Path::new("movie.mp4")), None);
        assert_eq!(Format::from_path(Path::new("noext")), None);
    }
}
