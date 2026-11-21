use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

/// What a tooltip shows: a label and the keys of the control's shortcut, each
/// key in its own badge.
#[derive(Clone)]
pub struct Tip {
    label: SharedString,
    keys: Vec<SharedString>,
    /// Whether the tooltip opens above the control and not below it.
    above: bool,
    /// Whether the tooltip is drawn as the simpler bubble that Settings uses
    /// for its theme swatches; see [`Tip::plain`].
    plain: bool,
}

pub fn tip(label: impl Into<SharedString>) -> Tip {
    Tip::new(label)
}

/// Gives a control a tooltip. The control must not have its own `on_hover`
/// handler. A control that has one keeps a [`Hover`] itself and adds the
/// element that [`Hover::layer`] returns.
pub fn attach(
    control: Stateful<Div>,
    id: impl Into<ElementId>,
    tip: Tip,
    extent: f32,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let hint = window.use_keyed_state(id.into(), cx, |_, _| Hover::default());
    if hint.read(cx).animating() {
        window.request_animation_frame();
    }
    let layer = hint.read(cx).layer(&tip, extent, cx);
    control
        .relative()
        .on_hover(move |hovered, _, cx| hint.update(cx, |hint, cx| hint.set(*hovered, cx)))
        .children(layer)
}

impl Tip {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            keys: Vec::new(),
            above: false,
            plain: false,
        }
    }

    /// Returns the tooltip's label. For a control that shows only an icon, the
    /// label is also the control's name for assistive technology.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// Makes this the simpler bubble that Settings uses for its theme
    /// swatches. It opens above the control, in the small text size, with a
    /// top edge in `Theme::contrast(15.)`. While it appears it grows from 95%
    /// of its size and does not move. For it to open as soon as the pointer
    /// arrives, the control passes hover changes to [`Hover::set_now`].
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self.above = true;
        self
    }

    /// Makes the tooltip open above the control. This is for controls along
    /// the window's bottom edge.
    pub fn above(mut self) -> Self {
        self.above = true;
        self
    }

    /// Shows the keys that the shortcut `id` of the shortcut registry is bound
    /// to now, one badge per key. When the user binds the shortcut to other
    /// keys, the tooltip shows the new keys.
    pub fn shortcut(mut self, id: &str, cx: &App) -> Self {
        self.keys = crate::keymap::badges(id, cx);
        self
    }

    /// Shows the keys that the shortcut `id` of the shortcut registry is bound
    /// to now, written as one text in a single badge.
    pub fn shortcut_written(mut self, id: &str, cx: &App) -> Self {
        self.keys = crate::keymap::hint(id, cx).into_iter().collect();
        self
    }
}

/// The height of a tooltip's bubble. `WINDOW_MARGIN` is the smallest distance
/// between a tooltip and the window's edge.
const BUBBLE_HEIGHT: f32 = 24.;
const WINDOW_MARGIN: f32 = 5.;

/// Holds the state of one control's tooltip between frames. The tooltip opens
/// once the pointer has stayed on the control for 400 ms, and closes when the
/// pointer leaves. It takes 150 ms to appear and 150 ms to disappear.
#[derive(Default)]
pub struct Hover {
    hovered: bool,
    /// The number of times the pointer has come onto the control or left it. A
    /// timer that an earlier hover started compares its own number with this
    /// one, and opens nothing when they differ.
    visit: u64,
    shown: motion::Fade,
    /// Whether the window has no room for the tooltip below the control. This
    /// is measured each time the control is drawn. When it is true, the
    /// tooltip opens above the control, so that it is not pushed up over the
    /// control it describes.
    no_room_below: std::rc::Rc<std::cell::Cell<bool>>,
}

impl Hover {
    /// Records that the pointer came onto the control or left it. The tooltip
    /// opens after the pointer has stayed on the control for
    /// `motion::TOOLTIP_DELAY`, and closes when the pointer leaves.
    pub fn set(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.set_after(hovered, motion::TOOLTIP_DELAY, cx);
    }

    /// Does the same as [`Hover::set`], except that the tooltip opens as soon
    /// as the pointer arrives.
    pub fn set_now(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.set_after(hovered, std::time::Duration::ZERO, cx);
    }

    fn set_after(&mut self, hovered: bool, delay: std::time::Duration, cx: &mut Context<Self>) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        self.visit += 1;
        if hovered && delay.is_zero() {
            self.shown.set(true, motion::TOOLTIP_FADE);
            cx.notify();
        } else if hovered {
            let visit = self.visit;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    if this.hovered && this.visit == visit {
                        this.shown.set(true, motion::TOOLTIP_FADE);
                        cx.notify();
                    }
                });
            })
            .detach();
        } else if self.shown.on() {
            self.shown.set(false, motion::TOOLTIP_FADE);
            cx.notify();
        }
    }

    pub fn animating(&self) -> bool {
        self.shown.animating()
    }

    /// Returns the tooltip's element, or `None` while the tooltip is hidden.
    /// The control adds it as its last child, and the tooltip opens below the
    /// control. The control must be positioned (`relative` or `absolute`).
    /// `extent` is the control's height.
    pub fn layer(&self, tip: &Tip, extent: f32, cx: &App) -> Option<AnyElement> {
        // The tooltip fades in along `motion::EASE`. While it does, it grows
        // from 90% of its size about its centre and rises the last 9px into
        // its place.
        let shown = self.shown.value_with(motion::EASE);
        if shown <= 0. {
            return None;
        }
        let (scale, below) = if tip.plain {
            // The plain bubble grows from 95% to 100% of its size along the
            // same curve, and does not move.
            (0.95 + 0.05 * shown, 0.)
        } else {
            motion::tooltip_pop(shown)
        };
        let theme = theme::theme(cx);
        // GPUI cannot scale the label, so only the bubble's surface, border
        // and shadow are drawn at the scale, about the bubble's centre; the
        // label, inside the 6px padding, stays at full size.
        let inset = relative((1. - scale) / 2.);
        let surface = super::GlassPanel::new(
            div()
                .id("tooltip-surface")
                .absolute()
                .left(inset)
                .top(inset)
                .w(relative(scale))
                .h(relative(scale))
                .bg(theme.glass())
                .border_1()
                .border_color(theme.border_secondary()),
            theme::RADIUS_SM,
        )
        .backdrop_blur(theme::tooltip_blur())
        .drop_shadows(vec![
            BoxShadow::new(px(0.), px(2.), rgba(theme::BLACK_30).into()).blur_radius(px(8.)),
        ])
        // The top edge is brighter than the sides: a 1px line in
        // `Theme::glass_border_top` that ends 3px before each corner. On the
        // plain bubble the line is in `Theme::contrast(15.)` and runs the
        // whole width.
        .child(if tip.plain {
            div()
                .absolute()
                .top(px(-1.))
                .left_0()
                .right_0()
                .h(px(1.))
                .bg(theme.contrast(15.))
        } else {
            div()
                .absolute()
                .top(px(-1.))
                .left(px(3.))
                .right(px(3.))
                .h(px(1.))
                .bg(theme.glass_border_top())
        });
        // The label has 6px of padding on each side inside the 1px border. The
        // surface draws the border over the bubble, so the padding here is
        // 7px.
        let mut bubble = div()
            .relative()
            .h(px(BUBBLE_HEIGHT))
            .px(px(7.))
            .flex()
            .items_center()
            .gap(px(4.))
            .font_family(theme::FONT_UI)
            .text(if tip.plain {
                theme::TEXT_SM
            } else {
                theme::TEXT_MD
            })
            .font_weight(FontWeight::NORMAL)
            .text_color(theme.text())
            .whitespace_nowrap()
            .opacity(shown)
            .child(surface)
            .child(tip.label.clone());
        for key in &tip.keys {
            bubble = bubble.child(
                div()
                    .h(px(16.))
                    .min_w(px(16.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .bg(theme.contrast(10.))
                    .text(theme::TEXT_XS)
                    .child(key.clone()),
            );
        }
        let (top, anchor) = if tip.above || self.no_room_below.get() {
            (-motion::TOOLTIP_OFFSET + below, Anchor::BottomCenter)
        } else {
            (extent + motion::TOOLTIP_OFFSET + below, Anchor::TopCenter)
        };
        let slot = div().absolute().top(px(top)).left(relative(0.5));
        // The canvas measures where the control ends in the window. It does so
        // each time the control is drawn, the first time on the frame on which
        // the tooltip is still invisible.
        let no_room = self.no_room_below.clone();
        let probe = canvas(
            move |bounds, window, _| {
                let needed = motion::TOOLTIP_OFFSET + BUBBLE_HEIGHT + WINDOW_MARGIN;
                let bottom = f32::from(bounds.top()) + needed;
                no_room.set(bottom > f32::from(window.viewport_size().height));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top(px(extent))
        .left_0()
        .size_0();
        Some(
            div()
                .absolute()
                .inset_0()
                .child(probe)
                .child(
                    slot.child(
                        deferred(
                            anchored()
                                .anchor(anchor)
                                .snap_to_window_with_margin(px(WINDOW_MARGIN))
                                .child(bubble),
                        )
                        .with_priority(super::layer::TOOLTIP),
                    ),
                )
                .into_any_element(),
        )
    }
}
