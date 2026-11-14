//! "Set as Thumbnail": the picture a video or Flash movie shows now, as the
//! PNG production's `captureVideoFrame` and `captureCurrentWindowRect` make,
//! written as the file's thumbnail (`setThumbnail`).
use gpui::RenderImage;
use std::path::Path;

/// `setThumbnail`'s limit on what it writes.
const LARGEST: usize = 32 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// A captured picture, RGBA, before it is encoded.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// From pixels held as GPUI holds them (BGRA).
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

    /// The first frame of a decoded image.
    pub fn from_image(image: &RenderImage) -> Result<Self, String> {
        let size = image.size(0);
        let bytes = image.as_bytes(0).ok_or("The frame is not ready yet.")?;
        Ok(Self::from_bgra(
            size.width.0 as u32,
            size.height.0 as u32,
            bytes,
        ))
    }

    /// `canvas.toDataURL('image/png')`.
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

/// Full-range 4:2:0 luma and interleaved chroma (`420f`, what the video
/// player draws) to RGBA, by the BT.709 matrix.
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

/// A CoreVideo frame at the video's own size.
#[cfg(target_os = "macos")]
pub fn from_pixel_buffer(buffer: &objc2_core_video::CVPixelBuffer) -> Result<Picture, String> {
    use objc2_core_video::*;
    let not_ready = || "The video frame is not ready yet.".to_owned();
    if CVPixelBufferGetPlaneCount(buffer) != 2 {
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

/// "Set as Thumbnail" on what `surface` shows, the file `hash`: the picture
/// is taken now, then encoded and written off the thread that draws, and
/// every view showing the thumbnail reads it again. Only a marked
/// disposable copy is written to.
pub fn set_as_thumbnail(
    surface: &gpui::Entity<super::surface::MediaSurface>,
    session: &gpui::Entity<crate::session::Session>,
    hash: String,
    cx: &mut gpui::App,
) {
    if !session.read(cx).disposable() {
        return;
    }
    let Some(picture) = surface.read(cx).current_picture(cx) else {
        return;
    };
    let jpeg = session.read(cx).thumbnail(&hash);
    let written = cx.background_executor().spawn(async move {
        picture
            .and_then(Picture::png)
            .and_then(|png: Vec<u8>| write_thumbnail(&jpeg, &png))
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

/// `setThumbnail`: `png` becomes the thumbnail whose `.jpg` path is `jpeg`,
/// written beside it as `.png` (through a temporary file), and the `.jpg`
/// goes so the `.png` is the one read.
pub fn write_thumbnail(jpeg: &Path, png: &[u8]) -> Result<(), String> {
    if png.is_empty() || png.len() > LARGEST {
        return Err("Invalid thumbnail size".into());
    }
    if !png.starts_with(&PNG_SIGNATURE) {
        return Err("Thumbnail data is not a PNG".into());
    }
    let output = crate::grid::thumbnails::png_for(jpeg);
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
    use super::{Picture, rgba_from_biplanar, write_thumbnail};

    #[test]
    fn grey_and_colour_come_through_the_video_matrix() {
        // Mid grey: no chroma.
        assert_eq!(
            rgba_from_biplanar((1, 1), (&[128], 1), (&[128, 128], 2)),
            [128, 128, 128, 255]
        );
        // Strong red difference makes red.
        let red = rgba_from_biplanar((2, 2), (&[54, 54, 54, 54], 2), (&[99, 255], 2));
        assert!(red[0] > 200 && red[1] < 40 && red[2] < 40);
        assert_eq!(red.len(), 16);
    }

    #[test]
    fn a_frame_becomes_the_png_thumbnail_and_the_jpg_goes() {
        let folder = tempfile::tempdir().unwrap();
        let jpeg = folder.path().join("ab/cd/abcd.jpg");
        std::fs::create_dir_all(jpeg.parent().unwrap()).unwrap();
        std::fs::write(&jpeg, b"old").unwrap();
        let png = Picture::from_bgra(1, 1, &[0, 0, 255, 255]).png().unwrap();
        write_thumbnail(&jpeg, &png).unwrap();
        assert!(!jpeg.exists());
        let written = image::open(jpeg.with_extension("png")).unwrap().to_rgba8();
        // BGRA blue-green-red in, red out.
        assert_eq!(written.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(
            write_thumbnail(&jpeg, b"not a png"),
            Err("Thumbnail data is not a PNG".into())
        );
    }
}
