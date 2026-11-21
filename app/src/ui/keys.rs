//! The keys that every floating surface handles: Escape, the arrow keys, Enter
//! and Tab, as one set of actions. Each surface binds the keys it uses in its
//! own key context (`Modal`, `Popover`, `OverlayShell`, `SelectionModal`,
//! `Menu`). The innermost surface that has the keyboard focus handles a key
//! first, so a list that is open in a dialog handles Escape before the dialog
//! does.
use gpui::*;

actions!(
    keys,
    [
        /// Escape, which closes what is open.
        Dismiss,
        Up,
        Down,
        Left,
        Right,
        /// The Enter key.
        Confirm,
        /// Tab, which shows the next view of a panel's rail.
        NextView,
    ]
);

/// A key that a surface binds in its key context.
#[derive(Clone, Copy)]
pub enum Key {
    Escape,
    Up,
    Down,
    Left,
    Right,
    Enter,
    /// Tab. Shift-Tab does the same as Tab, as it did in Picto 0.6.19 and
    /// earlier.
    Tab,
}

/// Binds `keys` to their actions in `context`.
pub fn bind(cx: &mut App, context: &'static str, keys: &[Key]) {
    let context = Some(context);
    let mut bindings = Vec::new();
    for key in keys {
        match key {
            Key::Escape => bindings.push(KeyBinding::new("escape", Dismiss, context)),
            Key::Up => bindings.push(KeyBinding::new("up", Up, context)),
            Key::Down => bindings.push(KeyBinding::new("down", Down, context)),
            Key::Left => bindings.push(KeyBinding::new("left", Left, context)),
            Key::Right => bindings.push(KeyBinding::new("right", Right, context)),
            Key::Enter => bindings.push(KeyBinding::new("enter", Confirm, context)),
            Key::Tab => {
                bindings.push(KeyBinding::new("tab", NextView, context));
                bindings.push(KeyBinding::new("shift-tab", NextView, context));
            }
        }
    }
    cx.bind_keys(bindings);
}
