//! The button that a screen puts in the titlebar for one of its actions.
//! The Subscriptions screen and the Duplicates screen use it.
//!
//! The button has two forms. With `labelled` it shows its icon and its
//! label. Without `labelled` it is a square that shows only its icon, and
//! its label is its tooltip. A screen passes the same `labelled` to all of
//! its buttons, so they all change form at the same titlebar width.
use super::action_button::{ActionButton, Look, action_button};
use crate::theme;
use gpui::{ElementId, SharedString};

/// The height in pixels of a titlebar button. A button without a label is
/// also this wide.
pub const HEIGHT: f32 = theme::CONTROL_MD;
/// The space in pixels to the left and to the right of a labelled button's
/// contents.
const PADDING: f32 = 10.;
/// The space in pixels between a labelled button's icon and its label.
const GAP: f32 = 6.;
/// The space in pixels between two titlebar buttons that are next to each
/// other.
pub const BETWEEN: f32 = 4.;

/// Makes a titlebar button. `icon` is the name of a 14 px icon.
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
