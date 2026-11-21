//! Flash's controls (`FlashControls.tsx`): play or pause and stop on the
//! left, the volume on the right, in the media player's shaded bar
//! ([`super::video::controls`]), which hides as the player's does after the
//! pointer stops moving over the movie. The buttons are the media player's.
use super::flash::{Flash, Playback};
use super::surface::MediaSurface;
use super::video::button::media_button;
use super::video::controls::{button_row, controls_bar, volume_panel};
use crate::i18n::t;
use crate::ui;
use gpui::*;

/// The bar for the movie `surface` plays, at opacity `shown`, taking the
/// pointer while `visible`.
pub fn controls(
    surface: WeakEntity<MediaSurface>,
    playback: &Playback,
    ready: bool,
    shown: f32,
    visible: bool,
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
                ui::tip(if playback.playing {
                    t("Pause")
                } else {
                    t("Play")
                }),
            )
            .on_click(move |_, _, cx| {
                if ready {
                    play(Box::new(Flash::toggle_play), cx)
                }
            }),
        )
        .child(
            media_button("flash-stop", "player-stop", 18., ui::tip(t("Stop"))).on_click(
                move |_, _, cx| {
                    if ready {
                        stop(Box::new(Flash::stop), cx)
                    }
                },
            ),
        );
    let right = volume_panel("flash-volume", playback.volume, playback.muted)
        .on_change(move |volume, _, cx| {
            change(
                Box::new(move |flash: &mut Flash| flash.set_volume(volume)),
                cx,
            )
        })
        .on_mute(move |_, cx| mute(Box::new(Flash::toggle_mute), cx));
    let bar = controls_bar(shown, visible).child(button_row(left, right));
    // A press on the bar stays out of the movie beneath it.
    if visible {
        bar.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    } else {
        bar
    }
}
