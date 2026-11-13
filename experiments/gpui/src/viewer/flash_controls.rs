//! Flash's controls (`FlashControls.tsx`): play or pause and stop on the
//! left, the volume (`VolumePanel.tsx`) on the right, in the shaded bar of
//! `VideoPlayer.module.css`, which hides 2.5 s after the pointer stops
//! moving over the movie. The buttons are the media player's
//! ([`super::video::button`]).
// TODO: the media player draws its own bar and volume control inline; one
// of the two should serve both, as `FlashControls` reuses `VolumePanel`.
use super::flash::{Flash, Playback};
use super::surface::MediaSurface;
use super::video::button::media_button;
use crate::{theme, ui};
use gpui::*;
use std::rc::Rc;
use std::time::Duration;

/// The bar for the movie `surface` plays, at opacity `shown`.
pub fn controls(
    surface: WeakEntity<MediaSurface>,
    playback: &Playback,
    ready: bool,
    shown: f32,
) -> Div {
    let run = {
        let surface = surface.clone();
        move |control: Box<dyn Fn(&mut Flash)>, cx: &mut App| {
            let _ = surface.update(cx, |surface, cx| surface.control_flash(control, cx));
        }
    };
    let (play, stop, change, mute) = (run.clone(), run.clone(), run.clone(), run);
    let left = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(
            media_button(
                "flash-play",
                if playback.playing {
                    "player-pause"
                } else {
                    "player-play"
                },
                20.,
                ui::tip(if playback.playing { "Pause" } else { "Play" }),
            )
            .on_click(move |_, _, cx| {
                if ready {
                    play(Box::new(Flash::toggle_play), cx)
                }
            }),
        )
        .child(
            media_button("flash-stop", "player-stop", 18., ui::tip("Stop")).on_click(
                move |_, _, cx| {
                    if ready {
                        stop(Box::new(Flash::stop), cx)
                    }
                },
            ),
        );
    let right = volume_panel("flash-volume", playback.volume, playback.muted)
        .on_change(move |volume, cx| {
            change(
                Box::new(move |flash: &mut Flash| flash.set_volume(volume)),
                cx,
            )
        })
        .on_mute(move |cx| mute(Box::new(Flash::toggle_mute), cx));
    controls_bar(shown).child(button_row(left, right))
}

/// `useMediaControlsVisibility`: the controls hide this long after the
/// pointer last moved over the player.
pub const HIDE_DELAY: Duration = Duration::from_millis(2500);
/// `.controls { transition: opacity 0.3s ease }`
pub const FADE: Duration = Duration::from_millis(300);
/// `VolumePanel`: the slider stays this long after the pointer leaves.
const VOLUME_LINGER: Duration = Duration::from_millis(300);

/// `.controls`: the bar along the bottom, shaded from black at 60% below to
/// clear above, 24px over its row and 12px from either side. `shown` is its
/// opacity; hidden, it takes no pointer (`.controlsHidden`).
pub fn controls_bar(shown: f32) -> Div {
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
            linear_color_stop(rgba(0x00000099), 0.),
            linear_color_stop(rgba(0x00000000), 1.),
        ))
        .opacity(shown);
    if shown > 0. {
        // A press on the bar stays out of the media beneath it.
        bar.occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    } else {
        bar
    }
}

/// `.buttonRow` with its left and right groups.
pub fn button_row(left: impl IntoElement, right: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .child(div().flex().items_center().gap(px(4.)).child(left))
        .child(div().flex().items_center().gap(px(4.)).child(right))
}

/// The glyph for a volume (`VolumePanel`'s icon choice).
pub fn volume_icon(volume: f32, muted: bool) -> &'static str {
    if muted || volume == 0. {
        "media-volume-off"
    } else if volume < 0.5 {
        "media-volume-2"
    } else {
        "media-volume"
    }
}

/// The volume at a point on the 80px track, from its top edge: full at the
/// top, silent at the bottom.
pub fn volume_at(from_top: f32) -> f32 {
    (1. - from_top / TRACK).clamp(0., 1.)
}

const TRACK: f32 = 80.;

#[derive(Default)]
struct VolumeState {
    shown: bool,
    dragging: bool,
    /// Counts leaves, so only the latest one's timer hides the slider.
    leave: u64,
}

/// `VolumePanel`: the mute button, and above it while the pointer is over
/// either the percentage and an 80px vertical track to drag.
#[derive(IntoElement)]
pub struct VolumePanel {
    id: ElementId,
    volume: f32,
    muted: bool,
    on_change: Option<Rc<dyn Fn(f32, &mut App)>>,
    on_mute: Option<Rc<dyn Fn(&mut App)>>,
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
    pub fn on_change(mut self, handler: impl Fn(f32, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn on_mute(mut self, handler: impl Fn(&mut App) + 'static) -> Self {
        self.on_mute = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for VolumePanel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "volume".into()),
            cx,
            |_, _| VolumeState::default(),
        );
        let effective = if self.muted { 0. } else { self.volume };
        let shown = state.read(cx).shown;
        let mute = self.on_mute.clone();
        let button = media_button(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "mute".into()),
            volume_icon(self.volume, self.muted),
            16.,
            ui::tip(if self.muted { "Unmute" } else { "Mute" }).shortcut("video.toggleMute", cx),
        )
        .on_click(move |_, _, cx| {
            if let Some(mute) = &mute {
                mute(cx);
            }
        });
        let popup = shown.then(|| {
            let change = self.on_change.clone();
            let drag_state = state.clone();
            // `.volumePopup`: centred 36px above the button.
            div()
                .absolute()
                .bottom(px(36.))
                .left(px(16. - 24.))
                .w(px(48.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(6.))
                        .py(px(8.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .rounded(px(theme::RADIUS_MD))
                        .bg(rgba(0x000000cc))
                        .border_1()
                        .border_color(rgba(0xffffff1a))
                        .child(
                            div()
                                .text_size(px(theme::TEXT_XS))
                                .line_height(px(theme::TEXT_XS + 2.))
                                .font_family(theme::FONT_MONO)
                                .text_color(rgba(0xffffffb3))
                                .child(format!("{}%", (effective * 100.).round())),
                        )
                        .child(
                            // `.volumeTrack` with its `.volumeFill`; a press
                            // or drag sets the volume where the pointer is.
                            div()
                                .relative()
                                .w(px(4.))
                                .h(px(TRACK))
                                .rounded(px(2.))
                                .bg(rgba(0xffffff26))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .bottom_0()
                                        .w_full()
                                        .h(px(TRACK * effective))
                                        .rounded(px(2.))
                                        .bg(rgba(0xfffffff2)),
                                )
                                .child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            let report = {
                                                let change = change.clone();
                                                move |y: Pixels, cx: &mut App| {
                                                    if let Some(change) = &change {
                                                        change(
                                                            volume_at(f32::from(y - bounds.top())),
                                                            cx,
                                                        );
                                                    }
                                                }
                                            };
                                            let (down, report_down) =
                                                (drag_state.clone(), report.clone());
                                            window.on_mouse_event(
                                                move |event: &MouseDownEvent, phase, _, cx| {
                                                    if phase == DispatchPhase::Bubble
                                                        && event.button == MouseButton::Left
                                                        && bounds.contains(&event.position)
                                                    {
                                                        down.update(cx, |state, _| {
                                                            state.dragging = true
                                                        });
                                                        report_down(event.position.y, cx);
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            );
                                            let moved = drag_state.clone();
                                            window.on_mouse_event(
                                                move |event: &MouseMoveEvent, phase, _, cx| {
                                                    if phase == DispatchPhase::Capture
                                                        && moved.read(cx).dragging
                                                    {
                                                        report(event.position.y, cx);
                                                    }
                                                },
                                            );
                                            let up = drag_state.clone();
                                            window.on_mouse_event(
                                                move |_: &MouseUpEvent, phase, _, cx| {
                                                    if phase == DispatchPhase::Capture
                                                        && up.read(cx).dragging
                                                    {
                                                        up.update(cx, |state, cx| {
                                                            state.dragging = false;
                                                            cx.notify();
                                                        });
                                                    }
                                                },
                                            );
                                        },
                                    )
                                    .absolute()
                                    .inset_0(),
                                ),
                        ),
                )
        });
        let hovered = state.clone();
        div()
            .id(self.id)
            .relative()
            .flex()
            .items_center()
            .on_hover(move |inside, window, cx| {
                let inside = *inside;
                let leave = hovered.update(cx, |state, cx| {
                    if inside {
                        state.shown = true;
                        state.leave += 1;
                        cx.notify();
                        None
                    } else if !state.dragging {
                        state.leave += 1;
                        Some(state.leave)
                    } else {
                        None
                    }
                });
                if let Some(leave) = leave {
                    let state = hovered.clone();
                    window
                        .spawn(cx, async move |cx| {
                            cx.background_executor().timer(VOLUME_LINGER).await;
                            let _ = state.update(cx, |state, cx| {
                                if state.leave == leave && !state.dragging {
                                    state.shown = false;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                }
            })
            .child(button)
            .children(popup)
    }
}

#[cfg(test)]
mod tests {
    use super::{volume_at, volume_icon};

    #[test]
    fn the_volume_icon_follows_the_level() {
        assert_eq!(volume_icon(0.8, false), "media-volume");
        assert_eq!(volume_icon(0.3, false), "media-volume-2");
        assert_eq!(volume_icon(0.3, true), "media-volume-off");
        assert_eq!(volume_icon(0., false), "media-volume-off");
    }

    #[test]
    fn the_track_reads_from_the_top() {
        assert_eq!(volume_at(0.), 1.);
        assert_eq!(volume_at(40.), 0.5);
        assert_eq!(volume_at(120.), 0.);
    }
}
