use super::icon_button::{IconButton, Look, icon_button};

/// Returns the small glyph button of table rows and job strips: the shared
/// icon button with the look `Look::Small`.
pub fn small_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> IconButton {
    icon_button(id, icon).look(Look::Small)
}
