//! `.icBtn` of `VideoPlayer.module.css`: the player's 32px control, white on
//! the dark controls, as the shared icon button's media look.
use crate::ui::icon_button::{IconButton, Look, icon_button};
use crate::ui::tooltip::Tip;
use gpui::*;

pub fn media_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    size: f32,
    tip: Tip,
) -> IconButton {
    icon_button(id, glyph)
        .look(Look::Media)
        .glyph(size)
        .tip(tip)
}

/// `.icBtn.rateButton`: as wide as its label, at least 28px, 4px either side.
pub fn label_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tip: Tip,
) -> IconButton {
    icon_button(id, "").look(Look::Media).label(label).tip(tip)
}
