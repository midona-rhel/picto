//! `MediaCoverDialog`: media 200 at a time in a grid of squares; a click
//! frames the chosen image in a square that is dragged to place and zoomed
//! with the wheel or the slider, and Confirm saves it. A subscription's
//! cover (`SubscriptionCoverDialog`: `subscriptions.cover.candidates`,
//! `subscriptions.cover.set`) and the library's (`LibraryCoverDialogHost`)
//! both use it: [`CoverHost`] is what each owner provides, and
//! [`cover_layer`] draws the dialog for any of them.
use crate::cover::Crop;
use crate::grid::thumbnails::Thumbnails;
use crate::theme::Text as _;
use crate::ui::{self, action_button::Look as ButtonLook, modal::Size};
use crate::{theme, ui::modal::Presence};
use gpui::*;
use serde_json::Value;
use std::{cell::Cell, path::PathBuf, rc::Rc};

/// `.candidate` squares are at least 104px with 8px between them
/// (`floor((width + 8) / 112)` columns).
const CANDIDATE: f32 = 112.;
const GAP: f32 = 8.;
/// `zoomPercent` runs from 100 to 300 in steps of 5.
const ZOOM_MIN: f32 = 100.;
const ZOOM_MAX: f32 = 300.;
const ZOOM_STEP: f32 = 5.;

/// One medium that can become the cover.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub media_item: i64,
    /// Its file's content hash.
    pub hash: String,
    /// The source's size; a square until it is known.
    pub size: (u32, u32),
    pub thumbnail: Option<PathBuf>,
}

/// Where a drag started: the pointer and the focus it moved from.
type Drag = Rc<Cell<Option<(Point<Pixels>, f32, f32)>>>;

/// Whose cover the dialog sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Subscription(i64),
    Library,
}

/// The dialog's state, kept by its owner (in the Subscriptions screen's
/// `Dialog::Cover` for a subscription).
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
    /// The text over the candidates, and what shows when there are none.
    instructions: &'static str,
    empty: &'static str,
}

impl Cover {
    /// A dialog for `target` that starts loading its candidates, framing
    /// `selected` at once when it is given (`initialCandidate`).
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
            instructions,
            empty,
        }
    }
}

/// What an owner of the dialog provides.
pub trait CoverHost: Sized + 'static {
    /// The open dialog's state.
    fn cover(&mut self) -> Option<&mut Cover>;
    /// A save is on its way: Back and Confirm wait.
    fn cover_busy(&self) -> bool;
    /// The cache the candidates' thumbnails decode into.
    fn cover_thumbnails(&self) -> Entity<Thumbnails>;
    /// The grid has scrolled near its end: the next page.
    fn load_more_candidates(&mut self, cx: &mut Context<Self>);
    /// Confirm: the framed square is saved, and the dialog closes once it
    /// has been.
    fn save_cover(&mut self, cx: &mut Context<Self>);
    /// The close button, Escape and the backdrop.
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

    /// Back: the candidates again.
    fn unselect_candidate(&mut self, cx: &mut Context<Self>) {
        if let Some(cover) = self.cover() {
            cover.selected = None;
            cx.notify();
        }
    }
}

/// Adds a page of candidates to the dialog: a first page with nothing on it
/// asks for the next while there is more (the cursor to read on with).
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

/// `floor((width + 8) / 112)` columns, at least one, and their side.
pub fn columns(width: f32) -> (usize, f32) {
    let count = (((width + GAP) / CANDIDATE).floor() as usize).max(1);
    let side = (width - (count - 1) as f32 * GAP) / count as f32;
    (count, side)
}

/// The zoom after a wheel turn of `delta_y` as the browser reports it
/// (`deltaY`, positive for a turn down): 5 points per event, in for a turn
/// up.
pub fn wheel_zoom(zoom: f32, delta_y: f32) -> f32 {
    (zoom - delta_y.signum() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The slider's zoom, to the nearest step.
pub fn stepped_zoom(zoom: f32) -> f32 {
    ((zoom / ZOOM_STEP).round() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The focus after dragging by `moved` over a viewport `side` wide: the
/// image follows the pointer, as far as it reaches past the square.
pub fn dragged(
    from: (f32, f32),
    moved: (f32, f32),
    size: (u32, u32),
    zoom: f32,
    side: f32,
) -> (f32, f32) {
    let aspect = size.0.max(1) as f32 / size.1.max(1) as f32;
    let zoom = (zoom / 100.).max(1.);
    // `subscriptionCoverGeometry`'s width and height ratios.
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

/// The dialog: the candidates, or the chosen one being framed.
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
    let Some(cover) = host.cover() else {
        return Empty.into_any_element();
    };
    let theme = theme::theme(cx);
    let viewport = window.viewport_size();
    match &cover.selected {
        Some(selected) => {
            // `.cropViewport`: `min(58vh, 420px)` square, no wider than
            // the body.
            let side = (f32::from(viewport.height) * 0.58).min(420.);
            let inner = side - 2.;
            let (x, y, square) =
                crate::cover::square(selected.size.0 as f32, selected.size.1 as f32, cover.crop);
            let scale = inner / square.max(1.);
            let image = selected.thumbnail.clone().map(|path| {
                img(path)
                    .image_cache(&cache)
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
                // The border over the image, which GPUI cannot clip to
                // the rounded corners.
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
                        // GPUI's delta is the content's movement, the
                        // opposite of the browser's `deltaY`.
                        this.set_crop(
                            |crop| crop.zoom = wheel_zoom(crop.zoom, -f32::from(delta.y)),
                            cx,
                        );
                    }
                    cx.stop_propagation();
                }))
                // The drag follows the pointer anywhere in the window
                // until the button comes up, as pointer capture does.
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
            // `.zoomRow`: `auto minmax(180px, 320px)` across
            // `min(100%, 420px)`, the label taking what is left.
            let zoom = div()
                .w(px(420.))
                .max_w_full()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(12.))
                .text_color(theme.text_secondary())
                .child(div().flex_1().min_w_0().child("Zoom"))
                .child(
                    div().w(px(320.)).flex_none().child(
                        ui::slider::slider(
                            "media-cover-zoom",
                            f64::from(cover.crop.zoom),
                            f64::from(ZOOM_MIN),
                            f64::from(ZOOM_MAX),
                            f64::from(ZOOM_STEP),
                        )
                        .dialog()
                        .on_change(move |zoom, cx| {
                            let _ = zoom_view.update(cx, |this, cx| {
                                this.set_crop(|crop| crop.zoom = stepped_zoom(zoom as f32), cx)
                            });
                        }),
                    ),
                );
            let back = view.clone();
            ui::modal("media-cover", "Set cover photo", presence)
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
                        .label("Back")
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            let _ = back.update(cx, |this, cx| this.unselect_candidate(cx));
                        }),
                )
                .footer(
                    ui::action_button("media-cover-confirm", ButtonLook::Primary)
                        .label("Confirm")
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
                message("Loading…")
            } else if cover.candidates.is_empty() {
                message(cover.empty)
            } else {
                // The body's width less its padding and both gutters:
                // the body's and the grid's own.
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
                                // `.candidate` is a `<button>`; with the
                                // keyboard its border turns the primary
                                // colour, as under the pointer.
                                let ring = ui::focus::Ring {
                                    width: 1.,
                                    offset: -1.,
                                    color: theme.primary(),
                                };
                                let tile = div()
                                    .id(id.clone())
                                    .relative()
                                    .size(px(cell))
                                    .flex_none()
                                    .rounded(px(theme::RADIUS_MD))
                                    .border_1()
                                    .border_color(theme.border_secondary())
                                    .bg(theme.surface_1())
                                    .cursor_pointer()
                                    .hover(move |style| style.border_color(theme.primary()))
                                    .children(candidate.thumbnail.clone().map(|path| {
                                        img(path)
                                            .image_cache(&cache)
                                            .size_full()
                                            .rounded(px(theme::RADIUS_MD - 1.))
                                            .object_fit(ObjectFit::Cover)
                                    }))
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
                        // `.loadingMore`: sticky at the grid's foot.
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
                                    .child("Loading more…"),
                            )
                    }))
                    .into_any_element()
            };
            ui::modal("media-cover", "Set cover photo", presence)
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
    fn the_grid_fits_as_many_columns_as_the_electron_one() {
        // The large dialog's grid is 630px inside its gutter.
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
        // A landscape image twice as wide as tall in a 420px square: it
        // reaches 420px past it across, and not at all down.
        assert_eq!(
            dragged((500., 500.), (-210., 40.), (2000, 1000), 100., 420.),
            (1000., 500.)
        );
        assert_eq!(
            dragged((500., 500.), (42., 0.), (2000, 1000), 100., 420.),
            (400., 500.)
        );
        // Zoomed, a square image reaches past it both ways.
        assert_eq!(
            dragged((500., 500.), (0., -42.), (800, 800), 200., 420.),
            (500., 600.)
        );
    }
}
