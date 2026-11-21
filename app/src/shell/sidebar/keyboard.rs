//! Moves through the sidebar's rows with the keyboard. From the tree filter,
//! Down or Tab moves keyboard focus to the first row. Up, Down, Tab and
//! Shift-Tab move from row to row, and the row that the keyboard is on is
//! drawn as focused. Enter or Space does what a click on the row does.
//! Escape moves keyboard focus out of the rows.
use super::{Section, Sidebar};
use gpui::*;

actions!(
    sidebar,
    [FocusNext, FocusPrevious, OpenFocused, LeaveRows, EnterRows]
);

/// The key context of the rows while no rename field in a row has keyboard
/// focus. A rename field handles its own keys.
const ROWS: &str = "SidebarRows && !TextInput";

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", FocusNext, Some(ROWS)),
        KeyBinding::new("tab", FocusNext, Some(ROWS)),
        KeyBinding::new("up", FocusPrevious, Some(ROWS)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(ROWS)),
        KeyBinding::new("enter", OpenFocused, Some(ROWS)),
        KeyBinding::new("space", OpenFocused, Some(ROWS)),
        KeyBinding::new("escape", LeaveRows, Some(ROWS)),
        KeyBinding::new("down", EnterRows, Some("SidebarFilter")),
        KeyBinding::new("tab", EnterRows, Some("SidebarFilter")),
    ]);
}

/// Returns the row after `current` in `rows` when `forward` is true, and the
/// row before it otherwise. At the first or the last row it returns that
/// row. When `current` is `None` or is not in `rows`, it returns the first
/// row.
pub fn step(rows: &[String], current: Option<&str>, forward: bool) -> Option<String> {
    let at = current.and_then(|current| rows.iter().position(|row| row == current));
    let next = match (at, forward) {
        (None, _) => 0,
        (Some(at), true) => (at + 1).min(rows.len().saturating_sub(1)),
        (Some(at), false) => at.saturating_sub(1),
    };
    rows.get(next).cloned()
}

impl Sidebar {
    /// Moves keyboard focus to the rows and makes the first row the focused
    /// row.
    pub(super) fn enter_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_row = self.shown_rows.first().cloned();
        window.focus(&self.rows_focus, cx);
        self.scroll_to_focused();
        cx.notify();
    }

    pub(super) fn focus_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.keyboard_row = step(&self.shown_rows, self.keyboard_row.as_deref(), forward);
        self.scroll_to_focused();
        cx.notify();
    }

    pub(super) fn leave_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_row = None;
        window.focus(&self.home, cx);
        cx.notify();
    }

    /// Does on the focused row what a click without modifier keys does.
    /// Enter and Space run this.
    pub(super) fn open_focused(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.keyboard_row.clone() else {
            return;
        };
        let node = row.strip_prefix("quick-").unwrap_or(&row).to_owned();
        let (index, group, has_children) = {
            let nav = &self.session.read(cx).nav;
            let Some(index) = nav.iter().position(|entry| entry.id() == node) else {
                return;
            };
            (
                index,
                nav[index].group,
                nav.iter().any(|entry| entry.parent == Some(index)),
            )
        };
        if Section::of_node(&node).is_none() {
            self.selection.clear();
            self.session
                .update(cx, |session, cx| session.navigate(index, cx));
        } else if group {
            if has_children {
                self.toggle_collapsed(&node, cx);
            }
        } else {
            self.click_node(index, node, Modifiers::default(), cx);
        }
        cx.notify();
    }

    /// Scrolls the list so that the whole focused row is in the visible part
    /// of the list. It uses the bounds at which the row and the list were
    /// last drawn.
    fn scroll_to_focused(&self) {
        let Some((row, list)) = self
            .keyboard_row
            .as_deref()
            .and_then(|row| self.bounds.row(row))
        else {
            return;
        };
        let offset = self.scroll.offset();
        let shift = if row.top() < list.top() {
            list.top() - row.top()
        } else if row.bottom() > list.bottom() {
            list.bottom() - row.bottom()
        } else {
            return;
        };
        self.scroll.set_offset(point(offset.x, offset.y + shift));
    }
}

#[cfg(test)]
mod tests {
    use super::step;

    #[test]
    fn the_keyboard_steps_through_the_rows_and_stops_at_the_ends() {
        let rows = ["system:active", "system:inbox", "folder:1"].map(String::from);
        assert_eq!(step(&rows, None, true).as_deref(), Some("system:active"));
        assert_eq!(
            step(&rows, Some("system:active"), true).as_deref(),
            Some("system:inbox")
        );
        assert_eq!(
            step(&rows, Some("folder:1"), true).as_deref(),
            Some("folder:1")
        );
        assert_eq!(
            step(&rows, Some("system:active"), false).as_deref(),
            Some("system:active")
        );
        // When the current row is no longer shown, the step goes to the
        // first row.
        assert_eq!(
            step(&rows, Some("folder:9"), false).as_deref(),
            Some("system:active")
        );
    }
}
