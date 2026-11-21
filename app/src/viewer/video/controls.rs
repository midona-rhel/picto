//! The parts of the controls that the media player and the Flash player's
//! controls (`viewer/flash_controls.rs`) both use: the shaded bar along the
//! bottom edge of the player, the row of buttons in it, and the volume
//! control.
use super::button::media_button;
use super::model;
use crate::i18n::t;
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use gpui::*;
use std::cell::Cell;
use std::rc::Rc;

/// Returns the bar that holds the controls, along the bottom edge of the
/// player. Its background goes from black at 60% at its bottom edge to
/// fully transparent at its top edge. It has 24 px of padding above its
/// rows, 12 px at either side and 10 px below, and 4 px between its rows.
/// It is drawn at the opacity `shown`. While `visible` is true the bar
/// occludes what is behind it, so the pointer does not reach the media
/// under the bar. While `visible` is false, which includes the time the
/// bar takes to fade out, pointer events pass through the bar.
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

/// Returns the row of buttons of the controls: the group `left` at the left
/// end of the row and the group `right` at the right end.
pub fn button_row(left: impl IntoElement, right: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .child(left)
        .child(right)
}

/// The height of the volume slider's track, in pixels.
const TRACK: f32 = 80.;

type VolumeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;
type MuteHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// What the volume control keeps from one frame to the next.
#[derive(Default)]
struct VolumeState {
    /// True while the volume slider is shown.
    shown: bool,
    /// True while the slider's track is dragged.
    dragging: Rc<Cell<bool>>,
    /// Counts the times the pointer has come onto, or left, the mute button
    /// or the slider. Each time the pointer leaves starts a timer, and only
    /// the timer that was started at the latest count hides the slider.
    visit: u64,
}

/// Called when the pointer comes onto the mute button or the volume slider
/// (`inside` is true) and when it leaves one of them (`inside` is false).
/// Coming onto either shows the slider at once. Leaving starts a timer of
/// 300 ms (`motion::MEDIA_VOLUME_PANEL_GRACE`) that hides the slider. The
/// timer does nothing if the pointer has come onto the button or the
/// slider again in that time, or if the slider is being dragged when it
/// ends. No timer is started when the pointer leaves during a drag.
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

/// The volume control: the mute button and, while the pointer is over the
/// button or the slider, the volume slider above the button. The slider is
/// a box that shows the volume as a percentage and, under it, a vertical
/// track 80 px tall that is pressed or dragged to set the volume.
#[derive(IntoElement)]
pub struct VolumePanel {
    id: ElementId,
    volume: f32,
    muted: bool,
    on_change: Option<VolumeHandler>,
    on_mute: Option<MuteHandler>,
}

/// Returns a volume control with the element id `id` that shows the volume
/// `volume`, from 0 to 1, and whether the player is muted.
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
    /// Sets the handler that is called with the new volume, from 0 to 1,
    /// when the track is pressed or dragged.
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Sets the handler that is called when the mute button is clicked.
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
            ui::tip(if self.muted { t("Unmute") } else { t("Mute") })
                .shortcut("video.toggleMute", cx),
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
        // The track is 4 px wide, and the part of it from its bottom up to
        // the volume is filled. A press on the track sets the volume at the
        // pointer's position, and a drag that starts on it follows the
        // pointer anywhere in the window until the mouse button is
        // released. Assistive technology sees the track as a slider named
        // "Volume" whose value is the volume in percent. Its increment and
        // decrement actions change the volume by 10%.
        let stepped = |delta: f32| {
            let change = self.on_change.clone();
            move |_: Option<&gpui::accesskit::ActionData>, window: &mut Window, cx: &mut App| {
                if let Some(change) = &change {
                    change((effective + delta).clamp(0., 1.), window, cx);
                }
            }
        };
        let track = div()
            .id(child("volume"))
            .role(Role::Slider)
            .aria_label(crate::i18n::t("Volume"))
            .aria_numeric_value((effective * 100.).round() as f64)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(100.)
            .on_a11y_action(AccessibleAction::Increment, stepped(0.1))
            .on_a11y_action(AccessibleAction::Decrement, stepped(-0.1))
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
        // The slider's box is drawn above the mute button, centred on it,
        // with its bottom edge 36 px above the bottom edge of the volume
        // control. It occludes what is behind it, and it reports when the
        // pointer comes onto it or leaves it, as the mute button does.
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
