//! Moving through the sidebar's rows with the keyboard, as the Electron
//! rows' `tabIndex` and `onKeyDown` allow: from the tree filter, Down or Tab
//! enters the rows; Up, Down, Tab and Shift-Tab move between them with the
//! focus look; Enter or Space opens the row as a click does; Escape gives
//! the keyboard back to the window.
use super::{Section, Sidebar};
use gpui::*;

actions!(
    sidebar,
    [FocusNext, FocusPrevious, OpenFocused, LeaveRows, EnterRows]
);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", FocusNext, Some("SidebarRows")),
        KeyBinding::new("tab", FocusNext, Some("SidebarRows")),
        KeyBinding::new("up", FocusPrevious, Some("SidebarRows")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("SidebarRows")),
        KeyBinding::new("enter", OpenFocused, Some("SidebarRows")),
        KeyBinding::new("space", OpenFocused, Some("SidebarRows")),
        KeyBinding::new("escape", LeaveRows, Some("SidebarRows")),
        KeyBinding::new("down", EnterRows, Some("SidebarFilter")),
        KeyBinding::new("tab", EnterRows, Some("SidebarFilter")),
    ]);
}

/// The row after (or before) `current` among `rows`, staying at the ends.
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
    /// The keyboard moves to the rows, at the first one.
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

    /// Enter or Space: the row does what a plain click does.
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

    /// Scrolls the list so the focused row shows, from where it was last
    /// drawn.
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
        // A row that is no longer shown starts again from the top.
        assert_eq!(
            step(&rows, Some("folder:9"), false).as_deref(),
            Some("system:active")
        );
    }
}
