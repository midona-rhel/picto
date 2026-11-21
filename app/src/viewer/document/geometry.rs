//! The arithmetic of the document viewers' layout: the width of a page that
//! has a largest width, the scale of a PDF's pages, and the column in which
//! the paged viewer puts all the pages of a document one under the other.
//! Every size is a pair of a width and a height.

pub type PageSize = (f32, f32);

/// Returns the size of a page that is as wide as `available_width`, but at
/// most `maximum_width` wide, and that has the proportions of `natural`.
/// Only the width is limited. A page that is higher than the viewport is
/// not made smaller; the viewport scrolls. Returns `None` when
/// `available_width` or a side of `natural` is not above 0.
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

/// How the pages of a PDF are scaled. With `PageFit`, a page that is wider
/// than the viewport has room for is scaled down to fit. With `Actual`,
/// every page has its natural size. With `Custom`, every page is scaled by
/// the factor it holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScaleMode {
    PageFit,
    Actual,
    Custom(f32),
}

/// The smallest factor of `ScaleMode::Custom`, 25%. `MAX_SCALE` is the
/// largest, 800%. `STEP` is the factor by which one step of zooming in
/// multiplies the scale and one step of zooming out divides it.
pub const MIN_SCALE: f32 = 0.25;
pub const MAX_SCALE: f32 = 8.;
pub const STEP: f32 = 1.2;

impl ScaleMode {
    /// Returns the mode that scales every page by `scale`, which is first
    /// limited to the range from `MIN_SCALE` to `MAX_SCALE`.
    pub fn custom(scale: f32) -> Self {
        ScaleMode::Custom(scale.clamp(MIN_SCALE, MAX_SCALE))
    }

    /// Returns the factor by which a page of the natural size `natural` is
    /// scaled in the column. With `PageFit`, the page is scaled down to
    /// `available_width`, the viewport's width less its padding, and a page
    /// that is not wider than that keeps its natural size. With `Actual`
    /// the factor is 1, and with `Custom` it is the custom factor, for
    /// every page. The factor returned is never below 0.1.
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

/// The place of a page in the column: its left edge, its top edge, its
/// width and its height, in pixels from the column's top-left corner.
pub type PageRect = (f32, f32, f32, f32);

/// The layout of all the pages of a document one under the other, which is
/// the content that the paged viewer scrolls. `pages` holds the place of
/// each page.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Column {
    pub pages: Vec<PageRect>,
    /// The width and height of the whole content, with the padding around
    /// the pages.
    pub size: PageSize,
}

/// Lays the pages out in a column. `sizes` holds the size of each page on
/// screen. There are `padding` pixels around the pages and `gap` pixels
/// between two pages. The content is as wide as the widest page with its
/// padding, or as wide as `viewport` when that is wider, and each page is
/// centred in that width. When the pages with their gaps and padding are
/// less high than `viewport`, the content is as high as `viewport` and the
/// pages are centred in that height.
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
    /// Returns the indexes of the pages of which a part lies between `from`
    /// and `to`, which are distances in pixels from the content's top.
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

    /// Returns the index of the page at `y`, a distance in pixels from the
    /// content's top. That is the last page whose top edge is at or above
    /// `y`, so a `y` in the gap or the padding under a page gives that
    /// page. A `y` above the first page gives the first page.
    pub fn page_at(&self, y: f32) -> usize {
        self.pages.iter().rposition(|page| page.1 <= y).unwrap_or(0)
    }

    /// Returns where `y` is as the index of a page (`page_at`) and the
    /// distance of `y` below that page's top edge as a fraction of the
    /// page's height. The paged viewer takes it before the zoom changes and
    /// gives it to `resolve` on the new layout, so that the same part of
    /// the document stays in view. Returns `None` when there are no pages.
    pub fn anchor(&self, y: f32) -> Option<(usize, f32)> {
        let index = self.page_at(y);
        let page = self.pages.get(index)?;
        Some((index, (y - page.1) / page.3.max(1.)))
    }

    /// Returns the distance from this content's top, in pixels, of the
    /// point that `anchor` names. `anchor` comes from `anchor` on another
    /// layout of the same pages. Returns `None` when this layout has no
    /// page with the anchor's index.
    pub fn resolve(&self, anchor: (usize, f32)) -> Option<f32> {
        let page = self.pages.get(anchor.0)?;
        Some(page.1 + anchor.1 * page.3)
    }
}

/// Returns whether the paged viewer keeps a drawing of a page that has just
/// arrived. `arrived` is the size the drawing was asked for. `wanted` is
/// the size at which the page is wanted now, or `None` when the page is not
/// wanted any more. `held` is the size of the drawing the page already has,
/// or `None` when it has none. A drawing at the wanted size is kept. A
/// drawing that was asked for at an earlier zoom or width is kept only
/// while the page has no drawing at all. So a wanted page shows a drawing
/// as soon as one arrives, and a later drawing is never replaced by an
/// earlier one. A drawing of a page that is not wanted is discarded.
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

/// Returns the width and height in device pixels of the drawing that a page
/// needs: its size on screen, `size`, multiplied by `pixel_ratio`, the
/// number of device pixels in one logical pixel, and rounded up. Each side
/// is at least 1. Returns `None` when a side of `size` or `pixel_ratio` is
/// not above 0.
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
        // A column that is less high than the viewport is centred in the
        // viewport's height.
        let short = column(&[(100., 100.)], (300., 400.), 24., 16.);
        assert_eq!(short.pages, vec![(100., 150., 100., 100.)]);
        // A page that is wider than the viewport makes the content wider
        // than the viewport.
        let wide = column(&[(500., 100.)], (300., 400.), 24., 16.);
        assert_eq!(wide.size.0, 548.);
        assert_eq!(wide.pages[0].0, 24.);
    }

    #[test]
    fn finds_the_pages_in_view() {
        let laid = column(&[(100., 100.); 5], (100., 0.), 0., 10.);
        // The tops of the five pages are at 0, 110, 220, 330 and 440.
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
        // A drawing at the size that the page is wanted at now is kept.
        assert!(keep_drawn((200, 300), Some((200, 300)), Some((100, 150))));
        // A drawing at a size that was wanted earlier is discarded when the
        // page has a drawing, and kept when it has none.
        assert!(!keep_drawn((100, 150), Some((200, 300)), Some((200, 300))));
        assert!(keep_drawn((100, 150), Some((200, 300)), None));
        // A drawing of a page that is not wanted any more is discarded.
        assert!(!keep_drawn((100, 150), None, None));
        assert_eq!(page_pixels((612., 792.), 2.), Some((1224, 1584)));
        assert_eq!(page_pixels((0., 792.), 2.), None);
    }
}
