//! Page geometry of the document viewers (`documentPageGeometry.ts`, and the
//! scale arithmetic of `PdfViewer.tsx`). Sizes are (width, height).

pub type PageSize = (f32, f32);

/// `fitDocumentPage`: the page scaled to fit the viewport's content box.
pub fn fit_page(available: PageSize, natural: PageSize) -> Option<PageSize> {
    if available.0 <= 0. || available.1 <= 0. || natural.0 <= 0. || natural.1 <= 0. {
        return None;
    }
    let scale = (available.0 / natural.0).min(available.1 / natural.1);
    Some((natural.0 * scale, natural.1 * scale))
}

/// `boundDocumentPageWidth`: fixed-width documents scroll instead of
/// shrinking to the viewport's height.
pub fn bound_page_width(
    available_width: f32,
    natural: PageSize,
    maximum_width: f32,
) -> Option<PageSize> {
    if available_width <= 0. || natural.0 <= 0. || natural.1 <= 0. {
        return None;
    }
    let width = available_width.min(maximum_width);
    Some((width, width * natural.1 / natural.0))
}

/// `max-width: 100%; max-height: 100%; object-fit: contain` on a canvas or
/// image: its natural size, shrunk to fit, never enlarged. The DjVu and
/// comic pages are shown this way.
pub fn contain_page(available: PageSize, natural: PageSize) -> Option<PageSize> {
    let fitted = fit_page(available, natural)?;
    Some(if fitted.0 >= natural.0 {
        natural
    } else {
        fitted
    })
}

/// `documentCanvasGeometry`: the page's size on screen and in backing
/// pixels for a scale and the display's pixel ratio.
#[derive(Debug, PartialEq)]
pub struct CanvasGeometry {
    pub css: PageSize,
    pub pixels: (u32, u32),
    pub render_scale: f32,
}

pub fn canvas_geometry(natural: PageSize, scale: f32, pixel_ratio: f32) -> Option<CanvasGeometry> {
    if natural.0 <= 0. || natural.1 <= 0. || scale <= 0. || pixel_ratio <= 0. {
        return None;
    }
    let render_scale = scale * pixel_ratio;
    Some(CanvasGeometry {
        css: (natural.0 * scale, natural.1 * scale),
        pixels: (
            (natural.0 * render_scale).ceil() as u32,
            (natural.1 * render_scale).ceil() as u32,
        ),
        render_scale,
    })
}

/// How a PDF page is scaled (`scaleMode` in `PdfViewer.tsx`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScaleMode {
    PageFit,
    Actual,
    Custom(f32),
}

/// The smallest and largest custom PDF zoom, and one zoom step.
pub const MIN_SCALE: f32 = 0.25;
pub const MAX_SCALE: f32 = 8.;
pub const STEP: f32 = 1.2;

impl ScaleMode {
    /// The page's scale in the viewport's content box. Never below 0.1, as
    /// production clamps it.
    pub fn scale(self, available: PageSize, natural: PageSize) -> f32 {
        let fit = fit_page((available.0.max(1.), available.1.max(1.)), natural)
            .map_or(1., |fitted| fitted.0 / natural.0);
        match self {
            ScaleMode::PageFit => fit,
            ScaleMode::Actual => 1.,
            ScaleMode::Custom(scale) => scale,
        }
        .max(0.1)
    }

    /// `setZoomScale`.
    pub fn custom(scale: f32) -> Self {
        ScaleMode::Custom(scale.clamp(MIN_SCALE, MAX_SCALE))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CanvasGeometry, ScaleMode, bound_page_width, canvas_geometry, contain_page, fit_page,
    };

    #[test]
    fn fits_tall_pages_from_the_available_height() {
        assert_eq!(fit_page((1200., 900.), (600., 800.)), Some((675., 900.)));
        assert_eq!(fit_page((0., 900.), (600., 800.)), None);
    }

    #[test]
    fn keeps_fixed_width_documents_bounded() {
        assert_eq!(
            bound_page_width(1200., (816., 1056.), 816.),
            Some((816., 1056.))
        );
        assert_eq!(
            bound_page_width(408., (816., 1056.), 816.),
            Some((408., 528.))
        );
    }

    #[test]
    fn uses_one_transform_for_screen_and_backing_pixels() {
        assert_eq!(
            canvas_geometry((612., 792.), 1.25, 2.),
            Some(CanvasGeometry {
                css: (765., 990.),
                pixels: (1530, 1980),
                render_scale: 2.5,
            })
        );
    }

    #[test]
    fn contained_pages_shrink_but_never_grow() {
        assert_eq!(
            contain_page((1000., 1000.), (200., 100.)),
            Some((200., 100.))
        );
        assert_eq!(
            contain_page((500., 500.), (2550., 3300.)).map(|(w, h)| (w.round(), h)),
            Some((386., 500.))
        );
    }

    #[test]
    fn pdf_scales_by_mode() {
        let natural = (612., 792.);
        assert_eq!(ScaleMode::PageFit.scale((612., 396.), natural), 0.5);
        assert_eq!(ScaleMode::Actual.scale((10., 10.), natural), 1.);
        assert_eq!(ScaleMode::custom(20.), ScaleMode::Custom(8.));
        assert_eq!(ScaleMode::custom(0.01), ScaleMode::Custom(0.25));
        // A tiny viewport still gives a usable page.
        assert_eq!(ScaleMode::PageFit.scale((1., 1.), natural), 0.1);
    }
}
