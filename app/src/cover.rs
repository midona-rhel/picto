//! The library's cover: the image the library folder keeps beside its
//! database, cropped to the square the user framed.
use std::path::{Path, PathBuf};

/// Holds the frame stored in `.picto-library-cover.json`: the point of the
/// image the square is centred on, in thousandths of the image's width and
/// height, and the zoom in percent.
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

/// Returns the square of the source image that the cover shows, as
/// `(x, y, side)` in source pixels. The image is scaled so that it covers
/// the square and then enlarged by the zoom. It is then moved so that the
/// focus point is as near the square's centre as it can be while the image
/// still covers the whole square.
pub fn square(width: f32, height: f32, crop: Crop) -> (f32, f32, f32) {
    let aspect = width / height;
    let zoom = (crop.zoom / 100.).max(1.);
    // The width and height of the displayed image, in units of the square's
    // side.
    let across = aspect.max(1.) * zoom;
    let down = (1. / aspect).max(1.) * zoom;
    let centre_x = 0.5 + (0.5 - crop.focus_x / 1000.) * (across - 1.);
    let centre_y = 0.5 + (0.5 - crop.focus_y / 1000.) * (down - 1.);
    // The square's top-left corner within the displayed image, converted to
    // source pixels.
    let scale = width / across;
    let side = scale.min(width).min(height);
    let x = ((across / 2. - centre_x) * scale).clamp(0., width - side);
    let y = ((down / 2. - centre_y) * scale).clamp(0., height - side);
    (x, y, side)
}

/// Writes the cropped cover into the folder `scratch` and returns its path.
/// Returns the path of the uncropped cover when the cover cannot be decoded
/// or the cropped one cannot be written, and `None` when the library has no
/// cover.
pub fn prepare(library: &Path, scratch: &Path) -> Option<PathBuf> {
    prepare_into(library, &scratch.join("library-cover.png"))
}

/// Does the same as `prepare`, but writes the cropped cover to the file
/// `target`. It writes only `target` and only reads the library's folder.
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
        // A zoom of 200 percent shows a square of half the side, still
        // centred.
        assert_eq!(square(400., 400., crop), (100., 100., 200.));
        // A focus at the left edge moves the square to the image's left
        // edge and not past it.
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
