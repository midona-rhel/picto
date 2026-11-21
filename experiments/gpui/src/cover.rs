//! The library's cover: the image the library folder keeps beside its
//! database, cropped to the square the user framed.
use std::path::{Path, PathBuf};

/// The frame stored in `.picto-library-cover.json`: where the square is
/// centred, in thousandths of the image, and how far it is zoomed in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub focus_x: f32,
    pub focus_y: f32,
    pub zoom: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            focus_x: 500.,
            focus_y: 500.,
            zoom: 100.,
        }
    }
}

/// The square of the source image the cover shows, as `(x, y, side)` in
/// source pixels. `subscriptionCoverGeometry` lays the image out so that it
/// covers the square, enlarged by the zoom, and slides it so the focus point
/// moves toward the centre without uncovering an edge.
pub fn square(width: f32, height: f32, crop: Crop) -> (f32, f32, f32) {
    let aspect = width / height;
    let zoom = (crop.zoom / 100.).max(1.);
    // The displayed image, in units of the square's side.
    let across = aspect.max(1.) * zoom;
    let down = (1. / aspect).max(1.) * zoom;
    let centre_x = 0.5 + (0.5 - crop.focus_x / 1000.) * (across - 1.);
    let centre_y = 0.5 + (0.5 - crop.focus_y / 1000.) * (down - 1.);
    // The square's top-left corner within the displayed image, then to
    // source pixels.
    let scale = width / across;
    let side = scale.min(width).min(height);
    let x = ((across / 2. - centre_x) * scale).clamp(0., width - side);
    let y = ((down / 2. - centre_y) * scale).clamp(0., height - side);
    (x, y, side)
}

/// Writes the cropped cover into `scratch` and returns its path, or the
/// uncropped cover's when it has no frame or cannot be read.
pub fn prepare(library: &Path, scratch: &Path) -> Option<PathBuf> {
    prepare_into(library, &scratch.join("library-cover.png"))
}

/// The same, writing the cropped cover to `target`. Only `target` is
/// written; the library's folder is only read.
pub fn prepare_into(library: &Path, target: &Path) -> Option<PathBuf> {
    let source = ["jpg", "png"]
        .into_iter()
        .map(|extension| library.join(format!(".picto-library-cover.{extension}")))
        .find(|path| path.is_file())?;
    let crop = std::fs::read(library.join(".picto-library-cover.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .map(|frame| {
            let number = |key: &str, fallback: f32| {
                frame
                    .get(key)
                    .and_then(serde_json::Value::as_f64)
                    .map_or(fallback, |value| value as f32)
            };
            Crop {
                focus_x: number("imageFocusX", 500.),
                focus_y: number("imageFocusY", 500.),
                zoom: number("imageZoomPercent", 100.),
            }
        })
        .unwrap_or_default();
    let cropped = || -> Option<PathBuf> {
        let image = image::open(&source).ok()?;
        let (x, y, side) = square(image.width() as f32, image.height() as f32, crop);
        let side = (side.round() as u32).max(1);
        let framed = image.crop_imm(x.round() as u32, y.round() as u32, side, side);
        framed.save(target).ok()?;
        Some(target.to_path_buf())
    };
    cropped().or(Some(source))
}

#[cfg(test)]
mod tests {
    use super::{Crop, square};

    #[test]
    fn an_unframed_cover_shows_the_centred_square() {
        assert_eq!(square(400., 200., Crop::default()), (100., 0., 200.));
        assert_eq!(square(200., 400., Crop::default()), (0., 100., 200.));
    }

    #[test]
    fn zoom_narrows_the_square_and_focus_slides_it() {
        let crop = Crop {
            focus_x: 500.,
            focus_y: 500.,
            zoom: 200.,
        };
        // Twice the zoom shows half the side, still centred.
        assert_eq!(square(400., 400., crop), (100., 100., 200.));
        // A focus at the left edge slides the square fully left, and no
        // further.
        let left = Crop {
            focus_x: 0.,
            ..crop
        };
        assert_eq!(square(400., 400., left), (0., 100., 200.));
        let corner = Crop {
            focus_x: 1000.,
            focus_y: 1000.,
            ..crop
        };
        assert_eq!(square(400., 400., corner), (200., 200., 200.));
    }
}
