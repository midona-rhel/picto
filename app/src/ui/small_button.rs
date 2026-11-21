use super::icon_button::{IconButton, Look, icon_button};

/// The quiet glyph button of table rows and job strips (`.querySmallBtn`):
/// the shared icon button in its small look.
pub fn small_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> IconButton {
    icon_button(id, icon).look(Look::Small)
}
