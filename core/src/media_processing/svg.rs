//! SVG file handling.
//!
//! Ported from `hydrus/client/ClientSVGHandling.py` which uses Qt SVG rendering.
//! We use the `resvg` crate for SVG parsing and rendering instead of Qt.

use std::path::Path;

use super::{FileError, FileResult};

/// Get the resolution (width, height) of an SVG file.
///
/// Ported from `ClientSVGHandling.GetSVGResolution()`.
/// Python uses `QSvgRenderer.defaultSize()`. We parse the SVG using `resvg`'s `usvg` tree
/// to get the default size from the viewBox/width/height attributes.
pub fn get_svg_resolution(path: &Path) -> FileResult<(u32, u32)> {
    let svg_data = std::fs::read(path).map_err(FileError::Io)?;

    let opt = usvg::Options::default();

    let tree = usvg::Tree::from_data(&svg_data, &opt)
        .map_err(|e| FileError::UnsupportedFile(format!("Could not parse SVG file: {}", e)))?;

    let size = tree.size();
    let width = size.width() as u32;
    let height = size.height() as u32;

    // Python: defaultSize() can return 0x0 for some SVGs, which is handled upstream
    Ok((width.max(1), height.max(1)))
}

/// Generate a thumbnail PNG from an SVG file.
///
/// Ported from `ClientSVGHandling.GenerateThumbnailNumPyFromSVGPath()`.
/// Python renders via `QSvgRenderer` into a `QImage`. We use `resvg` to render to a pixmap
/// and then encode as PNG.
pub fn generate_thumbnail_from_svg(
    path: &Path,
    target_resolution: (u32, u32),
) -> FileResult<Vec<u8>> {
    let svg_data = std::fs::read(path).map_err(FileError::Io)?;

    let opt = usvg::Options::default();

    let tree = usvg::Tree::from_data(&svg_data, &opt)
        .map_err(|e| FileError::Thumbnail(format!("Could not parse SVG file: {}", e)))?;

    let (target_width, target_height) = target_resolution;

    // Python: renders at target_resolution with aspect ratio preserved, transparent background
    let mut pixmap = tiny_skia::Pixmap::new(target_width, target_height).ok_or_else(|| {
        FileError::Thumbnail("Failed to create pixmap for SVG rendering".to_string())
    })?;

    // Calculate scale to fit within target resolution while maintaining aspect ratio
    // (matches Python's KeepAspectRatio behavior)
    let svg_size = tree.size();
    let scale_x = target_width as f32 / svg_size.width();
    let scale_y = target_height as f32 / svg_size.height();
    let scale = scale_x.min(scale_y);

    let transform = tiny_skia::Transform::from_scale(scale, scale);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let png_data = pixmap.encode_png().map_err(|e| {
        FileError::Thumbnail(format!("Failed to encode SVG thumbnail as PNG: {}", e))
    })?;

    Ok(png_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_svg_resolution_with_explicit_dimensions() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"></svg>"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), svg).unwrap();

        let (w, h) = get_svg_resolution(tmp.path()).unwrap();
        assert_eq!(w, 200);
        assert_eq!(h, 100);
    }

    #[test]
    fn test_get_svg_resolution_with_viewbox() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 150"></svg>"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), svg).unwrap();

        let (w, h) = get_svg_resolution(tmp.path()).unwrap();
        assert_eq!(w, 300);
        assert_eq!(h, 150);
    }

    #[test]
    fn test_generate_thumbnail_from_svg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect width="200" height="100" fill="red"/>
        </svg>"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), svg).unwrap();

        let png = generate_thumbnail_from_svg(tmp.path(), (100, 50)).unwrap();
        assert!(!png.is_empty());
        // PNG magic bytes
        assert_eq!(&png[..4], &[0x89, 0x50, 0x4E, 0x47]);
    }
}
