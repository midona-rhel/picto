//! Flash's controls (`FlashControls.tsx`): play or pause and stop on the
//! left, the volume on the right, in the media players' bar, which hides
//! 2.5 s after the pointer stops moving over the movie.
use super::flash::{Flash, Playback};
use super::surface::MediaSurface;
use crate::ui;
use crate::ui::media_controls::{button_row, controls_bar, media_button, volume_panel};
use gpui::*;

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
            )
            .disabled(!ready)
            .tip(ui::tip(if playback.playing { "Pause" } else { "Play" }))
            .on_click(move |_, _, cx| play(Box::new(Flash::toggle_play), cx)),
        )
        .child(
            media_button("flash-stop", "player-stop", 18.)
                .disabled(!ready)
                .tip(ui::tip("Stop"))
                .on_click(move |_, _, cx| stop(Box::new(Flash::stop), cx)),
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
