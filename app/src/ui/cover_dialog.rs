//! Draws the dialog that sets a cover. It first shows the pictures that can
//! become the cover as a grid of squares, loaded 200 at a time. A click on
//! one shows that picture in a square frame, where the user drags it into
//! place and zooms it with the wheel or the slider. Confirm saves the framed
//! square as the cover. The dialog sets the cover of a subscription
//! (`screens/subscriptions/cover.rs`) and the cover of the library
//! (`shell/library_cover.rs`). [`CoverHost`] is what each of those two
//! owners provides, and [`cover_layer`] draws the dialog for either.
use crate::cover::Crop;
use crate::grid::originals::{Originals, THUMBNAIL_LONG_EDGE, Want};
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look as ButtonLook, modal::Size};
use crate::{theme, ui::modal::Presence};
use gpui::*;
use serde_json::Value;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

/// The squares of the grid are at least 104px wide with 8px between them,
/// which gives `floor((width + 8) / 112)` columns.
const CANDIDATE: f32 = 112.;
const GAP: f32 = 8.;
/// The zoom is a percentage. It runs from 100 to 300 in steps of 5.
const ZOOM_MIN: f32 = 100.;
const ZOOM_MAX: f32 = 300.;
const ZOOM_STEP: f32 = 5.;

/// One picture or video that can become the cover.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub media_item: i64,
    /// The content hash of its file.
    pub hash: String,
    /// The size of the original in pixels. A square until the size is known.
    pub size: (u32, u32),
    pub thumbnail: Option<PathBuf>,
}

/// Holds where a drag started: the pointer's position, and the focus the
/// picture had at that moment. It is `None` while no drag is in progress.
type Drag = Rc<Cell<Option<(Point<Pixels>, f32, f32)>>>;

/// Says whose cover the dialog sets: the cover of the subscription with this
/// id, or the cover of the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Subscription(i64),
    Library,
}

/// Holds the dialog's state. The owner of the dialog keeps it. For a
/// subscription, the Subscriptions screen keeps it in `Dialog::Cover`.
pub struct Cover {
    pub target: Target,
    pub candidates: Rc<Vec<Candidate>>,
    pub next: Option<Value>,
    pub loading: bool,
    pub loading_more: bool,
    pub selected: Option<Candidate>,
    pub crop: Crop,
    drag: Drag,
    list: UniformListScrollHandle,
    /// Loads the original file of the picture in the frame. It is made when
    /// a picture is first framed and dropped when the user goes back to the
    /// candidates, which frees the decoded picture.
    originals: Option<Entity<Originals>>,
    /// The content hash of the picture in the frame and the path of its
    /// original file, once that has been looked up. The path is `None` when
    /// the file is missing or is not a picture.
    original: Option<(String, Option<PathBuf>)>,
    /// The text above the squares (`instructions`), and the text shown when
    /// there is nothing to choose from (`empty`).
    instructions: &'static str,
    empty: &'static str,
}

impl Cover {
    /// Returns the state of a dialog for `target`. The state says that the
    /// candidates are loading, and the owner loads them. When `selected` is
    /// given, the dialog opens with that picture already in the frame.
    pub fn new(
        target: Target,
        selected: Option<Candidate>,
        instructions: &'static str,
        empty: &'static str,
    ) -> Self {
        Self {
            target,
            candidates: Rc::default(),
            next: None,
            loading: true,
            loading_more: false,
            selected,
            crop: Crop::default(),
            drag: Rc::default(),
            list: UniformListScrollHandle::new(),
            originals: None,
            original: None,
            instructions,
            empty,
        }
    }

    /// Returns the original of the picture in the frame, decoded for a frame
    /// `side` wide on a display with `scale` pixels per point. Returns
    /// `None` until the decode has finished; the frame shows the thumbnail
    /// until then.
    ///
    /// A thumbnail's long edge is 512 pixels, and the frame enlarges the
    /// picture up to three times, so the thumbnail alone looks blurred. The
    /// original is decoded once, at the number of pixels the frame shows at
    /// the largest zoom and never more than the file has. It is read after
    /// the picture has been in the frame for a fifth of a second, the wait
    /// the grid and the viewers also use.
    fn framed_original<V: 'static>(
        &mut self,
        side: f32,
        scale: f32,
        blobs: &Path,
        cx: &mut Context<V>,
    ) -> Option<Arc<RenderImage>> {
        let selected = self.selected.as_ref()?;
        let (width, height) = selected.size;
        // The size is not known, so the pixels needed cannot be worked out.
        if width <= 1 || height <= 1 {
            return None;
        }
        let shown = side * (ZOOM_MAX / 100.) / width.min(height) as f32 * scale.max(1.);
        let part = shown.min(1.);
        if width.max(height) as f32 * part <= THUMBNAIL_LONG_EDGE {
            return None;
        }
        if self.original.as_ref().map(|(hash, _)| hash) != Some(&selected.hash) {
            self.original = Some((selected.hash.clone(), original_in(blobs, &selected.hash)));
        }
        let path = self.original.as_ref()?.1.clone()?;
        let originals = self
            .originals
            .get_or_insert_with(|| {
                let originals = cx.new(Originals::new);
                // The dialog is drawn again when the decode finishes.
                cx.observe(&originals, |_, _, cx| cx.notify()).detach();
                originals
            })
            .clone();
        let want = Want {
            hash: selected.hash.clone(),
            path,
            size: (
                ((width as f32 * part).ceil() as u32).clamp(1, width),
                ((height as f32 * part).ceil() as u32).clamp(1, height),
            ),
        };
        let hash = want.hash.clone();
        originals.update(cx, |originals, cx| originals.plan(vec![want], cx));
        originals.read(cx).get(&hash)
    }
}

/// Returns the path of the original file with the content hash `hash` in a
/// library's `blobs` folder, when that file is a picture. Originals are
/// stored as
/// `blobs/f/<ab>/<cd>/<hash>.<extension>`, and a candidate does not carry
/// its file's type, so the file is found by its name without the extension.
/// A file whose extension is not a picture format (a video, for example) is
/// not returned: the frame then keeps the thumbnail.
fn original_in(blobs: &Path, hash: &str) -> Option<PathBuf> {
    if hash.len() < 4 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let folder = blobs.join("f").join(&hash[..2]).join(&hash[2..4]);
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .find(|path| {
            path.file_stem().and_then(|stem| stem.to_str()) == Some(hash)
                && image::ImageFormat::from_path(path).is_ok()
        })
}

/// The functions that an owner of the dialog provides.
pub trait CoverHost: Sized + 'static {
    /// Returns the state of the open dialog, or `None` when the dialog is
    /// closed.
    fn cover(&mut self) -> Option<&mut Cover>;
    /// Returns whether a save is in progress. Back and Confirm are disabled
    /// while it is.
    fn cover_busy(&self) -> bool;
    /// Returns the cache that the candidates' thumbnails are decoded into.
    fn cover_thumbnails(&self) -> Entity<Thumbnails>;
    /// Returns the `blobs` folder of the library that the candidates are in.
    fn cover_blobs(&self, cx: &App) -> PathBuf;
    /// Called when the grid has been scrolled near its end. Loads the next
    /// page.
    fn load_more_candidates(&mut self, cx: &mut Context<Self>);
    /// Called by the Confirm button. Saves the framed square as the cover.
    /// The dialog closes when the save has finished.
    fn save_cover(&mut self, cx: &mut Context<Self>);
    /// Called by the close button, by Escape and by a click on the backdrop.
    fn dismiss_cover(&mut self, cx: &mut Context<Self>);

    fn choose_candidate(&mut self, candidate: Candidate, cx: &mut Context<Self>) {
        if let Some(cover) = self.cover() {
            cover.crop = Crop::default();
            cover.selected = Some(candidate);
            cx.notify();
        }
    }

    fn set_crop(&mut self, change: impl FnOnce(&mut Crop), cx: &mut Context<Self>) {
        if let Some(cover) = self.cover() {
            change(&mut cover.crop);
            cx.notify();
        }
    }

    /// Called by the Back button. Shows the candidates again and drops the
    /// decoded original.
    fn unselect_candidate(&mut self, cx: &mut Context<Self>) {
        if let Some(cover) = self.cover() {
            cover.selected = None;
            cover.originals = None;
            cover.original = None;
            cx.notify();
        }
    }
}

/// Adds a page of candidates to the dialog. When the dialog still has no
/// candidates after this page and there are more pages, it returns the cursor
/// of the next page, which the caller then loads.
pub fn add_page(
    cover: &mut Cover,
    candidates: Vec<Candidate>,
    next: Option<Value>,
) -> Option<Value> {
    cover.loading = false;
    cover.loading_more = false;
    let mut all = cover.candidates.as_ref().clone();
    all.extend(candidates);
    let again = (all.is_empty() && next.is_some())
        .then(|| next.clone())
        .flatten();
    if again.is_some() {
        cover.loading_more = true;
    }
    cover.candidates = Rc::new(all);
    cover.next = next;
    again
}

/// Returns the number of columns for a grid `width` wide, which is
/// `floor((width + 8) / 112)` and at least one, and the side of one square.
pub fn columns(width: f32) -> (usize, f32) {
    let count = (((width + GAP) / CANDIDATE).floor() as usize).max(1);
    let side = (width - (count - 1) as f32 * GAP) / count as f32;
    (count, side)
}

/// Returns the zoom after one wheel event. `delta_y` is positive when the
/// wheel turns down. Each event changes the zoom by 5 points: the picture is
/// zoomed in when the wheel turns up, and out when it turns down.
pub fn wheel_zoom(zoom: f32, delta_y: f32) -> f32 {
    (zoom - delta_y.signum() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Returns the slider's value `zoom` rounded to the nearest step and kept
/// between the smallest and the largest zoom.
pub fn stepped_zoom(zoom: f32) -> f32 {
    ((zoom / ZOOM_STEP).round() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// Returns the focus after a drag by `moved` that started at the focus
/// `from`, in a frame `side` wide. The picture follows the pointer, but only
/// as far as the picture extends past the frame.
pub fn dragged(
    from: (f32, f32),
    moved: (f32, f32),
    size: (u32, u32),
    zoom: f32,
    side: f32,
) -> (f32, f32) {
    let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
    let zoom = (zoom / 100.).max(1.);
    // How many times wider and taller than the frame the picture is drawn.
    // `cover::square` uses the same two ratios.
    let across = aspect.max(1.) * zoom;
    let down = (1. / aspect).max(1.) * zoom;
    let along = |focus: f32, moved: f32, ratio: f32| {
        let travel = (ratio - 1.).max(0.) * side;
        if travel == 0. {
            focus
        } else {
            (focus - moved / travel * 1000.).round().clamp(0., 1000.)
        }
    };
    (along(from.0, moved.0, across), along(from.1, moved.1, down))
}

/// Draws the dialog: the candidates, or the chosen picture in its frame.
pub fn cover_layer<V: CoverHost>(
    host: &mut V,
    presence: Presence,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let busy = host.cover_busy();
    let view = cx.entity().downgrade();
    let dismiss = {
        let view = view.clone();
        move |_: &mut Window, cx: &mut App| {
            let _ = view.update(cx, |this, cx| this.dismiss_cover(cx));
        }
    };
    let cache = host.cover_thumbnails();
    let blobs = host.cover_blobs(cx);
    let Some(cover) = host.cover() else {
        return Empty.into_any_element();
    };
    let theme = theme::theme(cx);
    let viewport = window.viewport_size();
    // The frame is a square whose side is 58% of the window's height and at
    // most 420px.
    let side = (f32::from(viewport.height) * 0.58).min(420.);
    let inner = side - 2.;
    let original = cover.framed_original(inner, window.scale_factor(), &blobs, cx);
    match &cover.selected {
        Some(selected) => {
            let (x, y, square) =
                crate::cover::square(selected.size.0 as f32, selected.size.1 as f32, cover.crop);
            let scale = inner / square.max(1.);
            // The original takes the thumbnail's place when it is ready.
            let image = match original {
                Some(original) => Some(img(original)),
                None => selected
                    .thumbnail
                    .clone()
                    .map(|path| img(path).image_cache(&cache)),
            }
            .map(|image| {
                image
                    .absolute()
                    .left(px(-x * scale))
                    .top(px(-y * scale))
                    .w(px(selected.size.0 as f32 * scale))
                    .h(px(selected.size.1 as f32 * scale))
                    .object_fit(ObjectFit::Fill)
            });
            let (crop, size, drag) = (cover.crop, selected.size, cover.drag.clone());
            let frame = div()
                .id("media-cover-crop")
                .relative()
                .size(px(side))
                .flex_none()
                .rounded(px(theme::RADIUS_LG))
                .bg(theme.surface_1())
                .cursor(if drag.get().is_some() {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
                .child(
                    div()
                        .absolute()
                        .inset(px(1.))
                        .overflow_hidden()
                        .children(image),
                )
                // The border is drawn over the picture, because GPUI cannot
                // cut the picture to the round corners.
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(theme::RADIUS_LG))
                        .border_1()
                        .border_color(theme.border()),
                )
                .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    if f32::from(delta.y) != 0. {
                        // GPUI's delta is how far the content moves, which is
                        // negative when the wheel turns down. `wheel_zoom`
                        // takes a value that is positive then.
                        this.set_crop(
                            |crop| crop.zoom = wheel_zoom(crop.zoom, -f32::from(delta.y)),
                            cx,
                        );
                    }
                    cx.stop_propagation();
                }))
                // The drag follows the pointer anywhere in the window until
                // the button is released. For that reason the handlers are
                // on the window and not on the frame.
                .child(
                    canvas(|_, _, _| {}, {
                        let view = view.clone();
                        move |bounds, _, window, _| {
                            let pressed = drag.clone();
                            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble
                                    && event.button == MouseButton::Left
                                    && bounds.contains(&event.position)
                                {
                                    pressed.set(Some((event.position, crop.focus_x, crop.focus_y)));
                                    cx.stop_propagation();
                                }
                            });
                            let moving = drag.clone();
                            let view = view.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture {
                                    return;
                                }
                                let Some((start, focus_x, focus_y)) = moving.get() else {
                                    return;
                                };
                                let moved = event.position - start;
                                let (focus_x, focus_y) = dragged(
                                    (focus_x, focus_y),
                                    (f32::from(moved.x), f32::from(moved.y)),
                                    size,
                                    crop.zoom,
                                    f32::from(bounds.size.width),
                                );
                                let _ = view.update(cx, |this, cx| {
                                    this.set_crop(
                                        |crop| {
                                            crop.focus_x = focus_x;
                                            crop.focus_y = focus_y;
                                        },
                                        cx,
                                    )
                                });
                            });
                            let released = drag.clone();
                            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                                if phase == DispatchPhase::Capture {
                                    released.set(None);
                                }
                            });
                        }
                    })
                    .absolute()
                    .inset_0(),
                );
            let zoom_view = view.clone();
            // The zoom row is 420px wide, or as wide as the dialog's body when
            // the body is narrower. The slider is 320px wide and the label takes
            // the width that is left.
            let zoom = div()
                .w(px(420.))
                .max_w_full()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(12.))
                .text_color(theme.text_secondary())
                .child(div().flex_1().min_w_0().child(t("Zoom")))
                .child(
                    div().w(px(320.)).flex_none().child(
                        ui::slider::slider(
                            "media-cover-zoom",
                            f64::from(cover.crop.zoom),
                            f64::from(ZOOM_MIN),
                            f64::from(ZOOM_MAX),
                            f64::from(ZOOM_STEP),
                        )
                        .look(ui::slider::Look::Dialog)
                        .on_change(move |zoom, cx| {
                            let _ = zoom_view.update(cx, |this, cx| {
                                this.set_crop(|crop| crop.zoom = stepped_zoom(zoom as f32), cx)
                            });
                        }),
                    ),
                );
            let back = view.clone();
            ui::modal("media-cover", t("Set cover photo"), presence)
                .size(Size::Medium)
                .focus(&focus)
                .on_close(dismiss)
                .child(
                    div()
                        .min_h(px(220.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(16.))
                        .child(frame)
                        .child(zoom),
                )
                .footer(
                    ui::action_button("media-cover-back", ButtonLook::Standard)
                        .label(t("Back"))
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            let _ = back.update(cx, |this, cx| this.unselect_candidate(cx));
                        }),
                )
                .footer(
                    ui::action_button("media-cover-confirm", ButtonLook::Primary)
                        .label(t("Confirm"))
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.save_cover(cx))),
                )
                .into_any_element()
        }
        None => {
            let message = |text: &'static str| {
                div()
                    .min_h(px(180.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text_color(theme.text_secondary())
                    .child(text)
                    .into_any_element()
            };
            let body = if cover.loading {
                message(t("Loading…"))
            } else if cover.candidates.is_empty() {
                message(cover.empty)
            } else {
                // The width of the body, less its padding and less two
                // scrollbar gutters: the body's and the grid's.
                let width = Size::Large.content_width() - ui::scrollbar::GUTTER;
                let (count, cell) = columns(width);
                let row = cell + GAP;
                let rows = cover.candidates.len().div_ceil(count);
                let height = (rows as f32 * row).min((f32::from(viewport.height) * 0.58).min(560.));
                let candidates = cover.candidates.clone();
                let more = cover.next.is_some();
                let pick = view.clone();
                let list = uniform_list("media-cover-candidates", rows, move |range, _, cx| {
                    if more && range.end + 3 >= rows {
                        let view = pick.clone();
                        cx.defer(move |cx| {
                            let _ = view.update(cx, |this, cx| this.load_more_candidates(cx));
                        });
                    }
                    let theme = theme::theme(cx);
                    // The colour of the dialog's panel as one opaque colour:
                    // the panel's translucent fill over the window's
                    // background.
                    let glass = theme.glass();
                    let behind =
                        theme::blend(theme.background(), Rgba { a: 1., ..glass }, glass.a);
                    range
                        .map(|index| {
                            let mut line = div().h(px(row)).flex().gap(px(GAP));
                            for candidate in candidates.iter().skip(index * count).take(count) {
                                let chosen = candidate.clone();
                                let pressed = (candidate.clone(), pick.clone());
                                let pick = pick.clone();
                                let id = ElementId::from((
                                    "media-cover-candidate",
                                    candidate.media_item as usize,
                                ));
                                // A square is a button. When it has the
                                // keyboard focus its border is the primary
                                // colour, as it is under the pointer.
                                let ring = ui::focus::Ring {
                                    width: 1.,
                                    offset: -1.,
                                    color: theme.primary(),
                                };
                                // A picture that fills a square of another
                                // shape is drawn larger than the square. It
                                // is cut to the square (`overflow_hidden`).
                                // GPUI cuts to rectangles only, so the
                                // square's four round corners are then
                                // painted over in the colour of the dialog
                                // behind the square. The border is drawn
                                // last, on top of the picture.
                                let tile = div()
                                    .id(id.clone())
                                    .group("media-cover-candidate")
                                    .role(Role::Button)
                                    .aria_label(t("Set cover photo"))
                                    .relative()
                                    .size(px(cell))
                                    .flex_none()
                                    .rounded(px(theme::RADIUS_MD))
                                    .overflow_hidden()
                                    .bg(theme.surface_1())
                                    .cursor_pointer()
                                    .children(candidate.thumbnail.clone().map(|path| {
                                        img(path)
                                            .image_cache(&cache)
                                            .size_full()
                                            .object_fit(ObjectFit::Cover)
                                    }))
                                    .child(
                                        canvas(
                                            |_, _, _| {},
                                            move |bounds, _, window, _| {
                                                window.paint_drop_shadows_outside(
                                                    bounds,
                                                    Corners::all(px(theme::RADIUS_MD)),
                                                    &[BoxShadow::new(px(0.), px(0.), behind.into())
                                                        .spread_radius(px(theme::RADIUS_MD))],
                                                );
                                            },
                                        )
                                        .absolute()
                                        .inset_0(),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded(px(theme::RADIUS_MD))
                                            .border_1()
                                            .border_color(theme.border_secondary())
                                            .group_hover("media-cover-candidate", move |style| {
                                                style.border_color(theme.primary())
                                            }),
                                    )
                                    .on_click(move |_, _, cx| {
                                        let chosen = chosen.clone();
                                        let _ = pick.update(cx, |this, cx| {
                                            this.choose_candidate(chosen, cx)
                                        });
                                    });
                                line = line.child(
                                    ui::focus::stop(id, tile, move |_, _, cx| {
                                        let (chosen, pick) = pressed.clone();
                                        let _ = pick.update(cx, |this, cx| {
                                            this.choose_candidate(chosen, cx)
                                        });
                                    })
                                    .ring(
                                        ring,
                                        theme::RADIUS_MD,
                                        1.,
                                    ),
                                );
                            }
                            line
                        })
                        .collect()
                })
                .h(px(height))
                .track_scroll(&cover.list);
                let scroll = cover.list.0.borrow().base_handle.clone();
                let loading_more = cover.loading_more;
                div()
                    .relative()
                    .mt(px(12.))
                    .pr(px(ui::scrollbar::GUTTER))
                    .child(list)
                    .child(ui::scrollbar("media-cover-scrollbar", &scroll, window, cx))
                    .children(loading_more.then(|| {
                        // The "Loading more…" label stays at the bottom
                        // edge of the grid while the grid scrolls.
                        div()
                            .absolute()
                            .bottom(px(4.))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(theme::RADIUS_SM))
                                    .bg(theme.glass())
                                    .text(theme::TEXT_SM)
                                    .text_color(theme.text_secondary())
                                    .child(t("Loading more…")),
                            )
                    }))
                    .into_any_element()
            };
            ui::modal("media-cover", t("Set cover photo"), presence)
                .size(Size::Large)
                .focus(&focus)
                .on_close(dismiss)
                .child(
                    div()
                        .min_h(px(220.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text(theme::TEXT_MD)
                                .line_height(px(theme::LINE_HEIGHT_MD))
                                .text_color(theme.text_secondary())
                                .child(cover.instructions),
                        )
                        .child(body),
                )
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{columns, dragged, stepped_zoom, wheel_zoom};

    #[test]
    fn the_grid_fits_as_many_columns_as_the_earlier_versions_grid() {
        // The grid of the large dialog is 630px wide inside its gutter.
        let (count, side) = columns(630.);
        assert_eq!(count, 5);
        assert!((side - 119.6).abs() < 0.01);
        assert_eq!(columns(50.).0, 1);
    }

    #[test]
    fn zoom_moves_in_steps_between_its_bounds() {
        assert_eq!(wheel_zoom(100., 3.), 100.);
        assert_eq!(wheel_zoom(100., -3.), 105.);
        assert_eq!(wheel_zoom(300., -1.), 300.);
        assert_eq!(stepped_zoom(162.), 160.);
        assert_eq!(stepped_zoom(999.), 300.);
    }

    #[test]
    fn a_drag_moves_the_focus_only_where_the_image_reaches_past_the_square() {
        // A picture twice as wide as it is tall in a 420px frame extends 420px
        // past the frame horizontally and not at all vertically.
        assert_eq!(
            dragged((500., 500.), (-210., 40.), (2000, 1000), 100., 420.),
            (1000., 500.)
        );
        assert_eq!(
            dragged((500., 500.), (42., 0.), (2000, 1000), 100., 420.),
            (400., 500.)
        );
        // A zoomed square picture extends past the frame in both directions.
        assert_eq!(
            dragged((500., 500.), (0., -42.), (800, 800), 200., 420.),
            (500., 600.)
        );
    }
}
