//! What assistive technology (VoiceOver, Narrator, Orca) is told about the
//! app. GPUI builds an AccessKit tree, and each element that has an id and a
//! role is a node of it (`.role(…)`, `.aria_label(…)`, `.aria_toggled(…)`, …).
//!
//! GPUI builds the tree only while assistive technology has asked for it
//! (`Window::is_a11y_active`). At other times the roles and names that
//! elements are given are stored and never read. Work that is done only to
//! describe something (formatting a name from numbers, making a node per item)
//! is skipped unless the window is building the tree ([`active`]).
//!
//! The rules, which the README also gives under "Accessibility":
//! - Every shared control sets its own role, name, value and states. A view
//!   gives a control a name of its own, through `t`, when the control's
//!   visible text or tooltip is not a good name for it.
//! - A control is named by its visible label, or else by its tooltip's text.
//! - Plain text is not in the tree. A container whose text assistive
//!   technology should read (a heading, a status line, a dialog's message)
//!   takes a role and its text as a label.
//! - Ids under one node with a role must not repeat in a frame. A debug build
//!   stops on a repeated id while the tree is built.
use gpui::{Role, SharedString, StatefulInteractiveElement, Toggled, Window};

/// Returns the `Toggled` state for a boolean: `True` when `on` is true, and
/// `False` otherwise. A control that can be ticked or pressed reports it.
pub fn toggled(on: bool) -> Toggled {
    if on { Toggled::True } else { Toggled::False }
}

/// Returns whether the window is building its accessibility tree in this
/// frame. Work that only describes the window to assistive technology is done
/// only when this is true.
pub fn active(window: &Window) -> bool {
    window.is_a11y_active()
}

/// Makes `element` a text that assistive technology reads: a dialog's message,
/// a status line, the text of an empty list. This is needed because the text
/// that GPUI draws is not in the tree.
pub fn text<E: StatefulInteractiveElement>(element: E, text: impl Into<SharedString>) -> E {
    element.role(Role::Label).aria_value(text)
}

/// Makes `element` a heading of the level `level` with the name `text`, for
/// example a section's title.
pub fn heading<E: StatefulInteractiveElement>(
    element: E,
    text: impl Into<SharedString>,
    level: usize,
) -> E {
    element
        .role(Role::Heading)
        .aria_label(text)
        .aria_level(level)
}

#[cfg(test)]
pub mod tree;

#[cfg(test)]
mod tests;
