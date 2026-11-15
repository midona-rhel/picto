use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;

/// What a tooltip says: a label and the keys of its shortcut, one badge each
/// (`KbdTooltip.tsx`).
#[derive(Clone)]
pub struct Tip {
    label: SharedString,
    keys: Vec<SharedString>,
    /// Opens above the control, where Mantine flips a tooltip that would
    /// leave the window below it.
    above: bool,
    /// The plain CSS bubble Settings draws where it has no Mantine tooltip
    /// (`.themeSwatch[data-tooltip]::after`).
    plain: bool,
}

pub fn tip(label: impl Into<SharedString>) -> Tip {
    Tip::new(label)
}

/// Gives a control a tooltip. The control must not have its own `on_hover`;
/// one that does keeps a [`Hover`] itself and adds its layer.
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

    /// What the tooltip says, which also names an icon-only control for
    /// assistive technology (production's `aria-label` repeats it).
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// `.themeSwatch[data-tooltip]::after`, Settings' own bubble: shown at
    /// once (give its [`Hover`] [`Hover::set_now`]), above the control, in
    /// the small text, with a `--color-contrast-15` top edge, growing from
    /// 95% without rising.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self.above = true;
        self
    }

    /// Opens above the control: for controls along the window's bottom edge.
    pub fn above(mut self) -> Self {
        self.above = true;
        self
    }

    /// The keys a registry shortcut is bound to now, so the tooltip
    /// follows a rebinding (`shortcutId`).
    pub fn shortcut(mut self, id: &str, cx: &App) -> Self {
        self.keys = crate::keymap::badges(id, cx);
        self
    }

    /// The keys a registry shortcut is bound to now, written as one badge,
    /// as a tooltip given the keys as text shows them
    /// (`shortcut={formatKeysDisplay(keys)}`).
    pub fn shortcut_written(mut self, id: &str, cx: &App) -> Self {
        self.keys = crate::keymap::hint(id, cx).into_iter().collect();
        self
    }
}

/// The bubble's height, and how far a tooltip keeps from the window's edge.
const BUBBLE_HEIGHT: f32 = 24.;
const WINDOW_MARGIN: f32 = 5.;

/// A control's tooltip timing, kept between frames: it opens once the pointer
/// has rested on the control for 400 ms and closes when the pointer leaves,
/// popping in and out over 150 ms each way.
#[derive(Default)]
pub struct Hover {
    hovered: bool,
    /// Counts hovers, so a timer from an earlier one opens nothing.
    visit: u64,
    shown: motion::Fade,
    /// Whether the window has no room for the tooltip below the control,
    /// as last measured while it was drawn: it opens above then, rather
    /// than being pushed up over the control it names.
    no_room_below: std::rc::Rc<std::cell::Cell<bool>>,
}

impl Hover {
    /// The pointer came onto the control or left it: the tooltip opens
    /// after it has rested there.
    pub fn set(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.set_after(hovered, motion::TOOLTIP_DELAY, cx);
    }

    /// As [`Hover::set`], opening as the pointer arrives.
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

    /// The tooltip, to add as the last child of its control, below which it
    /// opens. The control must be positioned (`relative` or `absolute`);
    /// `extent` is its height.
    pub fn layer(&self, tip: &Tip, extent: f32, cx: &App) -> Option<AnyElement> {
        // Mantine's `pop` on CSS `ease`: the tooltip fades in while growing
        // from 90% about its centre and rising the last 9px into place.
        let shown = self.shown.value_with(motion::EASE);
        if shown <= 0. {
            return None;
        }
        let (scale, below) = if tip.plain {
            // `transform: scale(0.95)` to `scale(1)` on the same `ease`.
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
        // `--glass-border-top`: the top edge is brighter than the sides
        // (the plain bubble's whole top border is `--color-contrast-15`).
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
        // `padding: 0 6px` inside the 1px border, which the surface draws.
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
        // Where the control ends in the window, measured as it is drawn
        // (the first frame, on which the tooltip is still invisible).
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
