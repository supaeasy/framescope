//! Frame-Export als PNG in voller Originalauflösung.

use anyhow::{anyhow, Context as _, Result};
use std::path::Path;

/// Dateiname `<videoname>_frame<nummer>.png` (ohne Zeichen, die Windows nicht erlaubt).
pub fn file_name(video: &Path, frame: usize) -> String {
    let stem = video
        .file_stem()
        .map_or_else(|| "video".into(), |s| s.to_string_lossy().into_owned());
    let clean: String = stem
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("{clean}_frame{frame}.png")
}

/// Speichert dicht gepackte RGBA-Daten als PNG.
pub fn save_png(path: &Path, width: usize, height: usize, rgba: Vec<u8>) -> Result<()> {
    let (w, h) = (u32::try_from(width)?, u32::try_from(height)?);
    let image =
        image::RgbaImage::from_raw(w, h, rgba).ok_or_else(|| anyhow!("Ungültige Bilddaten"))?;
    image
        .save_with_format(path, image::ImageFormat::Png)
        .with_context(|| format!("PNG konnte nicht gespeichert werden: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn names() {
        assert_eq!(
            file_name(&PathBuf::from("C:\\v\\Urlaub 2024.mp4"), 42),
            "Urlaub 2024_frame42.png"
        );
        assert_eq!(file_name(&PathBuf::from("a|b.mkv"), 0), "a_b_frame0.png");
    }

    #[test]
    fn png_roundtrip() {
        let dir = std::env::temp_dir().join("framescope_test_png");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| i as u8).collect();
        save_png(&path, 4, 3, rgba.clone()).unwrap();
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!((back.width(), back.height()), (4, 3));
        assert_eq!(back.into_raw(), rgba);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(save_png(&std::env::temp_dir().join("x.png"), 4, 4, vec![0; 10]).is_err());
    }
}
