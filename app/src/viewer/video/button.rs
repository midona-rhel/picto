//! The buttons of the media player's controls. Each is the shared icon
//! button (`ui/icon_button.rs`) with the look `Look::Media`: 32 px tall,
//! white on the dark controls.
use crate::ui::icon_button::{IconButton, Look, icon_button};
use crate::ui::tooltip::Tip;
use gpui::*;

/// Returns a button of the player's controls that shows the glyph `glyph`
/// at `size` pixels and has the tooltip `tip`.
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

/// Returns a button of the player's controls that shows the text `label` in
/// place of a glyph and has the tooltip `tip`. The button is as wide as its
/// label with 4 px of padding at either side, and at least 28 px wide. The
/// rate button is the only button of this kind.
pub fn label_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tip: Tip,
) -> IconButton {
    icon_button(id, "").look(Look::Media).label(label).tip(tip)
}
