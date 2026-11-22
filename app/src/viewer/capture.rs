//! The "Set as Thumbnail" command. It takes the picture that a video or a
//! Flash movie shows at that moment, encodes it as a PNG and writes it as
//! the file's thumbnail.
use gpui::RenderImage;
use std::path::Path;

/// The largest PNG, in bytes, that [`write_thumbnail`] writes.
const LARGEST: usize = 32 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// A captured picture as RGBA pixels, before it is encoded.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// Makes a picture from BGRA pixels, which is the order GPUI holds them
    /// in.
    pub fn from_bgra(width: u32, height: u32, bgra: &[u8]) -> Self {
        let mut rgba = bgra.to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    /// Makes a picture from the first frame of a decoded image.
    pub fn from_image(image: &RenderImage) -> Result<Self, String> {
        let size = image.size(0);
        let bytes = image.as_bytes(0).ok_or("The frame is not ready yet.")?;
        Ok(Self::from_bgra(
            size.width.0 as u32,
            size.height.0 as u32,
            bytes,
        ))
    }

    /// Encodes the picture as a PNG.
    pub fn png(self) -> Result<Vec<u8>, String> {
        let image = image::RgbaImage::from_raw(self.width, self.height, self.rgba)
            .ok_or("The frame is not ready yet.")?;
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        Ok(png)
    }
}

/// Converts a frame in the `420f` format, which is the format the video
/// player draws, to RGBA with the BT.709 matrix. The frame has one plane of
/// full-range luma and one plane of interleaved chroma at half the width and
/// half the height. Each plane is given as its bytes and its bytes per row.
#[cfg(any(test, target_os = "macos"))]
pub fn rgba_from_biplanar(
    (width, height): (usize, usize),
    luma: (&[u8], usize),
    chroma: (&[u8], usize),
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            let y = f32::from(luma.0[row * luma.1 + column]);
            let at = (row / 2) * chroma.1 + (column / 2) * 2;
            let cb = f32::from(chroma.0[at]) - 128.;
            let cr = f32::from(chroma.0[at + 1]) - 128.;
            let channel = |value: f32| value.round().clamp(0., 255.) as u8;
            rgba.extend_from_slice(&[
                channel(y + 1.5748 * cr),
                channel(y - 0.1873 * cb - 0.4681 * cr),
                channel(y + 1.8556 * cb),
                255,
            ]);
        }
    }
    rgba
}

/// Makes a picture from a CoreVideo frame, at the video's own size.
#[cfg(target_os = "macos")]
pub fn from_pixel_buffer(buffer: &objc2_core_video::CVPixelBuffer) -> Result<Picture, String> {
    use objc2_core_video::*;
    let not_ready = || "The video frame is not ready yet.".to_owned();
    // Only the format the player asks for is read. Another biplanar format
    // (video range, or 10-bit) would be read within bounds but would give
    // wrong colours.
    if CVPixelBufferGetPlaneCount(buffer) != 2
        || CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
    {
        return Err(not_ready());
    }
    // SAFETY: the buffer is locked for reading while its planes are read,
    // and each plane is read within its height and bytes per row.
    unsafe {
        if CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return Err(not_ready());
        }
        let plane = |index: usize| {
            let base = CVPixelBufferGetBaseAddressOfPlane(buffer, index).cast::<u8>();
            let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, index);
            let rows = CVPixelBufferGetHeightOfPlane(buffer, index);
            (!base.is_null()).then(|| (std::slice::from_raw_parts(base, stride * rows), stride))
        };
        let width = CVPixelBufferGetWidthOfPlane(buffer, 0);
        let height = CVPixelBufferGetHeightOfPlane(buffer, 0);
        let rgba = match (plane(0), plane(1)) {
            (Some(luma), Some(chroma)) if width > 0 && height > 0 => {
                Some(rgba_from_biplanar((width, height), luma, chroma))
            }
            _ => None,
        };
        CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
        Ok(Picture {
            width: width as u32,
            height: height as u32,
            rgba: rgba.ok_or_else(not_ready)?,
        })
    }
}

/// Runs "Set as Thumbnail" on what `surface` shows, which is the file
/// `hash`. The picture is taken at once. It is encoded and written on a
/// background thread, and afterwards every view that shows the thumbnail
/// reads it again. Only the library that is open in the window is written
/// to (`libraries::writable`), and only inside its folder.
pub fn set_as_thumbnail(
    surface: &gpui::Entity<super::surface::MediaSurface>,
    session: &gpui::Entity<crate::session::Session>,
    hash: String,
    cx: &mut gpui::App,
) {
    let Some(root) = session.read(cx).writable_root(cx) else {
        crate::ui::notify::error(
            "Could not set thumbnail",
            crate::i18n::en("No library is open."),
            cx,
        );
        return;
    };
    let Some(picture) = surface.read(cx).current_picture(cx) else {
        return;
    };
    let jpeg = session.read(cx).thumbnail(&hash);
    let written = cx.background_executor().spawn(async move {
        picture
            .and_then(Picture::png)
            .and_then(|png: Vec<u8>| write_thumbnail(&root, &jpeg, &png))
    });
    let session = session.clone();
    cx.spawn(async move |cx| {
        let result = written.await;
        cx.update(|cx| match result {
            Ok(()) => session.update(cx, |session, cx| session.thumbnail_written(hash, cx)),
            Err(error) => crate::ui::notify::error("Could not set thumbnail", error, cx),
        });
    })
    .detach();
}

/// Makes `png` the thumbnail whose `.jpg` path is `jpeg`. The PNG is written
/// beside that path with the extension `.png`, through a temporary file, and
/// the `.jpg` is deleted so that the `.png` is the one that is read. Both
/// paths must lie inside the library at `root` (`libraries::confine`).
pub fn write_thumbnail(root: &Path, jpeg: &Path, png: &[u8]) -> Result<(), String> {
    if png.is_empty() || png.len() > LARGEST {
        return Err("Invalid thumbnail size".into());
    }
    if !png.starts_with(&PNG_SIGNATURE) {
        return Err("Thumbnail data is not a PNG".into());
    }
    let output = crate::libraries::confine(root, &crate::grid::thumbnails::png_for(jpeg))?;
    let jpeg = &crate::libraries::confine(root, jpeg)?;
    if let Some(folder) = output.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let mut temporary = output.clone().into_os_string();
    temporary.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis())
    ));
    let temporary = std::path::PathBuf::from(temporary);
    let written = std::fs::write(&temporary, png)
        .and_then(|_| std::fs::rename(&temporary, &output))
        .and_then(|_| match std::fs::remove_file(jpeg) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        });
    let _ = std::fs::remove_file(&temporary);
    written.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::paths::Canonical as _;
    use super::{Picture, rgba_from_biplanar, write_thumbnail};

    #[test]
    fn grey_and_colour_come_through_the_video_matrix() {
        // Mid grey has no chroma, so all three channels are 128.
        assert_eq!(
            rgba_from_biplanar((1, 1), (&[128], 1), (&[128, 128], 2)),
            [128, 128, 128, 255]
        );
        // A large red difference gives red.
        let red = rgba_from_biplanar((2, 2), (&[54, 54, 54, 54], 2), (&[99, 255], 2));
        assert!(red[0] > 200 && red[1] < 40 && red[2] < 40);
        assert_eq!(red.len(), 16);
    }

    #[test]
    fn a_frame_becomes_the_png_thumbnail_and_the_jpg_goes() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().canonical().unwrap();
        let jpeg = root.join("ab/cd/abcd.jpg");
        std::fs::create_dir_all(jpeg.parent().unwrap()).unwrap();
        std::fs::write(&jpeg, b"old").unwrap();
        let png = Picture::from_bgra(1, 1, &[0, 0, 255, 255]).png().unwrap();
        write_thumbnail(&root, &jpeg, &png).unwrap();
        assert!(!jpeg.exists());
        let written = image::open(jpeg.with_extension("png")).unwrap().to_rgba8();
        // The pixel was given as BGRA with only its red byte set, so the pixel
        // of the PNG is red.
        assert_eq!(written.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(
            write_thumbnail(&root, &jpeg, b"not a png"),
            Err("Thumbnail data is not a PNG".into())
        );
    }

    /// A thumbnail whose folder leads out of the library through a symbolic
    /// link is not written, and nothing is removed or written in the folder
    /// the link leads to.
    #[cfg(unix)]
    #[test]
    fn a_thumbnail_is_written_only_inside_the_library() {
        let library = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = library.path().canonical().unwrap();
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.join("blobs/t")).unwrap();
        std::fs::write(elsewhere.path().join("abcd.jpg"), b"kept").unwrap();
        let png = Picture::from_bgra(1, 1, &[0, 0, 255, 255]).png().unwrap();
        let jpeg = root.join("blobs/t/abcd.jpg");
        assert!(write_thumbnail(&root, &jpeg, &png).is_err());
        assert_eq!(
            std::fs::read(elsewhere.path().join("abcd.jpg")).unwrap(),
            b"kept"
        );
        assert!(!elsewhere.path().join("abcd.png").exists());
        // A thumbnail path inside another library is refused as well.
        let other = tempfile::tempdir().unwrap();
        assert!(write_thumbnail(&root, &other.path().join("x.jpg"), &png).is_err());
        assert!(!other.path().join("x.png").exists());
    }
}
