//! `LibraryAvatar`: a library's cover in a disc, set into the surface by a
//! bevel, or the books glyph when the library has no cover.
use crate::grid::thumbnails::Thumbnails;
use crate::{motion, theme};
use gpui::*;
use std::path::PathBuf;

#[derive(IntoElement)]
pub struct LibraryAvatar {
    id: ElementId,
    cover: Option<PathBuf>,
    size: f32,
    cache: Option<Entity<Thumbnails>>,
    highlighted: bool,
    hovered: bool,
}

/// The avatar of a library whose cropped cover, if it has one, is at
/// `cover`. The disc is `size` pixels across.
pub fn library_avatar(
    id: impl Into<ElementId>,
    cover: Option<PathBuf>,
    size: f32,
) -> LibraryAvatar {
    LibraryAvatar {
        id: id.into(),
        cover,
        size,
        cache: None,
        highlighted: false,
        hovered: false,
    }
}

impl LibraryAvatar {
    /// Decodes the cover through this cache, which the caller pins.
    /// Without one the cover goes through GPUI's own image assets.
    pub fn cache(mut self, cache: &Entity<Thumbnails>) -> Self {
        self.cache = Some(cache.clone());
        self
    }

    /// `.highlighted`: the open library, ringed in the primary colour.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    /// The row holding the avatar is under the pointer
    /// (`.item:hover .itemIcon`): a ring in the text colour, which wins over
    /// the highlight.
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }
}

impl RenderOnce for LibraryAvatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // The ring's colours move over `--duration-fast`, as the
        // `box-shadow` transition does.
        let ring = window.use_keyed_state(self.id.clone(), cx, |_, _| {
            (
                motion::Fade::at(self.highlighted),
                motion::Fade::at(self.hovered),
            )
        });
        ring.update(cx, |(highlight, hover), _| {
            highlight.set(self.highlighted, motion::SIDEBAR_HOVER);
            hover.set(self.hovered, motion::SIDEBAR_HOVER);
        });
        let (highlight, hover, animating) = {
            let (highlight, hover) = ring.read(cx);
            (
                highlight.value(),
                hover.value(),
                highlight.animating() || hover.animating(),
            )
        };
        if animating {
            window.request_animation_frame();
        }
        let primary = theme.primary();
        let text = theme.text();
        let at = |color: Rgba, alpha: f32| Rgba { a: alpha, ..color };
        // The inner 2px ring and the outer 1px halo: the highlight's at 72%
        // and 24% primary, the hover's at 30% and 9% text.
        let tone = |highlit: f32, hovered: f32| {
            let resting = theme::blend(theme::clear(primary), at(primary, highlit), highlight);
            theme::blend(resting, at(text, hovered), hover)
        };
        let outer = |spread: f32, color: Rgba| {
            BoxShadow::new(px(0.), px(0.), color.into()).spread_radius(px(spread))
        };
        let inset = |y: f32, blur: f32, spread: f32, color: Rgba| {
            BoxShadow::new(px(0.), px(y), color.into())
                .blur_radius(px(blur))
                .spread_radius(px(spread))
                .inset()
        };
        let shadows = vec![
            outer(2., tone(0.72, 0.30)),
            outer(3., tone(0.24, 0.09)),
            // The disc's own edge, under the cover.
            inset(1., 0., 0., theme.contrast(20.)),
            inset(0., 0., 1., theme.border_secondary()),
            inset(-1., 2., 0., rgba(theme::BLACK_20)),
        ];
        let size = self.size;
        let content = match self.cover {
            Some(path) => {
                let image = img(path);
                match &self.cache {
                    Some(cache) => image.image_cache(cache),
                    None => image,
                }
                .size_full()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element()
            }
            // `IconBooks` at 72% of the disc, stroke 0.9.
            None => super::icon("books", (size * 0.72).round(), theme.text_secondary())
                .into_any_element(),
        };
        div()
            .relative()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .bg(theme.avatar_surface())
            .shadow(shadows)
            .flex()
            .items_center()
            .justify_center()
            .child(content)
            // `.avatar::after`: the bevel that sets the cover into the
            // surface, darker at the top and lit along the bottom.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .shadow(theme::avatar_bevel()),
            )
    }
}
