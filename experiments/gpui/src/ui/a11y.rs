//! What assistive technology (VoiceOver, Narrator, Orca) is told about the
//! app: GPUI's AccessKit tree, which each element with an id and a role
//! joins (`.role(…)`, `.aria_label(…)`, `.aria_toggled(…)`, …).
//!
//! GPUI builds the tree only while assistive technology has asked for it
//! (`Window::is_a11y_active`); otherwise the roles and names elements are
//! given are stored and never read. Work done only to describe something
//! (formatting a name from numbers, a node per item) is skipped unless the
//! window is building the tree ([`active`]).
//!
//! Rules (the README's "Accessibility"):
//! - Every shared control sets its own role, name, value and states; a view
//!   gives a control the name production's `aria-label` gives it, through
//!   `t`, when its visible text or tooltip is not that name.
//! - A control is named by its visible label, else by its tooltip's text
//!   (production's icon buttons repeat their tooltip in `aria-label`).
//! - Plain text is not in the tree: a container that reads as text to
//!   production (a heading, a status line, a dialog's message) takes a role
//!   and its text as a label.
//! - Ids under one node with a role must not repeat in a frame: a debug
//!   build stops on a repeated one while the tree is built.
use gpui::{Toggled, Window};

/// `aria-checked` / `aria-pressed` from a boolean.
pub fn toggled(on: bool) -> Toggled {
    if on { Toggled::True } else { Toggled::False }
}

/// Whether the window is building its accessibility tree this frame: gate
/// work that only describes the window to assistive technology on it.
pub fn active(window: &Window) -> bool {
    window.is_a11y_active()
}

#[cfg(test)]
pub mod tree;

#[cfg(test)]
mod tests;
