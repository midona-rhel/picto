//! The menus as they are shown: the menu bar GPUI sets, and the in-window
//! menu that stands for it off the Mac.
use super::*;
use crate::settings::registry;

/// The menus as GPUI sets them.
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

/// Whether the window carries the application menu button
/// (`ApplicationMenuButton`): off the Mac, where there is no menu bar; on
/// the Mac too with `PICTO_GPUI_APP_MENU=window`, as production's
/// development build shows it there to check the Windows and Linux menu.
pub fn in_window() -> bool {
    !cfg!(target_os = "macos") || crate::seams::app_menu_in_window()
}

/// The in-window menu's width (`width={220}`).
pub const IN_WINDOW_WIDTH: f32 = 220.;

/// The menus the in-window menu shows: this platform's, without the Mac's
/// app menu, whose rows File and Help already hold
/// (`picto:application-menu:get`).
pub(super) fn in_window_menus(cx: &App) -> Vec<TopMenu> {
    let mut menus = model(&context(cx), Platform::CURRENT);
    if Platform::CURRENT.is_mac() {
        menus.remove(0);
    }
    menus
}

/// `toMenuEntries`: each menu a submenu of the button's menu; a submenu
/// inside one (Recent Libraries, Set Rating) is laid out in place after a
/// separator, as the shared menu nests only one level. A row is enabled as
/// the menu bar's would be: by production's rule, by this app performing
/// it, and by something in the window handling its action. Its value is
/// its place in the menus, which [`in_window_action`] reads back.
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
                    // Handled in the window, or app-wide (asked of the app
                    // while this window is being updated, only its own
                    // handlers count).
                    let action = item.action.as_ref();
                    let enabled = item.enabled
                        && (window.is_action_available(action, cx)
                            || cx.is_action_available(action));
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

/// The action of the in-window menu's row whose value is `path`.
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
