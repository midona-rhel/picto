//! Page geometry of the document viewers (`documentPageGeometry.ts`, the
//! scale arithmetic of `PdfViewer.tsx`, and the continuous column of pages
//! the paged viewer scrolls). Sizes are (width, height).

pub type PageSize = (f32, f32);

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
    /// `setZoomScale`.
    pub fn custom(scale: f32) -> Self {
        ScaleMode::Custom(scale.clamp(MIN_SCALE, MAX_SCALE))
    }

    /// A page's scale in the continuous column: fitted, it is as wide as the
    /// viewport's content box but never larger than its natural size; at
    /// 100% or a custom zoom every page takes the same scale.
    pub fn column_scale(self, available_width: f32, natural: PageSize) -> f32 {
        match self {
            ScaleMode::PageFit if natural.0 > 0. => (available_width / natural.0).min(1.),
            ScaleMode::PageFit => 1.,
            ScaleMode::Actual => 1.,
            ScaleMode::Custom(scale) => scale,
        }
        .max(0.1)
    }
}

/// A page's place in the column: left, top, width, height.
pub type PageRect = (f32, f32, f32, f32);

/// The pages of a document one under the other, as a viewer scrolls them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Column {
    pub pages: Vec<PageRect>,
    /// The content's size, padding included.
    pub size: PageSize,
}

/// Lays `sizes` (each page's size on screen) out in a column: `padding`
/// around, `gap` between, each page centred across the wider of the
/// content and the viewport, and the whole column centred in a viewport
/// taller than it.
pub fn column(sizes: &[PageSize], viewport: PageSize, padding: f32, gap: f32) -> Column {
    let widest = sizes.iter().map(|size| size.0).fold(0., f32::max);
    let width = (widest + 2. * padding).max(viewport.0);
    let stacked: f32 = sizes.iter().map(|size| size.1).sum::<f32>()
        + gap * sizes.len().saturating_sub(1) as f32
        + 2. * padding;
    let height = stacked.max(viewport.1);
    let mut top = padding + (height - stacked) / 2.;
    let pages = sizes
        .iter()
        .map(|&(page_width, page_height)| {
            let place = ((width - page_width) / 2., top, page_width, page_height);
            top += page_height + gap;
            place
        })
        .collect();
    Column {
        pages,
        size: (width, height),
    }
}

impl Column {
    /// The pages any part of which lies between `from` and `to` (content
    /// coordinates, top down).
    pub fn between(&self, from: f32, to: f32) -> std::ops::Range<usize> {
        let first = self
            .pages
            .iter()
            .position(|page| page.1 + page.3 > from)
            .unwrap_or(self.pages.len());
        let end = self
            .pages
            .iter()
            .rposition(|page| page.1 < to)
            .map_or(first, |last| (last + 1).max(first));
        first..end
    }

    /// The page at `y`: the one it falls on, or, in a gap or the padding,
    /// the last one starting above it (the first one above the column).
    pub fn page_at(&self, y: f32) -> usize {
        self.pages.iter().rposition(|page| page.1 <= y).unwrap_or(0)
    }

    /// Where `y` is, as a page and a fraction of its height, for keeping a
    /// place when the zoom changes.
    pub fn anchor(&self, y: f32) -> Option<(usize, f32)> {
        let index = self.page_at(y);
        let page = self.pages.get(index)?;
        Some((index, (y - page.1) / page.3.max(1.)))
    }

    /// The `y` of an anchor taken in another layout of the same pages.
    pub fn resolve(&self, anchor: (usize, f32)) -> Option<f32> {
        let page = self.pages.get(anchor.0)?;
        Some(page.1 + anchor.1 * page.3)
    }
}

/// Whether a drawn page that arrives is kept: the size now wanted for that
/// page, if it is still wanted, and the size of the picture it already has.
/// A picture at the wanted size is kept; one drawn for an earlier zoom or
/// width that arrives late is kept only while the page has no picture at
/// all, so a page is never blank but never goes back to an older drawing.
pub fn keep_drawn(
    arrived: (u32, u32),
    wanted: Option<(u32, u32)>,
    held: Option<(u32, u32)>,
) -> bool {
    match wanted {
        None => false,
        Some(wanted) if wanted == arrived => true,
        Some(_) => held.is_none(),
    }
}

/// The pixels a page is drawn at for its size on screen and the display's
/// pixel ratio (`documentCanvasGeometry`).
pub fn page_pixels(size: PageSize, pixel_ratio: f32) -> Option<(u32, u32)> {
    if size.0 <= 0. || size.1 <= 0. || pixel_ratio <= 0. {
        return None;
    }
    Some((
        ((size.0 * pixel_ratio).ceil() as u32).max(1),
        ((size.1 * pixel_ratio).ceil() as u32).max(1),
    ))
}

#[cfg(test)]
mod tests {
    use super::{Column, ScaleMode, bound_page_width, column, keep_drawn, page_pixels};

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
    fn fitted_pages_take_the_width_but_never_grow() {
        let letter = (612., 792.);
        assert_eq!(ScaleMode::PageFit.column_scale(306., letter), 0.5);
        assert_eq!(ScaleMode::PageFit.column_scale(2000., letter), 1.);
        assert_eq!(ScaleMode::Actual.column_scale(306., letter), 1.);
        assert_eq!(ScaleMode::Custom(2.).column_scale(306., letter), 2.);
        assert_eq!(ScaleMode::custom(20.), ScaleMode::Custom(8.));
        assert_eq!(ScaleMode::custom(0.01), ScaleMode::Custom(0.25));
    }

    #[test]
    fn pages_stack_with_padding_and_gaps() {
        let laid = column(&[(100., 200.), (50., 100.)], (300., 100.), 24., 16.);
        assert_eq!(
            laid,
            Column {
                pages: vec![(100., 24., 100., 200.), (125., 240., 50., 100.)],
                size: (300., 364.),
            }
        );
        // A short column is centred in a taller viewport.
        let short = column(&[(100., 100.)], (300., 400.), 24., 16.);
        assert_eq!(short.pages, vec![(100., 150., 100., 100.)]);
        // A page wider than the viewport makes the content wider.
        let wide = column(&[(500., 100.)], (300., 400.), 24., 16.);
        assert_eq!(wide.size.0, 548.);
        assert_eq!(wide.pages[0].0, 24.);
    }

    #[test]
    fn finds_the_pages_in_view() {
        let laid = column(&[(100., 100.); 5], (100., 0.), 0., 10.);
        // Tops 0, 110, 220, 330, 440.
        assert_eq!(laid.between(0., 100.), 0..1);
        assert_eq!(laid.between(105., 225.), 1..3);
        assert_eq!(laid.between(1000., 1100.), 5..5);
        assert_eq!(laid.page_at(105.), 0);
        assert_eq!(laid.page_at(115.), 1);
        assert_eq!(laid.page_at(-5.), 0);
    }

    #[test]
    fn a_place_survives_a_zoom() {
        let small = column(&[(100., 100.); 3], (100., 0.), 0., 10.);
        let large = column(&[(200., 200.); 3], (200., 0.), 0., 10.);
        let anchor = small.anchor(160.).unwrap();
        assert_eq!(anchor, (1, 0.5));
        assert_eq!(large.resolve(anchor), Some(310.));
    }

    #[test]
    fn late_drawings_never_replace_a_newer_one() {
        // The page's drawing for the zoom now on screen is kept.
        assert!(keep_drawn((200, 300), Some((200, 300)), Some((100, 150))));
        // One for an earlier zoom arriving late is dropped while the page
        // has a picture, and kept while it has none.
        assert!(!keep_drawn((100, 150), Some((200, 300)), Some((200, 300))));
        assert!(keep_drawn((100, 150), Some((200, 300)), None));
        // A page scrolled away keeps nothing.
        assert!(!keep_drawn((100, 150), None, None));
        assert_eq!(page_pixels((612., 792.), 2.), Some((1224, 1584)));
        assert_eq!(page_pixels((0., 792.), 2.), None);
    }
}
