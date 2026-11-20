//! A screen's action in the titlebar: labelled while the titlebar has the
//! room, else its glyph alone with the label as its tooltip. A screen
//! switches all its actions together, so the row never mixes the two.
use super::action_button::{ActionButton, Look, action_button};
use crate::theme;
use gpui::{ElementId, SharedString};

/// The titlebar's control height, and the padding and gap a labelled
/// button keeps there.
pub const HEIGHT: f32 = theme::CONTROL_MD;
const PADDING: f32 = 10.;
const GAP: f32 = 6.;
/// The room between two buttons of one group.
pub const BETWEEN: f32 = 4.;

pub fn titlebar_button(
    id: impl Into<ElementId>,
    look: Look,
    icon: &'static str,
    label: impl Into<SharedString>,
    labelled: bool,
) -> ActionButton {
    let button = action_button(id, look).icon(icon, 14.).height(HEIGHT);
    if labelled {
        button.label(label).padding(PADDING).gap(GAP)
    } else {
        button.width(HEIGHT).padding(0.).tip(super::tip(label))
    }
}
