//! This file shows the application menu's model in two ways. On macOS it
//! gives the model to GPUI, which shows it as the system's menu bar. On
//! Windows and Linux, which have no menu bar, it builds the rows of a menu
//! that a button in the titlebar opens inside the window.
//!
//! It also answers, for the Quick Search panel, which of the menu's rows
//! that have a registry shortcut are enabled now ([`shortcut_rows`]).
use super::*;
use crate::settings::registry;

/// Returns the menus for `context` in the form that GPUI's `set_menus`
/// takes.
pub(super) fn gpui_menus(context: &MenuContext) -> Vec<Menu> {
    fn convert(entry: &Entry) -> MenuItem {
        match entry {
            Entry::Separator => MenuItem::Separator,
            Entry::Services => MenuItem::os_submenu("Services", SystemMenuType::Services),
            Entry::Submenu {
                title,
                enabled,
                entries,
            } => MenuItem::Submenu(Menu {
                name: title.clone().into(),
                items: entries.iter().map(convert).collect(),
                disabled: !enabled,
            }),
            Entry::Item(item) => MenuItem::Action {
                name: item.title.clone().into(),
                action: item.action.boxed_clone(),
                os_action: item.os_action,
                checked: item.checked,
                disabled: !item.enabled,
            },
        }
    }
    model(context, Platform::CURRENT)
        .iter()
        .map(|menu| Menu {
            name: menu.title.into(),
            items: menu.entries.iter().map(convert).collect(),
            disabled: false,
        })
        .collect()
}

/// Returns whether the main window has the application menu button
/// (`ApplicationMenuButton` in production). It has the button on Windows and
/// Linux, where there is no menu bar. On macOS it has the button only when
/// the environment variable `PICTO_GPUI_APP_MENU` is `window`. Production's
/// development build shows the button on macOS in the same way, so that the
/// Windows and Linux menu can be checked there.
pub fn in_window() -> bool {
    !cfg!(target_os = "macos") || crate::seams::app_menu_in_window()
}

/// The width of the menu inside the window, in pixels (`width={220}` in
/// production).
pub const IN_WINDOW_WIDTH: f32 = 220.;

/// Returns the menus that the menu inside the window shows. They are this
/// platform's menus. On macOS the first menu, "Picto", is left out, because
/// the File and Help menus already hold its rows. This is what production's
/// `picto:application-menu:get` returns.
pub(super) fn in_window_menus(cx: &App) -> Vec<TopMenu> {
    let mut menus = model(&context(cx), Platform::CURRENT);
    if Platform::CURRENT.is_mac() {
        menus.remove(0);
    }
    menus
}

/// Returns whether a handler would run `action` now. That is the case when
/// an element on the path from the focused element to the window's root
/// handles the action, or when the app handles it for every window. AppKit
/// asks GPUI the same question to enable a row of the menu bar.
pub fn handled(action: &dyn Action, window: &Window, cx: &mut App) -> bool {
    // The app is asked while this window is being updated, so only the
    // app's own handlers count in the second call.
    window.is_action_available(action, cx) || cx.is_action_available(action)
}

/// Returns whether the menu enables the row `item` now. The row is enabled
/// when production's rule for it holds (`item.enabled`) and a handler would
/// run its action ([`handled`]).
fn row_enabled(item: &Item, window: &Window, cx: &mut App) -> bool {
    item.enabled && handled(item.action.as_ref(), window, cx)
}

/// One row of the application menu that shows the keys of a shortcut of the
/// registry.
pub struct ShortcutRow {
    /// The registry id of the shortcut, for example `file.import`.
    pub id: &'static str,
    /// The action that the row performs.
    pub action: Box<dyn Action>,
    /// Whether the menu enables the row now.
    pub enabled: bool,
}

/// Returns every row of this platform's menus that shows the keys of a
/// registry shortcut, for the menu's current context. The Quick Search panel
/// lists a command that has such a row exactly when the row is enabled, and
/// runs the row's action.
pub fn shortcut_rows(window: &Window, cx: &mut App) -> Vec<ShortcutRow> {
    let menus = model(&context(cx), Platform::CURRENT);
    let mut rows = Vec::new();
    for menu in &menus {
        each_item(&menu.entries, &mut |item| {
            if let Keys::Registry(id) | Keys::Shortcut(id) = item.keys {
                rows.push((id, item.action.boxed_clone(), item.enabled));
            }
        });
    }
    rows.into_iter()
        .map(|(id, action, rule)| ShortcutRow {
            id,
            enabled: rule && handled(action.as_ref(), window, cx),
            action,
        })
        .collect()
}

/// Returns the rows of the menu inside the window (`toMenuEntries` in
/// production). Each menu of the model is a submenu of the button's menu. A
/// submenu inside a menu, such as Recent Libraries or Set Rating, is laid
/// out in its parent after a separator, because the shared menu control
/// nests only one level. A row is enabled when the menu bar would enable it
/// ([`row_enabled`]). A row's value is the path of its position in the
/// menus, and [`in_window_action`] finds the row's action from that path.
pub fn in_window_entries(window: &Window, cx: &mut App) -> Vec<crate::ui::menu::Entry> {
    use crate::ui::menu::{self as ui_menu, Entry as Row};
    let (preset, overrides) = crate::settings::keyboard(cx);
    fn rows(
        entries: &[Entry],
        path: &str,
        preset: KeyboardPreset,
        overrides: &Overrides,
        window: &Window,
        cx: &mut App,
    ) -> Vec<Row> {
        let mut out = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let path = format!("{path}.{index}");
            match entry {
                Entry::Separator => out.push(Row::Separator),
                Entry::Services => {}
                Entry::Submenu { entries, .. } => {
                    out.push(Row::Separator);
                    out.extend(rows(entries, &path, preset, overrides, window, cx));
                }
                Entry::Item(item) => {
                    let enabled = row_enabled(item, window, cx);
                    let mut row = ui_menu::item(item.action.name(), "")
                        .english(item.title.clone())
                        .value(path)
                        .disabled(!enabled);
                    if let Some(keys) = item.accelerator(preset, overrides) {
                        row = row.shortcut(registry::display(&keys));
                    }
                    if item.checked {
                        row = row.checked(true);
                    }
                    out.push(row.into());
                }
            }
        }
        out
    }
    let menus = in_window_menus(cx);
    let mut entries = Vec::new();
    for (index, menu) in menus.iter().enumerate() {
        let children = rows(
            &menu.entries,
            &index.to_string(),
            preset,
            &overrides,
            window,
            cx,
        );
        entries.push(ui_menu::submenu(menu.title, children).into());
    }
    entries
}

/// Returns the action of the row whose value is `path`, in the menu inside
/// the window. Returns `None` when `path` does not lead to a row.
pub fn in_window_action(path: &str, cx: &App) -> Option<Box<dyn Action>> {
    let menus = in_window_menus(cx);
    let mut indices = path.split('.').map(|part| part.parse::<usize>().ok());
    let mut entries = &menus.get(indices.next()??)?.entries;
    loop {
        match entries.get(indices.next()??)? {
            Entry::Item(item) => return Some(item.action.boxed_clone()),
            Entry::Submenu { entries: inner, .. } => entries = inner,
            Entry::Separator | Entry::Services => return None,
        }
    }
}
