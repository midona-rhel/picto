//! The keys every floating surface takes: Escape, the arrows, Enter and
//! Tab, as one set of actions. Each surface binds the ones it uses in its
//! own key context (`Modal`, `Popover`, `OverlayShell`, `SelectionModal`,
//! `Menu`), and the innermost surface holding the keyboard handles them,
//! so a list open in a dialog takes Escape before the dialog does.
use gpui::*;

actions!(
    keys,
    [
        /// Escape: close what is open.
        Dismiss,
        Up,
        Down,
        Left,
        Right,
        /// Enter.
        Confirm,
        /// Tab: the next view of a panel's rail.
        NextView,
    ]
);

/// A key a surface takes in its context.
#[derive(Clone, Copy)]
pub enum Key {
    Escape,
    Up,
    Down,
    Left,
    Right,
    Enter,
    /// Tab, and Shift-Tab the same way, since production reads only the key.
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
