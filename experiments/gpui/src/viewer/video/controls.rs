//! The pieces of `VideoPlayer.module.css` the media player and Flash's
//! controls (`FlashControls.tsx`) share: the shaded bar along the bottom,
//! its row of buttons, and `VolumePanel`.
use super::button::media_button;
use super::model;
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use gpui::*;
use std::cell::Cell;
use std::rc::Rc;

/// `.controls`: the bar along the bottom, shaded from black at 60% below to
/// clear above, 24px over its rows and 12px from either side, at opacity
/// `shown`. While `visible` it takes the pointer, so the media beneath it
/// does not; hidden (`.controlsHidden`) it passes the pointer through, even
/// while it fades out.
pub fn controls_bar(shown: f32, visible: bool) -> Div {
    let bar = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .pt(px(24.))
        .px(px(12.))
        .pb(px(10.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .bg(linear_gradient(
            0.,
            linear_color_stop(rgba(theme::MEDIA_CONTROLS_SHADE), 0.),
            linear_color_stop(transparent_black(), 1.),
        ))
        .opacity(shown);
    if visible { bar.occlude() } else { bar }
}

/// `.buttonRow`: the left and right groups of buttons at either end.
pub fn button_row(left: impl IntoElement, right: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .child(left)
        .child(right)
}

/// `.volumeTrack`'s height.
const TRACK: f32 = 80.;

type VolumeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;
type MuteHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Default)]
struct VolumeState {
    shown: bool,
    dragging: Rc<Cell<bool>>,
    /// Counts the pointer's visits, so only the latest leave's timer hides
    /// the slider.
    visit: u64,
}

/// `VolumePanel`'s `enter` and `leave`: the slider shows at once and goes
/// 300 ms after the pointer leaves the button and the slider, unless it is
/// being dragged.
fn hover(state: &Entity<VolumeState>, inside: bool, window: &mut Window, cx: &mut App) {
    let leave = state.update(cx, |state, cx| {
        state.visit += 1;
        if inside {
            state.shown = true;
            cx.notify();
            None
        } else {
            (!state.dragging.get()).then_some(state.visit)
        }
    });
    if let Some(visit) = leave {
        let state = state.clone();
        window
            .spawn(cx, async move |cx| {
                cx.background_executor()
                    .timer(motion::MEDIA_VOLUME_PANEL_GRACE)
                    .await;
                let _ = state.update(cx, |state, cx| {
                    if state.visit == visit && !state.dragging.get() {
                        state.shown = false;
                        cx.notify();
                    }
                });
            })
            .detach();
    }
}

/// `VolumePanel`: the mute button, and above it while the pointer is over
/// either the percentage and an 80px vertical track to press or drag.
#[derive(IntoElement)]
pub struct VolumePanel {
    id: ElementId,
    volume: f32,
    muted: bool,
    on_change: Option<VolumeHandler>,
    on_mute: Option<MuteHandler>,
}

pub fn volume_panel(id: impl Into<ElementId>, volume: f32, muted: bool) -> VolumePanel {
    VolumePanel {
        id: id.into(),
        volume,
        muted,
        on_change: None,
        on_mute: None,
    }
}

impl VolumePanel {
    /// Told the volume the track was pressed or dragged to.
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn on_mute(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_mute = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for VolumePanel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let child = |name: &'static str| {
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), name.into())
        };
        let state = window.use_keyed_state(child("volume"), cx, |_, _| VolumeState::default());
        let effective = if self.muted { 0. } else { self.volume };
        let mute = self.on_mute.clone();
        let entered = state.clone();
        let button = media_button(
            child("mute"),
            model::volume_icon(self.volume, self.muted),
            16.,
            ui::tip(if self.muted { "Unmute" } else { "Mute" }).shortcut("video.toggleMute", cx),
        )
        .on_hover(move |inside, window, cx| hover(&entered, inside, window, cx))
        .on_click(move |_, window, cx| {
            if let Some(mute) = &mute {
                mute(window, cx);
            }
        });
        let mut panel = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .items_center()
            .child(button);
        if !state.read(cx).shown {
            return panel;
        }
        let change = self.on_change.clone();
        let drag = state.clone();
        // `.volumeTrack` with its `.volumeFill`; a press sets the volume
        // where the pointer is, and a drag follows it anywhere in the window
        // until the button is up.
        let track = div()
            .relative()
            .w(px(4.))
            .h(px(TRACK))
            .rounded(px(2.))
            .bg(rgba(theme::MEDIA_TRACK))
            .cursor_pointer()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .w_full()
                    .h(relative(effective))
                    .rounded(px(2.))
                    .bg(rgba(theme::MEDIA_FILL)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let report = move |y: Pixels, window: &mut Window, cx: &mut App| {
                            if let Some(change) = &change {
                                let volume = model::volume_at(
                                    f32::from(y),
                                    f32::from(bounds.top()),
                                    f32::from(bounds.size.height),
                                );
                                change(volume, window, cx);
                            }
                        };
                        let state = drag.clone();
                        crate::ui::track_drag(
                            bounds,
                            drag.read(cx).dragging.clone(),
                            move |at, window, cx| report(at.y, window, cx),
                            move |cx| state.update(cx, |_, cx| cx.notify()),
                            window,
                        );
                    },
                )
                .absolute()
                .inset_0(),
            );
        let entered = state.clone();
        // `.volumePopup`: `bottom: 36px; left: 50%; transform: translateX(-50%)`.
        let popup = div()
            .id(child("popup"))
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .py(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .bg(rgba(theme::MEDIA_POPUP))
            .border_1()
            .border_color(rgba(theme::MEDIA_POPUP_BORDER))
            .on_hover(move |inside, window, cx| hover(&entered, *inside, window, cx))
            .child(
                div()
                    .font_family(theme::FONT_MONO)
                    .text(theme::TEXT_XS)
                    .text_color(rgba(theme::MEDIA_TEXT))
                    .child(format!("{}%", (effective * 100.).round() as i32)),
            )
            .child(track);
        panel = panel.child(
            div()
                .absolute()
                .bottom(px(36.))
                .left(relative(0.5))
                .w(px(0.))
                .flex()
                .justify_center()
                .child(popup),
        );
        panel
    }
}
