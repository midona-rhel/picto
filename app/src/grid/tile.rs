//! One tile of the grid: what it shows and how it is drawn.
//!
//! [`tile_art`] draws a tile. The library's grid and a collection's editor
//! both use it. `GridView::tile`, further down, builds the tile of the
//! library's grid and adds the area over the picture that receives clicks.
use super::prefs::ViewPrefs;
use super::thumbnails::{Thumb, Thumbnails};
use super::{Activated, GridView, MenuRequested, art, format, layout, marquee, reveal, selection};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::prelude::FluentBuilder;
use gpui::*;
use layout::ViewMode;
use picto_library::query::RootSummary;
use std::path::PathBuf;

/// The distance in pixels by which the name and the resolution under a
/// tile's picture are moved down. It is negative, so the text is moved up by
/// half a pixel. GPUI centres a line's ascent and descent in its row, which
/// puts the text half a pixel lower than centring it on the middle of the
/// font's em box does. Picto 0.6.19 and earlier centred the text on the
/// middle of the em box, and this value keeps the text at the same height.
const CANVAS_TEXT_LIFT: f32 = -0.5;

/// Returns `text` shaped as one line in the 13 px interface font, which is
/// the font a tile's name is drawn in.
pub(super) fn name_line(text: &str, window: &Window) -> ShapedLine {
    crate::ui::shape_line(
        text.to_owned(),
        font(theme::FONT_UI),
        theme::TEXT_MD,
        black(),
        window,
    )
}

/// Returns the width in pixels that `text` has in the 13 px interface font,
/// which is the font a tile's name is drawn in.
pub(super) fn name_width(text: &str, window: &Window) -> f32 {
    f32::from(name_line(text, window).width)
}

/// Returns the item's width divided by its height, or `None` when its width
/// and height are not known. For an audio item whose width and height are not
/// known it returns 2, so that the item is drawn twice as wide as it is tall.
pub(super) fn aspect(item: &RootSummary) -> Option<f64> {
    match (item.width, item.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(f64::from(width) / f64::from(height))
        }
        _ if item.mime.starts_with("audio/") => Some(2.),
        _ => None,
    }
}

/// The data of one tile that is drawn in a frame. `GridView::snapshot`
/// copies it out of the session, and only for the tiles that are drawn.
pub(super) struct TileData {
    /// The tile's index in the layout of the window of results.
    pub index: usize,
    /// The item's position in the destination's order.
    pub ordinal: usize,
    /// What the tile shows. The `y` of its tile is measured from the top of
    /// the scrollable height that stands for every item of the destination.
    pub look: TileLook,
}

/// Returns two values. The first says whether the tile shows the whole of
/// its picture with empty space beside it (`true`), or fills its whole image
/// area with the picture and cuts off what does not fit (`false`). The second
/// is the rectangle the picture takes inside the tile's image area: x, y,
/// width and height in pixels.
///
/// - In the grid mode with Fit Thumbnails on, pictures and videos fill the
///   square tile.
/// - In the grid mode with Fit Thumbnails off, everything is shown whole.
/// - Waterfall and justified tiles already have their item's shape, so a
///   picture fills them. A video is shown whole there, because its thumbnail
///   can have another shape than the item (a rotated video).
/// - Audio is always shown whole: its picture is a waveform, and filling the
///   tile would cut off the waveform's two ends.
///
/// The picture of an item whose width and height are not known takes the
/// whole image area.
pub(super) fn media_rect(
    tile: &layout::Tile,
    item: &RootSummary,
    prefs: &ViewPrefs,
) -> (bool, (f32, f32, f32, f32)) {
    let image_height = tile.h - prefs.text_height();
    let grid = prefs.mode == ViewMode::Grid;
    let contain = if item.mime.starts_with("audio/") {
        true
    } else if grid {
        !prefs.fit_cover
    } else {
        item.mime.starts_with("video/")
    };
    let rect = aspect(item)
        .filter(|_| contain)
        .and_then(|aspect| {
            crate::viewer::camera::contain((aspect as f32, 1.), (tile.w, image_height))
        })
        .unwrap_or((0., 0., tile.w, image_height));
    (contain, rect)
}

/// Returns the element that draws the picture `image` centred on the tile's
/// image area. `size` is the picture's width and height in its own units,
/// with `art::MARGIN` on each side included, and `scale` is the number of
/// pixels per unit. `area` is the width and height of the tile's image area.
/// `media` is the position of the media rectangle in that area. The element
/// is a child of the media rectangle, so its position is relative to it.
fn art_at(
    image: std::sync::Arc<Image>,
    size: (f32, f32),
    scale: f32,
    area: (f32, f32),
    media: (f32, f32),
) -> Img {
    let (width, height) = (size.0 * scale, size.1 * scale);
    img(image)
        .absolute()
        .left(px((area.0 - width) / 2. - media.0))
        .top(px((area.1 - height) / 2. - media.1))
        .w(px(width))
        .h(px(height))
}

/// What one tile shows. [`tile_art`] draws a tile from it.
pub(crate) struct TileLook {
    pub tile: layout::Tile,
    pub item: RootSummary,
    pub thumbnail: PathBuf,
    pub selected: bool,
    pub thumb: Thumb,
    /// The item's original file, decoded at the size the tile draws it. It
    /// is set when the tile draws its picture with more pixels than its
    /// thumbnail has and the original has been decoded.
    pub original: Option<std::sync::Arc<RenderImage>>,
    /// The opacity of the picture and the opacity of its placeholder, in
    /// that order, while the thumbnail fades in.
    pub alphas: (f32, f32),
    /// Whether the magnifier icon is drawn on the picture. It is drawn while
    /// the pointer is on the picture.
    pub magnifier: bool,
    /// Whether the whole tile, its placeholder included, is drawn without
    /// colour. It is true while the session's `grayscale` is on.
    pub grayscale: bool,
}

/// Draws one grid tile: the media rectangle, the 1 px line around it, the
/// badges inside it, the selection outline outside it, and the rows of text
/// below it that the view preferences show. The library's grid and a
/// collection's editor draw their tiles with it. Each adds its own area that
/// receives the pointer.
pub(crate) fn tile_art(
    look: TileLook,
    prefs: &ViewPrefs,
    art: &mut Option<art::Art>,
    thumbnails: &Entity<Thumbnails>,
    window: &Window,
    cx: &App,
) -> Div {
    let theme = theme::theme(cx);
    let TileLook {
        tile,
        item,
        thumbnail,
        selected,
        thumb,
        original,
        alphas,
        magnifier,
        grayscale,
    } = look;
    let image_height = tile.h - prefs.text_height();
    let audio = item.mime.starts_with("audio/");
    let font = item.mime.starts_with("font/");
    let timed = item.mime.starts_with("video/") || audio;
    // Media that is shown whole keeps its own rectangle inside the tile. The
    // badges and the selection outline follow that rectangle, not the tile.
    let (contain, (x, y, w, h)) = media_rect(&tile, &item, prefs);
    // The picture fades in over its dominant colour, and then that colour
    // fades out under it. Without the second step the colour would stay
    // visible through the transparent parts of a picture.
    let placeholder = item
        .palette
        .first()
        .map(|color| {
            let color = format::lab_to_rgb(color.l, color.a, color.b);
            rgb(if grayscale {
                format::grayscale(color)
            } else {
                color
            })
        })
        .unwrap_or(theme.surface_2());
    let (image_alpha, placeholder_alpha) = alphas;
    let mut cell = div()
        .absolute()
        .left(px(tile.x))
        .top(px(tile.y))
        .w(px(tile.w))
        .h(px(tile.h));
    if selected {
        cell = cell
            .child(
                div()
                    .absolute()
                    .left(px(x - layout::SELECTION_EXTENT))
                    .top(px(y - layout::SELECTION_EXTENT))
                    .w(px(w + 2. * layout::SELECTION_EXTENT))
                    .h(px(h + 2. * layout::SELECTION_EXTENT))
                    .rounded(px(theme::TILE_RADIUS + layout::SELECTION_EXTENT))
                    .border_2()
                    .border_color(rgb(theme::GRID_SELECTION)),
            )
            // A 1 px line in the app's background colour is drawn between
            // the selection outline and the picture.
            .child(
                div()
                    .absolute()
                    .left(px(x - 1.))
                    .top(px(y - 1.))
                    .w(px(w + 2.))
                    .h(px(h + 2.))
                    .rounded(px(theme::TILE_RADIUS + 1.))
                    .border_1()
                    .border_color(theme.background()),
            );
    }
    let layer = || div().absolute().inset_0().rounded(px(theme::TILE_RADIUS));
    let mut media = div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .rounded(px(theme::TILE_RADIUS));
    let area = (tile.w, image_height);
    if font {
        // A font has no thumbnail. The lettered card is drawn in its place
        // without a fade-in. It is centred on the tile's image area and is
        // at most 72% of that area's width and of its height.
        let art = art::Art::for_color(art, theme.surface_2());
        let mark = (tile.w * 0.72).min(image_height * 0.72).min(art::FONT_SIZE);
        let side = art::FONT_SIZE + 2. * art::MARGIN;
        media = media
            .overflow_hidden()
            .child(layer().bg(placeholder))
            .child(art_at(
                art.font.clone(),
                (side, side),
                mark / art::FONT_SIZE,
                area,
                (x, y),
            ));
    } else if thumb == Thumb::Failed {
        // A thumbnail that could not be read or decoded is replaced by the
        // picture of a torn page on the theme's placeholder colour. The torn
        // page is at most 64% of the height of the tile's image area.
        let art = art::Art::for_color(art, theme.surface_2());
        let (art_width, art_height) = art::BROKEN_SIZE;
        let mark = (image_height * 0.64).min(art_height);
        media = media.overflow_hidden().bg(theme.surface_2()).child(art_at(
            art.broken.clone(),
            (art_width + 2. * art::MARGIN, art_height + 2. * art::MARGIN),
            mark / art_height,
            area,
            (x, y),
        ));
    } else {
        media = media.child(layer().bg(placeholder).opacity(placeholder_alpha));
        // When the tile is larger than its thumbnail, the original takes the
        // thumbnail's place without a second fade-in.
        let image = match original {
            Some(original) => img(original),
            None => img(thumbnail).image_cache(thumbnails),
        }
        .size_full()
        .rounded(px(theme::TILE_RADIUS))
        // A thumbnail whose shape differs from its item's (cover art, a
        // rotated video) is shown whole and not cut.
        .object_fit(if contain {
            ObjectFit::Contain
        } else {
            ObjectFit::Cover
        });
        media = if audio {
            // An audio item is drawn as a faint tint with its thumbnail over
            // it. The thumbnail is drawn without colour and at half opacity.
            media
                .child(
                    layer()
                        .bg(rgba(theme::AUDIO_TINT))
                        .opacity(image_alpha)
                        .child(layer().bg(linear_gradient(
                            180.,
                            linear_color_stop(rgba(theme::AUDIO_TINT), 0.),
                            linear_color_stop(rgba(theme::AUDIO_TINT_FAR), 1.),
                        ))),
                )
                .child(
                    image
                        .absolute()
                        .inset_0()
                        .grayscale(true)
                        .opacity(image_alpha * 0.5),
                )
        } else {
            media.child(image.grayscale(grayscale).opacity(image_alpha))
        };
        // A picture that fills a tile of another shape (the grid mode
        // with Fit Thumbnails on) is drawn larger than the tile. It is
        // clipped to the tile's rectangle here. GPUI clips to rectangles
        // only, so the tile's round corners are made by painting over
        // the corners in the colour behind the tile.
        if !contain {
            let behind = theme.background();
            media = media.overflow_hidden().child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        window.paint_drop_shadows_outside(
                            bounds,
                            Corners::all(px(theme::TILE_RADIUS)),
                            &[BoxShadow::new(px(0.), px(0.), behind.into())
                                .spread_radius(px(theme::TILE_RADIUS))],
                        );
                    },
                )
                .absolute()
                .inset_0(),
            );
        }
    }
    cell = cell.child(media.child(layer().border_1().border_color(theme.border())));
    let collection = item.kind == picto_library::RootKind::Collection;
    if prefs.show_extension_label {
        let label = format::format_label(&item.mime);
        if collection {
            cell = cell.child(
                ui::badge(0.)
                    .w(px(18.))
                    .justify_center()
                    .left(px(x + 5.))
                    .top(px(y + 5.))
                    .child(ui::icon("group", 14., rgba(theme::BADGE_TEXT))),
            );
        } else if !label.is_empty() {
            cell = cell.child(
                ui::badge(4.)
                    .left(px(x + 5.))
                    .top(px(y + 5.))
                    .child(ui::badge_text(label)),
            );
        }
    }
    let animated = item.mime == "image/gif" && item.frame_count.unwrap_or(0) > 1;
    // A collection has no duration of its own. Its cover can be a video, but
    // the collection may hold several videos, or pictures and one video, so
    // a duration on its tile would not say what it is the duration of.
    if let Some(duration) = item
        .duration_ms
        .filter(|ms| *ms > 0 && (timed || animated) && !collection)
    {
        cell = cell.child(
            ui::badge(2.)
                .right(px(tile.w - x - w + 5.))
                .top(px(y + 5.))
                .child(ui::badge_text(format::format_duration(duration))),
        );
    }
    if prefs.show_item_count && collection {
        cell = cell.child(
            ui::badge(4.)
                .left(px(x + 5.))
                .top(px(y + h - 18. - 5.))
                .child(ui::badge_text(item.media_count.to_string())),
        );
    }
    if prefs.show_name {
        cell = cell.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(image_height + 7.))
                .h(px(15.))
                .flex()
                .justify_center()
                .child(
                    // The name is cut with an ellipsis at the tile's width
                    // less 8 px. On a selected tile the name is white on a
                    // rounded background in the selection colour, which is
                    // 3 px wider than the name on each side.
                    div()
                        .px(px(3.))
                        .rounded(px(2.))
                        .when(selected, |pill| {
                            pill.bg(rgb(theme::GRID_SELECTION))
                                .text_color(rgb(theme::BADGE_STRONG_TEXT))
                        })
                        .whitespace_nowrap()
                        .line_height(px(15.))
                        .child(div().relative().top(px(CANVAS_TEXT_LIFT)).child({
                            let name = if item.name.is_empty() {
                                "Untitled".to_owned()
                            } else {
                                item.name.clone()
                            };
                            // With the view preference Show Extension on,
                            // the name is followed by a full stop and the
                            // file extension in capitals.
                            let extension = format::label_extension(&item.mime);
                            let label = if prefs.show_extension && !extension.is_empty() {
                                format!("{name}.{}", extension.to_uppercase())
                            } else {
                                name
                            };
                            let max = tile.w - 8.;
                            crate::text::truncate_to(
                                &label,
                                max,
                                |text| name_width(text, window),
                                // The whole name was already shaped in this
                                // frame, so shaping it again only looks it up.
                                || {
                                    let line = name_line(&label, window);
                                    let ellipsis = name_width("…", window);
                                    format::characters_before(&label, max, ellipsis, |at| {
                                        f32::from(line.x_for_index(at))
                                    })
                                },
                            )
                        })),
                ),
        );
    }
    if prefs.show_resolution
        && let (Some(width), Some(height)) = (item.width, item.height)
        && width > 0
        && height > 0
    {
        cell = cell.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(image_height
                    + prefs.show_name as u8 as f32 * layout::TEXT_ROW_HEIGHT
                    + 5.))
                .h(px(15.))
                .flex()
                .justify_center()
                .text(theme::TEXT_CAPTION)
                .line_height(px(15.))
                .text_color(theme.text_tertiary())
                .child(
                    div()
                        .relative()
                        .top(px(CANVAS_TEXT_LIFT))
                        .child(format!("{width} × {height}")),
                ),
        );
    }
    if magnifier {
        cell = cell.child(
            div()
                .absolute()
                .left(px(tile.w - marquee::MAGNIFIER_WIDTH))
                .top(px(image_height - marquee::MAGNIFIER_HEIGHT))
                .w(px(marquee::MAGNIFIER_WIDTH))
                .h(px(marquee::MAGNIFIER_HEIGHT))
                .rounded_tl(px(10.))
                .rounded_br(px(theme::TILE_RADIUS))
                .bg(rgba(theme::GRID_MAGNIFIER_BACKGROUND))
                .child(
                    svg()
                        .path("magnifier")
                        .size_full()
                        .text_color(rgba(theme::GRID_MAGNIFIER_GLYPH)),
                ),
        );
    }
    cell
}

impl GridView {
    /// Builds the element of one tile of the library's grid: the tile that
    /// [`tile_art`] draws, with an area over its picture that receives
    /// clicks.
    pub(super) fn tile(
        &mut self,
        data: TileData,
        prefs: &ViewPrefs,
        now: f64,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let TileData {
            index,
            ordinal,
            mut look,
        } = data;
        let (tile, in_selection) = (look.tile, look.selected);
        let image_height = tile.h - prefs.text_height();
        // The magnifier icon is drawn on the picture under the pointer. It
        // is not drawn on a video, and not until 80 ms
        // (`motion::MAGNIFIER_RESUME`) have passed since the grid last
        // scrolled or its zoom last changed.
        look.magnifier = self.hovered == Some(index)
            && !look.item.mime.starts_with("video/")
            && self.scrolled.elapsed() >= crate::motion::MAGNIFIER_RESUME;
        look.alphas = reveal::alphas(self.reveal.progress(look.item.root_id.0, now));
        look.grayscale = self.session.read(cx).grayscale;
        // For assistive technology the tile is an option of the grid's list.
        // Its name is the file's name, it is selected while it is in the
        // selection, and its position is its place in the whole destination.
        // Only the tiles that are drawn are in the tree. The tile the keyboard
        // is on is the one that assistive technology reads as focused.
        let described = crate::ui::a11y::active(window).then(|| {
            let session = self.session.read(cx);
            (
                SharedString::from(look.item.name.clone()),
                session.total as usize,
                session.focus_ordinal() == Some(ordinal),
            )
        });
        let cell = tile_art(look, prefs, &mut self.art, &self.thumbnails, window, cx);
        let mut hit = div().id(ordinal);
        if let Some((name, total, focused)) = described {
            hit = hit
                .role(Role::ListBoxOption)
                .aria_label(name)
                .aria_selected(in_selection)
                .aria_position_in_set(ordinal + 1)
                .aria_size_of_set(total.max(ordinal + 1));
            if focused {
                hit = hit.aria_active_descendant();
            }
        }
        // Only the picture receives the pointer. The rows for the name and the
        // resolution below it count as empty space.
        cell.child(
            hit.absolute()
                .left_0()
                .top_0()
                .w(px(tile.w))
                .h(px(image_height))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if std::mem::take(&mut this.swallow_tile_click) {
                        return;
                    }
                    let press = selection::Press::here(event.modifiers());
                    this.session.update(cx, |session, cx| match press {
                        selection::Press::Range { keep } => session.select_to(ordinal, keep, cx),
                        selection::Press::Toggle => session.toggle(ordinal, cx),
                        selection::Press::Plain => session.select(ordinal, cx),
                        // A secondary click opened the tile's menu when the
                        // button was pressed, so the click does nothing.
                        selection::Press::Menu => {}
                    });
                    if press == selection::Press::Menu {
                        return;
                    }
                    window.focus(&this.focus, cx);
                    if event.click_count() == 2 && press == selection::Press::Plain {
                        cx.emit(Activated {
                            ordinal,
                            middle: false,
                        });
                    }
                }))
                // A Control-click on macOS is a secondary click. It opens
                // the tile's menu, as a right-click does.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if selection::Press::here(event.modifiers) == selection::Press::Menu {
                            cx.stop_propagation();
                            this.tile_menu(ordinal, in_selection, event.position, window, cx);
                        }
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(Activated {
                            ordinal,
                            middle: true,
                        });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        // The event is stopped here so that it does not
                        // reach the grid's own right-click handler, which
                        // opens the menu for empty space.
                        cx.stop_propagation();
                        this.tile_menu(ordinal, in_selection, event.position, window, cx);
                    }),
                ),
        )
    }

    /// Emits `MenuRequested`, which asks the shell to open the tile's
    /// context menu at `position` (in window coordinates), and gives `focus`
    /// the keyboard focus. A right-click on a tile inside the selection keeps
    /// the selection. A right-click on another tile selects that tile first.
    fn tile_menu(
        &mut self,
        ordinal: usize,
        in_selection: bool,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !in_selection {
            self.session
                .update(cx, |session, cx| session.select(ordinal, cx));
        }
        cx.emit(MenuRequested(position));
        window.focus(&self.focus, cx);
    }
}
