//! A library's avatar: the library's cover in a disc, or the books glyph when
//! the library has no cover. The disc has a bevel that makes the cover look
//! sunk into the surface around it.
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
    icon: Option<SharedString>,
    color: Option<Rgba>,
}

/// Returns the avatar of a library. `cover` is the path of the library's
/// cropped cover, if it has one. The disc is `size` pixels across.
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
        icon: None,
        color: None,
    }
}

impl LibraryAvatar {
    /// Makes the cover be decoded through the cache `cache`, which the caller
    /// owns. Without a cache the cover is loaded through GPUI's own image
    /// assets.
    pub fn cache(mut self, cache: &Entity<Thumbnails>) -> Self {
        self.cache = Some(cache.clone());
        self
    }

    /// Sets whether this is the open library. The avatar of the open library
    /// has a ring in the primary colour.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    /// Sets the library's icon (a name from the icon picker) and its colour.
    /// They are used when the library has no cover. The avatar then shows the
    /// icon in the colour, or the books glyph in the colour when the library
    /// has no icon.
    pub fn appearance(mut self, icon: Option<String>, color: Option<Rgba>) -> Self {
        self.icon = icon.map(Into::into);
        self.color = color;
        self
    }

    /// Sets whether the pointer is over the row that holds the avatar. The
    /// avatar then has a ring in the text colour, in place of the ring of the
    /// open library.
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }
}

impl RenderOnce for LibraryAvatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        // The ring's colours change over `motion::FAST`.
        let ring = window.use_keyed_state(self.id.clone(), cx, |_, _| {
            (
                motion::Fade::at(self.highlighted),
                motion::Fade::at(self.hovered),
            )
        });
        ring.update(cx, |(highlight, hover), _| {
            highlight.set(self.highlighted, motion::FAST);
            hover.set(self.hovered, motion::FAST);
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
        // The ring is 2px wide and has a fainter 1px line around it. For the
        // open library they are the primary colour at 72% and 24%. Under the
        // pointer they are the text colour at 30% and 9%.
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
            // The disc's own edge, which is drawn under the cover.
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
            None => {
                let color = self.color.unwrap_or(theme.text_secondary());
                match self.icon {
                    // The library's icon, at 72% of the disc's size.
                    Some(icon) => {
                        super::icon(icon, (size * 0.72).round(), color).into_any_element()
                    }
                    // The books glyph, at 72% of the disc's size.
                    None => super::icon("books", (size * 0.72).round(), color).into_any_element(),
                }
            }
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
            // The bevel that makes the cover look sunk into the surface:
            // darker at the top and lighter along the bottom.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .shadow(theme::avatar_bevel()),
            )
    }
}
