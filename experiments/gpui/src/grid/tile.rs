//! One grid tile: what it shows and how it is drawn (`drawBase.ts`), shared
//! by the library's grid and a collection's editor, and the grid's own tile
//! with the hit area that takes the pointer.
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

/// The canvas centres a tile's text rows on the middle of the font's em box
/// (`textBaseline = 'middle'`) at fractional positions; GPUI centres the
/// line's ascent and descent and lands half a point lower.
const CANVAS_TEXT_LIFT: f32 = -0.5;

/// A tile name shaped in the grid's 13px UI font.
pub(super) fn name_line(text: &str, window: &Window) -> ShapedLine {
    crate::ui::shape_line(
        text.to_owned(),
        font(theme::FONT_UI),
        theme::TEXT_MD,
        black(),
        window,
    )
}

/// A tile name's width in the grid's 13px UI font.
pub(super) fn name_width(text: &str, window: &Window) -> f32 {
    f32::from(name_line(text, window).width)
}

/// The item's aspect ratio when it is known. Audio without dimensions is
/// drawn two to one.
pub(super) fn aspect(item: &RootSummary) -> Option<f64> {
    match (item.width, item.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            Some(f64::from(width) / f64::from(height))
        }
        _ if item.mime.starts_with("audio/") => Some(2.),
        _ => None,
    }
}

/// A visible tile, copied out of the session for the visible tiles only.
pub(super) struct TileData {
    /// The tile in the window's layout.
    pub index: usize,
    /// The item's place in the destination.
    pub ordinal: usize,
    /// What it draws, placed in the whole destination's height.
    pub look: TileLook,
}

/// Whether a tile letterboxes its media, and the media's rect inside the
/// tile's image area. Only the grid mode letterboxes by choice; waterfall and
/// justified tiles already have their item's shape. Video and audio always
/// do. An item of unknown shape takes the whole area.
pub(super) fn media_rect(
    tile: &layout::Tile,
    item: &RootSummary,
    prefs: &ViewPrefs,
) -> (bool, (f32, f32, f32, f32)) {
    let image_height = tile.h - prefs.text_height();
    let timed = item.mime.starts_with("video/") || item.mime.starts_with("audio/");
    let contain = (prefs.mode == ViewMode::Grid && !prefs.fit_cover) || timed;
    let rect = aspect(item)
        .filter(|_| contain)
        .and_then(|aspect| {
            crate::viewer::camera::contain((aspect as f32, 1.), (tile.w, image_height))
        })
        .unwrap_or((0., 0., tile.w, image_height));
    (contain, rect)
}

/// Places artwork of `size` (drawing units, `art::MARGIN` included) drawn at
/// `scale`, centred on the tile's image area, inside the media rect at
/// (`x`, `y`).
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

/// What one tile shows, for [`tile_art`].
pub(crate) struct TileLook {
    pub tile: layout::Tile,
    pub item: RootSummary,
    pub thumbnail: PathBuf,
    pub selected: bool,
    pub thumb: Thumb,
    /// The original, decoded for the tile's size, once it is worth having.
    pub original: Option<std::sync::Arc<RenderImage>>,
    /// The image's and its placeholder's opacity, as the reveal has them.
    pub alphas: (f32, f32),
    /// The pointer rests on the image: the magnifier marks it.
    pub magnifier: bool,
    /// Grayscale (`gridGrayscaleAtom`): the whole tile without colour, its
    /// placeholder included.
    pub grayscale: bool,
}

/// One grid tile, drawn to the rules of the Electron canvas grid
/// (`drawBase.ts`): the media rect, its hairline ring, badges inside the
/// rect, the selection stroke outside it, and optional text rows below. The
/// library's grid and a collection's editor (`CanvasGrid` both) draw their
/// tiles with it; each adds the hit area that takes the pointer.
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
    // Letterboxed media keeps its own rect inside the tile; badges and the
    // selection stroke follow that rect, not the tile.
    let (contain, (x, y, w, h)) = media_rect(&tile, &item, prefs);
    // The image fades in over its dominant colour, then that colour fades
    // away beneath it, which matters for images with transparency.
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
            // A hairline of the app background between stroke and image.
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
        // A font has no thumbnail: its card shows at once, centred on
        // the tile and at most 72% of it.
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
        // The torn page, at most 64% of the tile's height, on the plain
        // placeholder colour.
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
        // The original, where the tile outgrew its thumbnail, takes the
        // thumbnail's place with no second reveal.
        let image = match original {
            Some(original) => img(original),
            None => img(thumbnail).image_cache(thumbnails),
        }
        .size_full()
        .rounded(px(theme::TILE_RADIUS))
        // A thumbnail of another shape than its item (cover art, a
        // rotated video) is fitted, not cropped.
        .object_fit(if contain {
            ObjectFit::Contain
        } else {
            ObjectFit::Cover
        });
        media = if audio {
            // Audio: a faint tint, and the cover art in grey at half
            // strength over it.
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
        // with Fit Thumbnails on) is drawn larger than the tile: cut it
        // to the tile, and to the tile's round corners. GPUI clips to
        // rectangles only, so the corners are painted over in the
        // colour behind the tile.
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
    if let Some(duration) = item.duration_ms.filter(|ms| *ms > 0 && (timed || animated)) {
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
                    // The name truncates to the tile less 8px as the
                    // canvas measures it; selected, it is white on a pill
                    // of the selection colour 3px wider either side.
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
                            // Show Extension adds `.EXT` after it.
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
                                // The whole name was shaped this frame
                                // already: shaping it again is a lookup.
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
    /// One grid tile ([`tile_art`]) and the image's hit area, which takes
    /// the clicks.
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
        // The magnifier marks the hovered image, except on video and while
        // the grid is scrolling.
        look.magnifier = self.hovered == Some(index)
            && !look.item.mime.starts_with("video/")
            && self.scrolled.elapsed() >= crate::motion::MAGNIFIER_RESUME;
        look.alphas = reveal::alphas(self.reveal.progress(look.item.root_id.0, now));
        look.grayscale = self.session.read(cx).grayscale;
        // An option of the grid's list, named by its file, selected while in
        // the selection, at its place in the whole destination. Only the
        // tiles drawn are in the tree; the one the keyboard is on is what
        // assistive technology reads as focused.
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
        // Only the image takes the pointer: the name and resolution rows
        // below it are empty space, as in the canvas grid's hit test.
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
                        // Opened its menu as it went down.
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
                // A secondary click (Control on macOS) is a right-click.
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
                        // The grid's own right-click is for empty space.
                        cx.stop_propagation();
                        this.tile_menu(ordinal, in_selection, event.position, window, cx);
                    }),
                ),
        )
    }

    /// A tile's menu, at a window position. A right-click inside the
    /// selection keeps it; elsewhere it selects the tile first.
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
